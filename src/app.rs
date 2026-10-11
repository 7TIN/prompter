//! The Prompter window: one utility surface for rewriting a rough prompt.
//!
//! State ownership, in one place:
//!
//! - `TextareaState` / `InputState` own the text. The view holds only the
//!   `Entity` handles and never a second copy of a buffer, so the two can never
//!   disagree;
//! - `ResizableState` owns the prompt/result split, so a drag survives a
//!   re-render;
//! - `Status` owns the one line of feedback, with its tone, so the message and
//!   its colour cannot be set independently.
//!
//! `render` builds elements and reads those entities. It starts no work and
//! mutates nothing.

// `Style` on its own would resolve to GPUI's styled trait through the glob, so
// the product's polish-style enum is always named for what it is. `theme` is
// re-exported because the window's backdrop is configured at construction time,
// before this module is involved.
use crate::rewrite;
use crate::style::Style as PolishStyle;
pub use crate::theme;

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{
    Input, InputContentType, InputEvent, InputState, Textarea, TextareaState,
};
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::theme::Theme;
use gpui_kit::component::*;
use gpui_kit::test::TestSupportExt as _;
use gpui_kit::*;
// The styled layer keeps a deliberately small compatibility icon enum; the
// assets bundle carries the full Lucide set, and that is the one applications
// draw from.
use gpui_kit::assets::IconName;

const KEY_FILE_NAME: &str = "prompter.key";

// Generate is the window's default commit, so it answers a keyboard as well as
// the pointer. The key binding and the toolbar Button both end in
// `PrompterView::generate`, the single place that decides whether a rewrite may
// start, so the two entry points cannot disagree about it.
gpui_kit::actions!(prompter, [Generate]);

/// Re-exported so a UI integration test can bind the same key the application
/// binds, instead of assuming the shortcut exists.
pub use Generate as GenerateAction;

/// The Generate button's two labels. It reserves the width of the longer one, so
/// swapping between them cannot reflow the toolbar beside it.
const GENERATE_LABEL: &str = "Generate";
const GENERATING_LABEL: &str = "Generating…";

/// Width reserved for the Generate button's label, icon and internal spacing.
const GENERATE_MIN_WIDTH: Rems = Rems(6.75);

/// How long the Generate button takes to cross-fade between its two states.
/// Matches the theme's `duration_normal`, which is the same curve the resize
/// handle and the rest of the component layer animate with.
const GENERATE_TRANSITION: Duration = Duration::from_millis(180);

fn read_clipboard() -> Option<String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .ok()
}

fn write_clipboard(text: &str) -> Result<(), arboard::Error> {
    arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
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
            Self::Typed => f.write_str("this session"),
            Self::File => f.write_str("saved key"),
            Self::Env => f.write_str("GEMINI_API_KEY"),
            Self::Offline => f.write_str("offline"),
        }
    }
}

/// How a status line should read. The tone picks the colour; the text always
/// carries the meaning, so the state never depends on colour alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StatusTone {
    Neutral,
    Working,
    Error,
}

/// The one line of feedback the window keeps at its foot. Cloning it is a refcount
/// bump, so `render` can hand it to a builder without copying a string.
#[derive(Clone, Debug)]
struct Status {
    text: SharedString,
    tone: StatusTone,
}

impl Status {
    fn new(text: impl Into<SharedString>, tone: StatusTone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }

    fn ready() -> Self {
        Self::new("Ready", StatusTone::Neutral)
    }

    fn color(&self, cx: &App) -> Hsla {
        match self.tone {
            StatusTone::Neutral => cx.theme().muted_foreground,
            StatusTone::Working => cx.theme().primary,
            StatusTone::Error => cx.theme().danger,
        }
    }
}

/// The Prompter window.
pub struct PrompterView {
    screen: Screen,
    prompt_input: Entity<TextareaState>,
    result_input: Entity<TextareaState>,
    key_input: Entity<InputState>,
    editors: Entity<ResizableState>,
    style: PolishStyle,
    status: Status,
    /// Whether a key has been typed into the setup field. The prompt editor
    /// deliberately has no such flag: nothing this window renders depends on the
    /// prompt, so a keystroke there re-renders the editor alone and never this
    /// view.
    key_typed: bool,
    busy: bool,
    generate_task: Option<Task<()>>,
    saved_key: String,
    env_key: String,
    _subscriptions: Vec<Subscription>,
}

impl PrompterView {
    /// Builds the window's initial state, including the prompt taken from the
    /// clipboard and the key loaded from disk or the environment.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
                .placeholder("Type or paste the prompt you want rewritten")
        });
        prompt_input.update(cx, |state, cx| {
            state.set_value(initial, window, cx);
        });

        // No subscription to the prompt editor: the window reads the prompt when
        // Generate is pressed, and nothing it draws depends on the prompt, so a
        // keystroke re-renders the editor and nothing else.

        let result_input = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder("The rewritten prompt appears here")
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
                    let typed = !state.read(cx).value().trim().is_empty();
                    if this.key_typed != typed {
                        this.key_typed = typed;
                        cx.notify();
                    }
                }
            },
        );

        // The `Generate` action is handled by an element in `render`, not by
        // `Context::on_action`: the latter registers on the window and may only
        // be called during paint, which a constructor is not.

        Self {
            screen,
            prompt_input,
            result_input,
            key_input,
            editors: cx.new(|_| ResizableState::default()),
            style: PolishStyle::default(),
            status: Status::ready(),
            key_typed: false,
            busy: false,
            generate_task: None,
            saved_key,
            env_key,
            _subscriptions: vec![key_subscription],
        }
    }

    /// The key a rewrite will actually use, in precedence order: this session's
    /// typed key, then the saved file, then the environment.
    ///
    /// Read from the input state rather than mirrored into this view, so there
    /// is one copy of the text instead of two that can disagree.
    fn effective_key(&self, cx: &App) -> Option<String> {
        let typed = self.key_input.read(cx).value();
        [
            typed.as_str(),
            self.saved_key.as_str(),
            self.env_key.as_str(),
        ]
        .into_iter()
        .map(str::trim)
        .find(|candidate| !candidate.is_empty())
        .map(ToOwned::to_owned)
    }

    fn key_source(&self, cx: &App) -> KeySource {
        if !self.key_input.read(cx).value().trim().is_empty() {
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
        let typed = self.key_input.read(cx).value().trim().to_owned();
        if !typed.is_empty() {
            match save_key(&typed) {
                Ok(()) => self.saved_key = typed,
                Err(error) => {
                    self.status = Status::new(
                        format!("Couldn’t save the key: {error}. It stays set for this session."),
                        StatusTone::Error,
                    );
                    self.saved_key = typed;
                }
            }
        }
        if self.status.tone != StatusTone::Error {
            self.status = Status::ready();
        }
        self.screen = Screen::Main;
        cx.notify();
    }

    fn skip_key(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Main;
        self.status = Status::new(
            "Offline mode — rewrites are local only",
            StatusTone::Neutral,
        );
        cx.notify();
    }

    fn refresh_from_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match read_clipboard() {
            Some(text) if !text.trim().is_empty() => {
                let chars = text.chars().count();
                self.prompt_input.update(cx, |state, cx| {
                    state.set_value(text, window, cx);
                });
                self.status =
                    Status::new(format!("Pasted {chars} characters"), StatusTone::Neutral);
            }
            _ => {
                self.status = Status::new("The clipboard is empty", StatusTone::Error);
            }
        }
        cx.notify();
    }

    fn set_style(&mut self, style: PolishStyle, cx: &mut Context<Self>) {
        self.style = style;
        cx.notify();
    }

    /// The keyboard entry point for the primary action. It reaches the same owner
    /// method the toolbar Button does, so neither can drift from the other.
    fn on_generate_action(&mut self, _: &Generate, window: &mut Window, cx: &mut Context<Self>) {
        self.generate(window, cx);
    }

    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // `Button::loading` already makes the button inert, and the action can be
        // dispatched from the keyboard, so the guard belongs here too.
        if self.busy {
            return;
        }
        let prompt = self.prompt_input.read(cx).value().trim().to_owned();
        if prompt.is_empty() {
            self.status = Status::new("Type or paste a prompt first", StatusTone::Error);
            cx.notify();
            return;
        }
        let key = self.effective_key(cx).unwrap_or_default();
        let source = self.key_source(cx);
        let style = self.style;

        self.busy = true;
        self.status = Status::new(format!("Rewriting with {source}…"), StatusTone::Working);
        cx.notify();

        let worker =
            cx.background_spawn(async move { rewrite::polish_blocking(&prompt, style, &key) });
        self.generate_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(polished) => {
                        this.result_input.update(cx, |state, cx| {
                            state.set_value(polished, window, cx);
                        });
                        this.status =
                            Status::new(format!("Rewritten with {source}"), StatusTone::Neutral);
                    }
                    Err(error) => {
                        this.status =
                            Status::new(format!("Couldn’t rewrite: {error}"), StatusTone::Error);
                    }
                }
                this.busy = false;
                cx.notify();
            })
            .ok();
        }));
    }

    /// Replaces the rewritten prompt.
    ///
    /// The same write path a completed rewrite uses, so a caller cannot leave the
    /// result textarea and the window's idea of the result out of step. Public so
    /// a UI integration test can present a long result through the production
    /// view instead of a fixture.
    pub fn set_result(
        &mut self,
        text: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.into();
        self.result_input.update(cx, |state, cx| {
            state.set_value(text, window, cx);
        });
        self.status = Status::ready();
        cx.notify();
    }

    fn copy_result(&mut self, cx: &mut Context<Self>) {
        let text = self.result_input.read(cx).value().trim().to_owned();
        if text.is_empty() {
            self.status = Status::new("There is nothing to copy yet", StatusTone::Error);
        } else {
            match write_clipboard(&text) {
                Ok(()) => self.status = Status::new("Copied", StatusTone::Neutral),
                Err(error) => {
                    self.status = Status::new(
                        format!("Couldn’t reach the clipboard: {error}"),
                        StatusTone::Error,
                    );
                }
            }
        }
        cx.notify();
    }

    fn render_setup(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status.clone();
        let edge = theme::edge(cx);
        let field = theme::field(cx);
        let muted = cx.theme().muted_foreground;

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_color(cx.theme().foreground)
            .child(
                // No fill, for the same reason as the main card: the window is
                // already the translucent surface.
                v_flex()
                    .w(rems(24.))
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(edge)
                    .child(div().text_xl().child("Prompter"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child("Rewrite a rough prompt into a clean one."),
                    )
                    .child(div().text_xs().text_color(muted).child("API key"))
                    .child(
                        Input::new(&self.key_input)
                            .content_type(InputContentType::Password)
                            .mask_toggle()
                            .bg(field),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Saved next to the app and asked once. Skip to work offline."),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(status.color(cx))
                            .child(status.text),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(
                                // The default variant, like every other quiet
                                // button here: `ghost` has no fill to show
                                // against a translucent window.
                                Button::new("skip").label("Skip").on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.skip_key(cx);
                                    },
                                )),
                            )
                            .child(
                                Button::new("continue")
                                    .primary()
                                    .icon(IconName::ArrowRight)
                                    .label("Continue")
                                    .disabled(!self.key_typed)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_key(cx);
                                    })),
                            ),
                    ),
            )
    }

    /// The style chooser and the two actions that apply to the whole window.
    ///
    /// One row, one baseline: the segmented control owns the leading edge, the
    /// actions own the trailing edge, and the space between them is what is left.
    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                ButtonGroup::new("style")
                    .small()
                    .compact()
                    .outline()
                    .children(PolishStyle::all().iter().map(|&style| {
                        Button::new(style.id())
                            .label(style.label())
                            .selected(style == self.style)
                    }))
                    .on_click(cx.listener(|this, selection: &Vec<usize>, _, cx| {
                        if let Some(style) = selection
                            .first()
                            .and_then(|&ix| PolishStyle::all().get(ix).copied())
                        {
                            this.set_style(style, cx);
                        }
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        // The default variant, not `ghost`: ghost is transparent
                        // by design, and a transparent control sitting on glass has
                        // no fill to read as a control at all.
                        //
                        // Paste here replaces the whole prompt rather than
                        // inserting at the cursor, which the label alone does not
                        // say. The tooltip earns its place for exactly that
                        // difference in scope.
                        Button::new("paste")
                            .icon(IconName::Clipboard)
                            .label("Paste")
                            .tooltip("Replace the prompt with the clipboard")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.refresh_from_clipboard(window, cx);
                            })),
                    )
                    .child(self.render_generate_button(cx)),
            )
    }

    /// The primary action, in its three states: `Generate`, `Generating…`, and
    /// back to `Generate`.
    ///
    /// Three details make the loop read as one control rather than a flicker:
    /// the width is reserved for the longer label, the spinner replaces the icon
    /// in the same slot, and the whole button cross-fades over one short
    /// transition when the state changes. `Button::loading` also stops the click
    /// and the keyboard action from firing twice, which is why it is not paired
    /// with `disabled` — a disabled button would repaint in the disabled palette
    /// and lose the primary emphasis mid-request.
    fn render_generate_button(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy;
        let label: AnyElement = if busy {
            ShimmerText::new(GENERATING_LABEL).into_any_element()
        } else {
            div().child(GENERATE_LABEL).into_any_element()
        };

        let button = Button::new("generate")
            .primary()
            .small()
            .icon(IconName::Wand)
            .loading(busy)
            // Reserved from the longer of the two labels, so the toolbar does not
            // reflow on the swap.
            .min_w(GENERATE_MIN_WIDTH)
            .accessibility_label(GENERATE_LABEL)
            .tooltip_with_action("Rewrite the prompt", &Generate, None)
            .child(label)
            .on_click(cx.listener(|this, _, window, cx| this.generate(window, cx)));

        // The animation is keyed on the state, so a change replays it and a
        // re-render for any other reason does not. `AnimationExt` already renders
        // the end state without scheduling frames when reduced motion is on, so
        // the guard below only avoids building an element nobody will animate.
        if cx.reduce_motion() {
            return button.into_any_element();
        }
        div()
            .id("generate-slot")
            .with_animation(
                ("generate-slot", busy as usize),
                Animation::new(GENERATE_TRANSITION).with_easing(ease_in_out),
                |this: Stateful<Div>, delta: f32| this.opacity(0.55 + 0.45 * delta),
            )
            .child(button)
            .into_any_element()
    }

    /// The two editors, split by a divider the user can drag.
    ///
    /// The panes are the scroll owners' parents and the `Textarea` inside each is
    /// the scroll owner, so neither needs a second scroll container wrapped
    /// around it. Each pane's height is bounded from below by the resizable's own
    /// `size_range`, which is what stops a long result from pushing the card off
    /// the bottom of the window.
    fn render_editors(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let editors = self.editors.clone();
        let prompt = self.prompt_input.clone();
        let result = self.result_input.clone();
        let has_result = !self.result_input.read(cx).value().trim().is_empty();
        let muted = cx.theme().muted_foreground;
        let field = theme::field(cx);

        v_resizable("editors")
            .with_state(&editors)
            .child(
                resizable_panel()
                    .size_range(theme::EDITOR_MIN_HEIGHT..Pixels::MAX)
                    .child(
                        v_flex()
                            .id("prompt-pane")
                            .test_support()
                            .size_full()
                            .min_h_0()
                            .gap_2()
                            .child(section_header("Prompt", muted, None))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    // Opaque, so reading never depends on what is
                                    // behind the window. The window surface around
                                    // it stays glass; the text surface does not.
                                    .child(Textarea::new(&prompt).size_full().bg(field)),
                            ),
                    ),
            )
            .child(
                resizable_panel()
                    .size_range(theme::EDITOR_MIN_HEIGHT..Pixels::MAX)
                    .child(
                        v_flex()
                            .id("result-pane")
                            .test_support()
                            .size_full()
                            .min_h_0()
                            .gap_2()
                            .child(section_header(
                                "Result",
                                muted,
                                Some(
                                    // Default variant, not `ghost`, for the same
                                    // reason as Paste: it needs a fill to read as a
                                    // control on a translucent window. A
                                    // recognizable symbol in a section already
                                    // titled "Result" needs an accessible name,
                                    // not a tooltip that would only restate it.
                                    Button::new("copy")
                                        .small()
                                        .icon(IconName::Copy)
                                        .disabled(!has_result)
                                        .accessibility_label("Copy the rewritten prompt")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.copy_result(cx);
                                        })),
                                ),
                            ))
                            .child(div().flex_1().min_h_0().child(
                                Textarea::new(&result).readonly(true).size_full().bg(field),
                            )),
                    ),
            )
    }

    /// The window's one line of feedback, and where the active key came from.
    ///
    /// This row shares the card's content inset, so its text starts on the same
    /// leading edge as the editors above it.
    fn render_status(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status.clone();
        let source = self.key_source(cx).to_string();
        let muted = cx.theme().muted_foreground;

        h_flex()
            .items_center()
            .justify_between()
            .gap_2()
            .border_t_1()
            .border_color(theme::edge(cx))
            .pt_2()
            .child(
                div()
                    .id("status")
                    .test_support()
                    .text_xs()
                    .text_color(status.color(cx))
                    .child(status.text),
            )
            .child(div().text_xs().text_color(muted).child(source))
    }

    /// The window's own title bar, drawn by the application.
    ///
    /// It is transparent on purpose. The component layer fills it with a
    /// gradient mixed from the theme's title-bar token, which would put an
    /// opaque band across the top of a glass window — the same mistake as a
    /// filled card. Only its hairline separator and its window controls remain,
    /// so the blurred backdrop runs uninterrupted from the window's top edge
    /// down through the editors.
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new().bg(cx.theme().transparent).child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Prompter"),
        )
    }

    fn render_main(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .text_color(cx.theme().foreground)
            // The window's default commit, handled on the main surface so a key
            // pressed while focus is inside either editor still reaches it: an
            // action dispatched from the focused node walks up through its
            // ancestors' handlers.
            .on_action(cx.listener(Self::on_generate_action))
            .child(
                // The card carries layout and an edge, and no fill. A wash here
                // would sit between the OS backdrop and the window surface and
                // read as flat grey rather than glass — the window itself is
                // already the translucent surface.
                v_flex()
                    .id("card")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .m_2()
                    .p_3()
                    .gap_3()
                    .rounded_xl()
                    .border_1()
                    .border_color(theme::edge(cx))
                    .child(self.render_toolbar(cx))
                    .child(self.render_editors(cx))
                    .child(self.render_status(cx)),
            )
            .into_any_element()
    }
}

/// A section's title row: the name on the leading edge, an optional action on
/// the trailing edge, both on one baseline.
///
/// The optional action is a real slot rather than a conditional gap: a section
/// without one keeps the title on the same leading edge as every other section,
/// which is what makes the two editors read as peers.
fn section_header(title: &'static str, muted: Hsla, action: Option<Button>) -> impl IntoElement {
    let trailing: AnyElement = match action {
        Some(button) => button.into_any_element(),
        None => div().into_any_element(),
    };
    h_flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(div().text_sm().text_color(muted).child(title))
        .child(trailing)
}

impl Render for PrompterView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The title bar is the window's own chrome and belongs to every screen:
        // it is what makes the window draggable and closable now that the
        // platform's caption is gone, so it cannot be owned by one of them.
        v_flex()
            .size_full()
            .relative()
            .text_color(cx.theme().foreground)
            // Raw GPUI, deliberately. The component layer owns exactly one
            // background for a window — a flat colour on the root surface — and a
            // flat tint is what reads as grey paint. The glass needs a second
            // layer with a falloff, so the window composes it itself.
            //
            // It is absolutely positioned and spans the whole window, title bar
            // included, so the light falls across the surface rather than
            // stopping at the top of the content area. Everything after it is
            // transparent, so this is the only thing painted over the backdrop.
            .child(
                div()
                    .id("glass-depth")
                    .absolute()
                    .inset_0()
                    .bg(theme::glass_depth(cx)),
            )
            .child(self.render_title_bar(cx))
            .child(match self.screen {
                Screen::Setup => self.render_setup(cx).into_any_element(),
                Screen::Main => self.render_main(cx).into_any_element(),
            })
            .into_any_element()
    }
}

/// Registers the global hotkey on its own thread.
///
/// The manager must live on the thread that pumps the Win32 loop. `recv` blocks,
/// so the thread costs nothing while idle; there is no polling loop.
pub fn spawn_hotkey_listener() -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {
        use global_hotkey::{
            GlobalHotKeyEvent, GlobalHotKeyManager,
            hotkey::{Code, HotKey, Modifiers},
        };

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

        // v1 only logs; bringing the GPUI window to the front is v1.1.
        while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
            let chars = read_clipboard().map_or(0, |text| text.chars().count());
            println!("hotkey event: {event:?} (clipboard {chars} chars)");
        }
    })
}

/// Binds the window's keys and applies the product theme.
///
/// Order matters: the font edit runs first because the translucency is written
/// on top of whatever background the active theme registered, and the bindings
/// run before anything reads the keymap.
pub fn prepare_app(cx: &mut App) {
    // The font is resolved first: the translucency is written on top of whatever
    // background the active theme registered.
    let family = install_fonts(cx);
    Theme::update(cx, |theme| {
        theme.font_family = family.into();
    });
    theme::install(cx);
    cx.bind_keys([KeyBinding::new("ctrl-enter", Generate, None)]);
}

/// The window's default size, and the floor below which its layout stops
/// working: two editors side by side plus a toolbar and a status row.
pub const WINDOW_SIZE: gpui::Size<gpui::Pixels> = gpui::size(px(780.), px(560.));
pub const WINDOW_MIN_SIZE: gpui::Size<gpui::Pixels> = gpui::size(px(640.), px(420.));

fn font_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        dirs.push(dir.join("assets").join("fonts"));
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
