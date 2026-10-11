mod rewrite;
mod style;

use std::fmt;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::*;
use gpui_kit::*;
use style::Style;

fn read_clipboard() -> Option<String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .ok()
}

fn write_clipboard(text: &str) -> Result<(), arboard::Error> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
}

fn load_api_key_from_env() -> String {
    std::env::var("GEMINI_API_KEY").unwrap_or_default()
}

fn mask_key(key: &str) -> String {
    let key = key.trim();
    match key.len() {
        0 => "not set".to_owned(),
        1..=4 => "****".to_owned(),
        _ => format!("...{}", &key[key.len() - 4..]),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KeySource {
    Typed,
    Env,
    Offline,
}

impl fmt::Display for KeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Typed => f.write_str("typed"),
            Self::Env => f.write_str("env"),
            Self::Offline => f.write_str("none (offline)"),
        }
    }
}

struct PrompterView {
    prompt_text: String,
    prompt_input: Entity<TextareaState>,
    output: String,
    style: Style,
    status: String,
    api_key_env: String,
    api_key_typed: String,
    api_key_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl PrompterView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial = read_clipboard().unwrap_or_default();
        let prompt_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(6)
                .placeholder("Type/paste prompt here, or Refresh from Clipboard")
        });
        prompt_input.update(cx, |state, cx| {
            state.set_value(initial.clone(), window, cx);
        });
        let prompt_subscription = cx.subscribe_in(
            &prompt_input,
            window,
            |this: &mut Self, state, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.prompt_text = state.read(cx).value().to_string();
                    cx.notify();
                }
            },
        );

        let api_key_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste Gemini API key here (or set GEMINI_API_KEY)")
        });
        let key_subscription = cx.subscribe_in(
            &api_key_input,
            window,
            |this: &mut Self, state, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.api_key_typed = state.read(cx).value().to_string();
                    cx.notify();
                }
            },
        );

        let api_key_env = load_api_key_from_env();
        let status = if api_key_env.trim().is_empty() {
            "Paste API key above, or set GEMINI_API_KEY env. Empty = offline mode.".to_owned()
        } else {
            "Ready. Env key found; typed key overrides it.".to_owned()
        };

        Self {
            prompt_text: initial,
            prompt_input,
            output: String::new(),
            style: Style::default(),
            status,
            api_key_env,
            api_key_typed: String::new(),
            api_key_input,
            _subscriptions: vec![prompt_subscription, key_subscription],
        }
    }

    fn effective_key(&self) -> Option<&str> {
        let typed = self.api_key_typed.trim();
        if !typed.is_empty() {
            return Some(typed);
        }
        let env = self.api_key_env.trim();
        (!env.is_empty()).then_some(env)
    }

    fn key_source(&self) -> KeySource {
        if !self.api_key_typed.trim().is_empty() {
            KeySource::Typed
        } else if !self.api_key_env.trim().is_empty() {
            KeySource::Env
        } else {
            KeySource::Offline
        }
    }

    fn reload_env_key(&mut self, cx: &mut Context<Self>) {
        self.api_key_env = load_api_key_from_env();
        self.status = format!(
            "Env reloaded. Using {} key {}.",
            self.key_source(),
            mask_key(self.effective_key().unwrap_or_default())
        );
        cx.notify();
    }

    fn refresh_from_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match read_clipboard() {
            Some(text) if !text.trim().is_empty() => {
                self.status = format!("Loaded {} chars from clipboard.", text.len());
                self.prompt_text = text.clone();
                self.prompt_input.update(cx, |state, cx| {
                    state.set_value(text, window, cx);
                });
            }
            _ => {
                self.status = "Clipboard is empty. Copy text then Refresh.".to_owned();
            }
        }
        cx.notify();
    }

    fn set_style(&mut self, style: Style, cx: &mut Context<Self>) {
        self.style = style;
        self.status = format!("Style: {style}");
        cx.notify();
    }

    fn generate(&mut self, cx: &mut Context<Self>) {
        if self.prompt_text.trim().is_empty() {
            self.status =
                "Nothing to polish. Type in the box or Refresh from clipboard.".to_owned();
            cx.notify();
            return;
        }
        let key = self.effective_key().unwrap_or_default().to_owned();
        let source = self.key_source();
        self.status = format!("Polishing with {} (key: {source})...", self.style);
        cx.notify();
        // Blocking call on the UI thread for v1; move to a background
        // executor in v1.1.
        match rewrite::polish_blocking(&self.prompt_text, self.style, &key) {
            Ok(polished) => {
                self.output = polished;
                self.status = "Done. Use Copy Result.".to_owned();
            }
            Err(error) => {
                self.status = format!("Error: {error}");
            }
        }
        cx.notify();
    }

    fn copy_result(&mut self, cx: &mut Context<Self>) {
        if self.output.trim().is_empty() {
            self.status = "No output to copy.".to_owned();
        } else {
            match write_clipboard(&self.output) {
                Ok(()) => self.status = "Copied to clipboard.".to_owned(),
                Err(error) => self.status = format!("Clipboard: {error}"),
            }
        }
        cx.notify();
    }
}

impl Render for PrompterView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let output = if self.output.is_empty() {
            "(output appears here)".to_owned()
        } else {
            self.output.clone()
        };
        let status = self.status.clone();
        let key_info = format!(
            "Key: {} ({})",
            mask_key(self.effective_key().unwrap_or_default()),
            self.key_source()
        );

        div()
            .v_flex()
            .gap_2()
            .p_4()
            .size_full()
            .child("Prompter - Ctrl+Alt+P (window must be open in v1)")
            .child("Gemini API key (typed overrides env, paste with Ctrl+V):")
            .child(Input::new(&self.api_key_input))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(key_info)
                    .child(
                        Button::new("reload-env")
                            .label("Reload Env Key")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reload_env_key(cx);
                            })),
                    ),
            )
            .child(format!("Status: {status}"))
            .child("Style:")
            .child({
                let mut row = div().h_flex().gap_2();
                for &style in Style::all() {
                    let button = if style == self.style {
                        Button::new(style.id()).primary().label(style.label())
                    } else {
                        Button::new(style.id()).label(style.label())
                    };
                    row = row.child(button.on_click(cx.listener(move |this, _, _, cx| {
                        this.set_style(style, cx);
                    })));
                }
                row
            })
            .child(format!("Prompt [{}] - type below or Refresh:", self.style))
            .child(Textarea::new(&self.prompt_input))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        Button::new("refresh")
                            .label("Refresh from Clipboard")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.refresh_from_clipboard(window, cx);
                            })),
                    )
                    .child(
                        Button::new("generate")
                            .primary()
                            .label("Generate")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.generate(cx);
                            })),
                    )
                    .child(
                        Button::new("copy")
                            .label("Copy Result")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.copy_result(cx);
                            })),
                    ),
            )
            .child("Output:")
            .child(output)
    }
}

fn spawn_hotkey_listener() -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {
        use global_hotkey::{
            GlobalHotKeyEvent,
            GlobalHotKeyManager,
            hotkey::{Code, HotKey, Modifiers},
        };

        // The manager must live on the thread that pumps the Win32 loop.
        let Ok(manager) = GlobalHotKeyManager::new() else {
            eprintln!("hotkey manager failed to initialize");
            return;
        };
        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyP);
        if let Err(error) = manager.register(hotkey) {
            eprintln!("hotkey register failed (Ctrl+Alt+P): {error}");
            return;
        }
        println!("Hotkey registered: Ctrl+Alt+P");

        // Blocking receive: zero CPU while idle, no polling loop.
        // v1 only logs; bringing the GPUI window to the front is v1.1.
        while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
            let chars = read_clipboard().map(|text| text.len()).unwrap_or(0);
            println!("hotkey event: {event:?} (clipboard {chars} chars)");
        }
    })
}

fn main() {
    let _hotkeys = spawn_hotkey_listener();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| PrompterView::new(window, cx))
            })
            .expect("failed to open window");
        });
}
