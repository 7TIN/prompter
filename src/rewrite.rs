use std::{fmt, sync::OnceLock, time::Duration};

use serde_json::{Value, json};

use crate::style::Style;

/// Default model. Override with the `GEMINI_MODEL` env var.
pub const DEFAULT_MODEL: &str = "gemini-3.6-flash";

/// How long one rewrite may take before it is reported as failed.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum PolishError {
    EmptyInput,
    Transport(reqwest::Error),
    /// Building the shared HTTP client failed, reported by message because the
    /// error itself is stored once and cannot be cloned back out.
    TransportMessage(String),
    Api {
        status: reqwest::StatusCode,
        body: String,
    },
    Decode(serde_json::Error),
    EmptyResponse,
    ApiMessage(String),
}

impl fmt::Display for PolishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => f.write_str("input is empty"),
            Self::Transport(e) => write!(f, "request failed: {e}"),
            Self::TransportMessage(message) => {
                write!(f, "could not open an HTTP connection: {message}")
            }
            Self::Api { status, body } => write!(f, "Gemini {status}: {body}"),
            Self::Decode(e) => write!(f, "invalid response: {e}"),
            Self::EmptyResponse => f.write_str("empty response from model"),
            Self::ApiMessage(msg) => write!(f, "Gemini error: {msg}"),
        }
    }
}

impl std::error::Error for PolishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(e) => Some(e),
            Self::Decode(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for PolishError {
    fn from(error: reqwest::Error) -> Self {
        Self::Transport(error)
    }
}

impl From<serde_json::Error> for PolishError {
    fn from(error: serde_json::Error) -> Self {
        Self::Decode(error)
    }
}

fn model_id() -> String {
    std::env::var("GEMINI_MODEL")
        .ok()
        .map(|model| model.trim().to_owned())
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned())
}

fn system_prompt(style: Style) -> String {
    format!(
        "You are a prompt-polishing tool. The user writes long prompts with bad grammar and verbose explanations. \
         Rewrite the given text into a clean prompt for an AI agent. Rules: fix grammar, be concise, \
         replace verbose explanations with the correct single technical word where possible, \
         preserve intent and all requirements, do not add new requirements, do not answer the prompt, \
         output ONLY the rewritten prompt with no preamble. Style: {style}. Instruction: {}",
        style.instruction()
    )
}

fn normalize_whitespace(input: &str) -> String {
    let without_cr = input.replace('\r', "");
    let collapsed = without_cr
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    // Collapse runs of blank lines into a single blank line.
    let mut lines = Vec::new();
    let mut blank_run = 0;
    for line in collapsed.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                lines.push(String::new());
            }
        } else {
            blank_run = 0;
            lines.push(line.to_owned());
        }
    }
    lines.join("\n").trim().to_owned()
}

fn capitalize_first(input: &str) -> String {
    let mut chars = input.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Offline fallback when no API key is set. Deterministic, no network.
pub fn offline_polish(input: &str, style: Style) -> String {
    let normalized = normalize_whitespace(input);
    if normalized.is_empty() {
        return String::new();
    }
    let polished = capitalize_first(&normalized);
    match style {
        Style::Short | Style::Brevity => {
            let first_two: String = polished.split_inclusive(['.', '?', '!']).take(2).collect();
            format!("{} (offline brevity polish)", first_two.trim())
        }
        Style::Direct => {
            format!("{polished} (offline direct polish: do exactly as specified)")
        }
        Style::Casual => format!("{polished} (offline casual polish)"),
        Style::Informed => format!("{polished} (offline informed polish)"),
        Style::Normal => format!("{polished} (offline grammar polish)"),
    }
}

/// One HTTP client for the process.
///
/// Building a `reqwest::blocking::Client` per request repeats TLS setup and
/// connection-pool construction on every rewrite, which is pure latency the user
/// waits through. The client is immutable once built, so it is built on first
/// use and shared. `reqwest::Error` is not `Clone`, so a construction failure is
/// stored as its message rather than as the error itself.
fn http_client() -> Result<&'static reqwest::blocking::Client, PolishError> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    match CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| error.to_string())
    }) {
        Ok(client) => Ok(client),
        Err(message) => Err(PolishError::TransportMessage(message.clone())),
    }
}

/// Blocking Gemini call.
pub fn polish_blocking(input: &str, style: Style, api_key: &str) -> Result<String, PolishError> {
    if api_key.trim().is_empty() {
        return Ok(offline_polish(input, style));
    }
    if input.trim().is_empty() {
        return Err(PolishError::EmptyInput);
    }

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model_id(),
        api_key.trim()
    );
    let body = json!({
        "contents": [{
            "parts": [{ "text": format!("{}\n\n---\nUser text to rewrite:\n{}", system_prompt(style), input) }]
        }],
        "generationConfig": { "maxOutputTokens": 1024 }
    });

    let response = http_client()?.post(&url).json(&body).send()?;
    let status = response.status();
    let body = response.text()?;

    if !status.is_success() {
        let snippet: String = body.chars().take(500).collect();
        return Err(PolishError::Api {
            status,
            body: snippet,
        });
    }

    let json: Value = serde_json::from_str(&body)?;
    let text = json
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned);

    match text {
        Some(text) => Ok(text),
        None => match json.pointer("/error/message").and_then(Value::as_str) {
            Some(message) => Err(PolishError::ApiMessage(message.to_owned())),
            None => Err(PolishError::EmptyResponse),
        },
    }
}
