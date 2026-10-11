mod rewrite;
mod style;

use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{
    Input, InputContentType, InputEvent, InputState, Textarea, TextareaState,
};
use gpui_kit::component::theme::Theme;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::IconName;
use gpui_kit::component::*;
use gpui_kit::*;
use style::Style;

const KEY_FILE_NAME: &str = "prompter.key";

fn read_clipboard() -> Option<String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .ok()
}

fn write_clipboard(text: &str) -> Result<(), arboard::Error> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
}

fn key_store_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|dir| dir.join(KEY_FILE_NAME))
}

fn load_saved_key() -> Option<String> {
    let path = key_store_path()?;
    std::fs::read_to_string(path)
        .ok()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
}

fn save_key(key: &str) -> std::io::Result<()> {
    let path = key_store_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "exe directory unknown")
    })?;
    std::fs::write(path, key.trim())
}

fn load_env_key() -> String {
    std::env::var("GEMINI_API_KEY").unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Setup,
    Main,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KeySource {
    Typed,
    File,
    Env,
    Offline,
}

impl fmt::Display for KeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Typed => f.write_str("typed"),
            Self::File => f.write_str("saved"),
            Self::Env => f.write_str("env"),
            Self::Offline => f.write_str("offline"),
        }
    }
}

struct PrompterView {
    screen: Screen,
    prompt_text: String,
    prompt_input: Entity<TextareaState>,
    output_state: Entity<TextareaState>,
    output: String,
    style: Style,
    status: String,
    saved_key: String,
    env_key: String,
    typed_key: String,
    key_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl PrompterView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved_key = load_saved_key().unwrap_or_default();
        let env_key = load_env_key();
        let screen = if saved_key.trim().is_empty() && env_key.trim().is_empty() {
            Screen::Setup
        } else {
            Screen::Main
        };

        let initial = read_clipboard().unwrap_or_default();
        let prompt_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 4)
                .placeholder("Type here, or pull from clipboard")
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

        let output_state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 4)
                .placeholder("Result")
        });

        let key_input = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Gemini API key")
        });
        let key_subscription = cx.subscribe_in(
            &key_input,
            window,
            |this: &mut Self, state, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.typed_key = state.read(cx).value().to_string();
                    cx.notify();
                }
            },
        );

        Self {
            screen,
            prompt_text: initial,
            prompt_input,
            output_state,
            output: String::new(),
            style: Style::default(),
            status: String::new(),
            saved_key,
            env_key,
            typed_key: String::new(),
            key_input,
            _subscriptions: vec![prompt_subscription, key_subscription],
        }
    }

    fn effective_key(&self) -> Option<&str> {
        for candidate in [&self.typed_key, &self.saved_key, &self.env_key] {
            let trimmed = candidate.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        None
    }

    fn key_source(&self) -> KeySource {
        if !self.typed_key.trim().is_empty() {
            KeySource::Typed
        } else if !self.saved_key.trim().is_empty() {
            KeySource::File
        } else if !self.env_key.trim().is_empty() {
            KeySource::Env
        } else {
            KeySource::Offline
        }
    }

    fn confirm_key(&mut self, cx: &mut Context<Self>) {
        let typed = self.typed_key.trim().to_owned();
        if !typed.is_empty() {
            match save_key(&typed) {
                Ok(()) => self.saved_key = typed,
                Err(error) => {
                    self.status = format!("Keep going, save failed: {error}");
                    self.saved_key = typed;
                }
            }
        }
        self.screen = Screen::Main;
        self.status = "Ready".to_owned();
        cx.notify();
    }

    fn skip_key(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Main;
        self.status = "Offline mode".to_owned();
        cx.notify();
    }

    fn refresh_from_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match read_clipboard() {
            Some(text) if !text.trim().is_empty() => {
                self.status = format!("{} chars", text.len());
                self.prompt_text = text.clone();
                self.prompt_input.update(cx, |state, cx| {
                    state.set_value(text, window, cx);
                });
            }
            _ => {
                self.status = "Clipboard empty".to_owned();
            }
        }
        cx.notify();
    }

    fn set_style(&mut self, style: Style, cx: &mut Context<Self>) {
        self.style = style;
        cx.notify();
    }

    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt_text.trim().is_empty() {
            self.status = "Empty prompt".to_owned();
            cx.notify();
            return;
        }
        let key = self.effective_key().unwrap_or_default().to_owned();
        let source = self.key_source();
        self.status = format!("Working ({}), {}", self.style, source);
        cx.notify();
        // Blocking call on the UI thread for v1; move to a background
        // executor in v1.1.
        match rewrite::polish_blocking(&self.prompt_text, self.style, &key) {
            Ok(polished) => {
                self.output = polished.clone();
                self.output_state.update(cx, |state, cx| {
                    state.set_value(polished, window, cx);
                });
                self.status = String::new();
            }
            Err(error) => {
                self.status = format!("Error: {error}");
            }
        }
        cx.notify();
    }

    fn copy_result(&mut self, cx: &mut Context<Self>) {
        if self.output.trim().is_empty() {
            self.status = "Nothing to copy".to_owned();
        } else {
            match write_clipboard(&self.output) {
                Ok(()) => self.status = "Copied".to_owned(),
                Err(error) => self.status = format!("Clipboard: {error}"),
            }
        }
        cx.notify();
    }

    fn render_setup(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status.clone();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_color(cx.theme().foreground)
            .child(
                v_flex()
                    .w(px(380.))
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(rgba(0x80808040))
                    .shadow_lg()
                    .bg(cx.theme().background.opacity(0.85))
                    .child(div().text_xl().child("Prompter"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("API Key"),
                    )
                    .child(
                        Input::new(&self.key_input)
                            .content_type(InputContentType::Password)
                            .mask_toggle(),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Saved next to the app. Asked once."),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .justify_end()
                            .child(
                                Button::new("skip")
                                    .label("Skip")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.skip_key(cx);
                                    })),
                            )
                            .child(
                                Button::new("continue")
                                    .primary()
                                    .icon(IconName::ArrowRight)
                                    .label("Continue")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_key(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(status),
                    ),
            )
    }

    fn render_main(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status.clone();
        v_flex()
            .size_full()
            .text_color(cx.theme().foreground)
            .child(
                v_flex()
                    .flex_1()
                    .gap_2()
                    .m_2()
                    .p_3()
                    .rounded_xl()
                    .border_1()
                    .border_color(rgba(0x80808040))
                    .shadow_lg()
                    .bg(cx.theme().background.opacity(0.85))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .justify_between()
                            .items_center()
                            .child({
                                let mut row = div().h_flex().gap_1().flex_wrap();
                                for &style in Style::all() {
                                    let button = if style == self.style {
                                        Button::new(style.id())
                                            .primary()
                                            .small()
                                            .label(style.label())
                                    } else {
                                        Button::new(style.id()).small().label(style.label())
                                    };
                                    row = row.child(button.on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.set_style(style, cx);
                                        },
                                    )));
                                }
                                row
                            })
                            .child(
                                div()
                                    .h_flex()
                                    .gap_2()
                                    .flex_shrink_0()
                                    .child(
                                        Button::new("paste")
                                            .small()
                                            .label("Paste")
                                            .tooltip("Paste from clipboard")
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    this.refresh_from_clipboard(window, cx);
                                                }),
                                            ),
                                    )
                                    .child(
                                        Button::new("generate")
                                            .primary()
                                            .small()
                                            .label("Generate")
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    this.generate(window, cx);
                                                }),
                                            ),
                                    ),
                            ),
                    )
                    .child(Textarea::new(&self.prompt_input))
                    .child(div().flex_1())
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(status),
                            )
                            .child(
                                Button::new("copy")
                                    .small()
                                    .icon(IconName::Copy)
                                    .tooltip("Copy result")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.copy_result(cx);
                                    })),
                            ),
                    )
                    .child(
                        Textarea::new(&self.output_state).readonly(true),
                    ),
            )
    }
}

impl Render for PrompterView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.screen {
            Screen::Setup => self.render_setup(cx).into_any_element(),
            Screen::Main => self.render_main(window, cx).into_any_element(),
        }
    }
}

fn spawn_hotkey_listener() -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {
        use global_hotkey::{
            GlobalHotKeyEvent, GlobalHotKeyManager,
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

fn font_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.join("assets").join("fonts"));
        }
    }
    dirs.push(PathBuf::from("assets").join("fonts"));
    dirs
}

/// Loads bundled Geist if present, falling back to a smooth system font.
/// Drop `Geist-Regular.ttf` (+ optional `Geist-Bold.ttf`) into
/// `assets/fonts/` next to the exe or the project root.
fn install_fonts(cx: &mut App) -> String {
    for dir in font_search_dirs() {
        let regular = dir.join("Geist-Regular.ttf");
        if !regular.is_file() {
            continue;
        }
        let mut paths = vec![regular];
        let bold = dir.join("Geist-Bold.ttf");
        if bold.is_file() {
            paths.push(bold);
        }
        let bytes: Vec<Cow<'static, [u8]>> = paths
            .iter()
            .filter_map(|path| std::fs::read(path).ok())
            .map(Cow::Owned)
            .collect();
        if bytes.is_empty() {
            continue;
        }
        match cx.text_system().add_fonts(bytes) {
            Ok(()) => return "Geist".to_owned(),
            Err(error) => eprintln!("bundled font failed: {error}"),
        }
    }
    "Segoe UI Variable Text".to_owned()
}

fn main() {
    let _hotkeys = spawn_hotkey_listener();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let family = install_fonts(cx);
            Theme::update(cx, |theme| {
                theme.font_family = family.into();
            });
            let bounds = Bounds::centered(None, size(px(750.), px(300.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_background: WindowBackgroundAppearance::Blurred,
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title("Prompter");
                cx.new(|cx| PrompterView::new(window, cx))
            })
            .expect("failed to open window");
        });
}
