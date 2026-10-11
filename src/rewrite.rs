use serde_json::{json, Value};

use crate::style::Style;

fn system_prompt(style: Style) -> String {
    format!(
        "You are a prompt-polishing tool. The user writes long prompts with bad grammar and verbose explanations. \
Rewrite the given text into a clean prompt for an AI agent. Rules: fix grammar, be concise, \
replace verbose explanations with the correct single technical word where possible, \
preserve intent and all requirements, do not add new requirements, do not answer the prompt, \
output ONLY the rewritten prompt with no preamble. Style: {}. Instruction: {}",
        style.label(),
        style.instruction()
    )
}

/// Offline fallback when no API key is set. Deterministic, no network.
pub fn offline_polish(input: &str, style: Style) -> String {
    let mut s = input.trim().to_string();
    // Collapse whitespace runs into single spaces, keep single newlines between paragraphs.
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == '\r' {
            continue;
        }
        if ch.is_whitespace() && ch != '\n' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else if ch == '\n' {
            // keep at most one blank line
            if !out.ends_with('\n') {
                out.push('\n');
            }
            prev_space = false;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    s = out.trim().to_string();
    if s.is_empty() {
        return String::new();
    }
    // Capitalize first letter.
    let mut chars: Vec<char> = s.chars().collect();
    if let Some(first) = chars.first_mut() {
        *first = first.to_ascii_uppercase();
    }
    s = chars.into_iter().collect();
    match style {
        Style::Short | Style::Brevity => {
            // crude brevity: keep first 2 sentences max
            let parts: Vec<&str> = s.split_inclusive(&['.', '?', '!'][..]).collect();
            if parts.len() > 2 {
                s = parts[..2].concat();
            }
            format!("{} (offline brevity polish)", s.trim())
        }
        Style::Direct => format!("{} (offline direct polish: do exactly as specified)", s.trim()),
        Style::Casual => format!("{} (offline casual polish)", s.trim()),
        Style::Informed => format!("{} (offline informed polish)", s.trim()),
        Style::Normal => format!("{} (offline grammar polish)", s.trim()),
    }
}

/// Blocking Gemini call. Returns rewritten prompt or error string.
pub fn polish_blocking(input: &str, style: Style, api_key: &str) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Ok(offline_polish(input, style));
    }
    if input.trim().is_empty() {
        return Err("Input is empty.".to_string());
    }
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key={}",
        api_key.trim()
    );
    let body = json!({
        "contents": [{
            "parts": [{ "text": format!("{}\n\n---\nUser text to rewrite:\n{}", system_prompt(style), input) }]
        }],
        "generationConfig": { "temperature": 0.3, "maxOutputTokens": 1024 }
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("http client error: {e}"))?;
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        let short = text.chars().take(500).collect::<String>();
        return Err(format!("Gemini {status}: {short}"));
    }
    let v: Value = resp.json().map_err(|e| format!("bad json: {e}"))?;
    // candidates[0].content.parts[0].text
    let text = v
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|t| t.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if text.is_empty() {
        // surface API error message if present
        if let Some(err) = v.pointer("/error/message").and_then(|m| m.as_str()) {
            return Err(format!("Gemini error: {err}"));
        }
        return Err("Empty response from model.".to_string());
    }
    Ok(text)
}
