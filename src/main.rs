mod rewrite;
mod style;

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use style::Style;

fn read_clipboard() -> String {
    match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
        Ok(t) => t,
        Err(_) => String::new(),
    }
}

fn write_clipboard(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_string()))
        .map_err(|e| format!("clipboard error: {e}"))
}

fn load_api_key() -> String {
    std::env::var("GEMINI_API_KEY").unwrap_or_default()
}

struct PrompterView {
    input: String,
    output: String,
    style: Style,
    status: String,
    api_key: String,
}

impl PrompterView {
    fn new(_cx: &mut Context<Self>) -> Self {
        let key = load_api_key();
        let status = if key.trim().is_empty() {
            "Offline mode (set GEMINI_API_KEY for Gemini)".to_string()
        } else {
            "Ready (Gemini)".to_string()
        };
        Self {
            input: read_clipboard(),
            output: String::new(),
            style: Style::default(),
            status,
            api_key: key,
        }
    }

    fn refresh_from_clipboard(&mut self, cx: &mut Context<Self>) {
        self.input = read_clipboard();
        if self.input.trim().is_empty() {
            self.status = "Clipboard is empty. Copy text then Refresh.".to_string();
        } else {
            self.status = format!("Loaded {} chars from clipboard.", self.input.len());
        }
        cx.notify();
    }

    fn set_style(&mut self, s: Style, cx: &mut Context<Self>) {
        self.style = s;
        self.status = format!("Style: {}", s.label());
        cx.notify();
    }

    fn generate(&mut self, cx: &mut Context<Self>) {
        if self.input.trim().is_empty() {
            self.status = "Nothing to polish. Refresh from clipboard first.".to_string();
            cx.notify();
            return;
        }
        self.status = format!("Polishing with {}...", self.style.label());
        cx.notify();
        // Blocking call on UI thread for v1 (1-3s freeze on Generate is acceptable).
        // v1.1: move to background executor.
        match rewrite::polish_blocking(&self.input, self.style, &self.api_key) {
            Ok(text) => {
                self.output = text;
                self.status = "Done. Use Copy Result.".to_string();
            }
            Err(e) => {
                self.status = format!("Error: {e}");
            }
        }
        cx.notify();
    }

    fn copy_result(&mut self, cx: &mut Context<Self>) {
        if self.output.trim().is_empty() {
            self.status = "No output to copy.".to_string();
        } else {
            match write_clipboard(&self.output) {
                Ok(_) => self.status = "Copied to clipboard.".to_string(),
                Err(e) => self.status = e,
            }
        }
        cx.notify();
    }
}

impl Render for PrompterView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input_preview = if self.input.len() > 2000 {
            format!("{}... [{} chars total]", &self.input[..2000], self.input.len())
        } else if self.input.is_empty() {
            "(empty - copy text anywhere, then Refresh)".to_string()
        } else {
            self.input.clone()
        };
        let output_preview = if self.output.is_empty() {
            "(output appears here)".to_string()
        } else {
            self.output.clone()
        };
        let status = self.status.clone();

        div()
            .v_flex()
            .gap_2()
            .p_4()
            .size_full()
            .child("Prompter - Ctrl+Alt+P (window must be open in v1)")
            .child(format!("Status: {status}"))
            .child("Style:")
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child({
                        let mut row = div().h_flex().gap_2();
                        for s in Style::all() {
                            let st = *s;
                            let btn = if st == self.style {
                                Button::new(st.id()).primary().label(st.label())
                            } else {
                                Button::new(st.id()).label(st.label())
                            };
                            row = row.child(btn.on_click(cx.listener(move |this, _, _, cx| {
                                this.set_style(st, cx);
                            })));
                        }
                        row
                    }),
            )
            .child(format!("Input [{}]:", self.style.label()))
            .child(input_preview)
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        Button::new("refresh")
                            .label("Refresh from Clipboard")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_from_clipboard(cx);
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
            .child(output_preview)
    }
}

fn spawn_hotkey_listener() {
    std::thread::spawn(|| {
        use global_hotkey::{
            GlobalHotKeyEvent,
            hotkey::{Code, HotKey, Modifiers},
            GlobalHotKeyManager,
        };
        // Must create manager on this thread (Windows Win32 loop requirement).
        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("hotkey manager failed: {e}");
                return;
            }
        };
        let hk = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyP);
        if let Err(e) = manager.register(hk) {
            eprintln!("hotkey register failed (Ctrl+Alt+P): {e}");
            return;
        }
        println!("Hotkey registered: Ctrl+Alt+P");
        loop {
            if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                // v1: log + snapshot clipboard length. Window focus-steal is v1.1.
                let n = read_clipboard().len();
                println!("hotkey event: {:?} (clipboard {} chars)", event, n);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

fn main() {
    spawn_hotkey_listener();
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(PrompterView::new)
        })
        .expect("Failed to open window");
    });
}
