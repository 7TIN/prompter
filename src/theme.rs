//! The product's theme layer.
//!
//! Prompter is a light window: the *chrome* — the window surface itself — is a
//! translucent white sheet over the desktop, and the *content* sits on top of
//! it in solid light neutrals (white wells, grey controls) with near-black
//! text. Only the window surface turns down; buttons, editors and text stay
//! fully opaque, because a control you cannot see the edge of is not a
//! control. That split is the whole design, and it is why the colours live
//! here rather than at the call sites.
//!
//! Blur is shelved, not attempted: the gradient-transparency material behind
//! the default needs no blur from the compositor, which is exactly why it is
//! the one material observed working. The untinted-acrylic override lives on
//! in [`crate::acrylic`] behind `PROMPTER_BACKDROP=blurred`.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::theme::Theme;
use gpui_kit::{App, Hsla, Pixels, WindowBackgroundAppearance, px, rgb};

/// Tint opacity of the window surface, as `(light, dark)`.
///
/// Only used when the backdrop is *not* opaque (the shelved glass variant). The
/// window is composited by the platform and the application paints one
/// translucent layer over it, so this number decides how much of the desktop
/// reads through.
///
/// It is a genuine trade, not a preference:
///
/// - lower reads more like glass and shows the desktop, but the chrome's own
///   text — the section labels and the status line — sits directly on an
///   arbitrary backdrop and stops being reliably readable;
/// - higher keeps the chrome readable anywhere and starts to look like flat
///   grey, because a thick enough wash stops the backdrop from mattering.
///
/// The editors are opaque ([`field`]), so the text people actually *read* is
/// never affected by this value. Only the small chrome text is.
///
/// Platform note: if the OS cannot supply the backdrop at all, this tint is the
/// only thing carrying legibility, so a value tuned for a blurred desktop can be
/// too low on a platform without one.
const BACKDROP_ALPHA: (f32, f32) = (0.16, 0.20);

/// Opacity of the translucent window surface: white enough to read as a glass
/// sheet, open enough to show the desktop through it.
///
/// Bleed-through dilutes linearly with this number and nothing else dilutes
/// it: the compositor here cannot blur, so there is no frosted middle ground
/// between "exact colours show through" and "solid". At `0.45` a saturated
/// window behind read as sky blue (`0.45 white + 0.55 behind`); at `0.75` the
/// same behind-window is a faint tint on white. `PROMPTER_GLASS_ALPHA`
/// overrides it without a rebuild.
const TRANSLUCENT_ALPHA: f32 = 0.75;

/// Overrides [`BACKDROP_ALPHA`] with a single value for both modes.
///
/// Glass is judged against whatever happens to be behind the window, and that
/// differs per desktop, per wallpaper and per day — so the number that reads as
/// glass on one machine is not the number that reads as glass on another. This
/// makes it tunable without a rebuild:
///
/// ```text
/// PROMPTER_GLASS_ALPHA=0.25 cargo run
/// ```
///
/// Out of range or unparseable values fall back to the defaults above, so a
/// typo degrades to the shipped look rather than to an invisible window.
fn backdrop_alpha_override() -> Option<f32> {
    let raw = std::env::var("PROMPTER_GLASS_ALPHA").ok()?;
    let alpha: f32 = raw.trim().parse().ok()?;
    (0.0..=1.0).contains(&alpha).then_some(alpha)
}

/// Opacity of the window's hairline edge.
const EDGE_ALPHA: f32 = 1.0;

/// The floor for either editor pane. Below this a prompt stops being workable,
/// which is what stops a drag from collapsing a pane out of existence.
///
/// `ResizablePanel::size_range` speaks in pixels rather than rems, matching the
/// library's own `PANEL_MIN_SIZE`.
pub const EDITOR_MIN_HEIGHT: Pixels = px(96.);

/// Lowers the window surface's alpha so the platform backdrop shows through.
///
/// Called once, before any window opens: the theme keeps the background in two
/// places — a solid colour and a renderable background — plus a projection into
/// the base layer, and [`Theme::update`] is the only write path that keeps all
/// three in step.
///
/// # Lifetime
///
/// This edit holds only until something loads a theme file. `Theme::change` and
/// `Theme::apply_config` re-read the registered theme, which restores the
/// opaque background and leaves the window grey. Nothing in the component layer
/// calls them on its own — `Theme::sync_system_appearance` is the application's
/// to call — so today the tint survives for the process's lifetime. If the
/// application ever starts following the OS appearance, call this again from
/// the same place that switched the mode.
pub fn install(cx: &mut App) {
    // The solid light design is the default for an opaque window and ignores
    // the glass alpha: on an opaque window a translucent root would blend with
    // the renderer's clear colour, not with the desktop, and wash out.
    if backdrop() == WindowBackgroundAppearance::Opaque {
        Theme::update(cx, apply_solid_light);
        return;
    }

    // The translucent look: the desktop reads through the chrome (no blur —
    // the compositor on at least one machine accepts blur policies and renders
    // black, so blur is shelved), while every role keeps the solid light
    // palette. Buttons, editors and text stay fully opaque; only the window
    // surface itself turns down.
    if backdrop() == WindowBackgroundAppearance::Transparent {
        let override_alpha = backdrop_alpha_override();
        Theme::update(cx, |theme| {
            apply_solid_light(theme);
            theme.colors.background = theme
                .colors
                .background
                .opacity(override_alpha.unwrap_or(TRANSLUCENT_ALPHA));
        });
        return;
    }

    let override_alpha = backdrop_alpha_override();
    Theme::update(cx, |theme| {
        let alpha = override_alpha.unwrap_or_else(|| {
            if theme.is_dark() {
                BACKDROP_ALPHA.1
            } else {
                BACKDROP_ALPHA.0
            }
        });
        theme.colors.background = theme.colors.background.opacity(alpha);

        // The shipped default theme leaves `input` undefined, and the dark-mode
        // fallback for the Default button variant runs straight through it:
        // `input.mix(transparent, 0.3)`. An undefined `input` resolves to
        // transparent black, so that fallback is *fully transparent* and the
        // quiet buttons in this window come out with no fill at all — fine
        // against an opaque window, invisible against glass. The role belongs to
        // the token layer, so it is defined here.
        theme.colors.input = theme.colors.border;

        // With `input` present the fallback would be a 30% wash, which is still
        // too faint to read as a control over a translucent backdrop. Give the
        // variant a real opaque surface, and its two interaction states surfaces
        // too so hover and press stay coherent with it.
        //
        // `secondary` is the theme's own opaque surface role, one step away from
        // `background`, which is what keeps these reading as buttons rather than
        // as second-class outlines.
        theme.tokens.button = theme.colors.secondary.into();
        theme.tokens.button_hover = theme.colors.accent.into();
        theme.tokens.button_active = theme.colors.muted.into();
    });
}

/// An opaque sRGB colour as the theme's working type.
///
/// `rgb` returns `Rgba` and the token layer speaks `ThemeToken`, so every
/// palette stop passes through `Hsla` here instead of converting twice at each
/// call site.
fn solid(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// The solid light palette: white chrome, white wells, neutral-grey controls,
/// near-black text.
///
/// Every role the window renders is pinned here so the design does not depend
/// on which shipped theme happened to load. The numbers are Tailwind neutrals,
/// chosen so each step of the ladder is visibly distinct on white:
///
/// - `#ffffff` chrome and editor wells, `#e5e5e5` buttons;
/// - `#171717` text, `#737373` secondary text, `#e5e5e5` hairlines;
/// - one dark neutral (`#171717`) reserved for the primary commit, so it reads
///   as the commit without introducing a hue.
fn apply_solid_light(theme: &mut Theme) {
    theme.colors.background = solid(0xffffff);
    theme.colors.foreground = solid(0x171717);
    theme.colors.muted = solid(0xf5f5f5);
    theme.colors.muted_foreground = solid(0x737373);
    theme.colors.border = solid(0xe5e5e5);
    theme.colors.input = solid(0xffffff);
    theme.colors.secondary = solid(0xe5e5e5);
    theme.colors.secondary_foreground = solid(0x171717);
    theme.colors.primary = solid(0x171717);
    theme.colors.primary_foreground = solid(0xffffff);
    theme.colors.accent = solid(0x171717);
    theme.colors.accent_foreground = solid(0xffffff);
    theme.colors.popover = solid(0xffffff);

    // Raised controls: light neutral fills with dark text, hover a step
    // darker, press a further step — the press state must stay clearly
    // distinct from the white chrome around it.
    theme.tokens.button = solid(0xe5e5e5).into();
    theme.tokens.button_hover = solid(0xd4d4d4).into();
    theme.tokens.button_active = solid(0xa3a3a3).into();
    theme.tokens.button_foreground = solid(0x171717).into();

    // The secondary set backs the outline segmented control. Same ladder as
    // the default set so the two read as one family.
    theme.tokens.button_secondary = solid(0xe5e5e5).into();
    theme.tokens.button_secondary_hover = solid(0xd4d4d4).into();
    theme.tokens.button_secondary_active = solid(0xa3a3a3).into();
    theme.tokens.button_secondary_foreground = solid(0x171717).into();

    // The primary commit is the dark neutral: white text on near-black in all
    // three states, so it is unmistakably the commit and still hue-free.
    theme.tokens.button_primary = solid(0x171717).into();
    theme.tokens.button_primary_hover = solid(0x404040).into();
    theme.tokens.button_primary_active = solid(0x262626).into();
    theme.tokens.button_primary_foreground = solid(0xffffff).into();
}

/// The window's hairline edge.
///
/// The window already has a frame drawn by the platform's border layer; this one
/// belongs to the rounded card that sits inside it, at a different edge.
pub fn edge(cx: &App) -> Hsla {
    cx.theme().border.opacity(EDGE_ALPHA)
}

/// Which backdrop material the window asks the platform for.
///
/// The default is `Transparent`: the gradient-transparency material, which
/// composites the desktop through per-pixel alpha without asking the
/// compositor to blur. It is the one material observed working — blur
/// (acrylic, blur-behind, transient) is accepted by the call and renders
/// black, so `blurred` stays selectable but shelved. Every non-`Opaque` value
/// also makes the renderer clear to fully transparent, which is what lets the
/// material show through at all.
///
/// `mica` and `mica-alt` are the opaque system materials, selectable only as a
/// control: if switching to one visibly changes the window, the material path
/// is alive. Mica is opaque by definition and will never look see-through.
///
/// ```text
/// PROMPTER_BACKDROP=blurred cargo run
/// ```
pub fn backdrop() -> WindowBackgroundAppearance {
    match std::env::var("PROMPTER_BACKDROP").ok().as_deref() {
        Some("mica") => WindowBackgroundAppearance::MicaBackdrop,
        Some("mica-alt") => WindowBackgroundAppearance::MicaAltBackdrop,
        Some("opaque") => WindowBackgroundAppearance::Opaque,
        Some("blurred") => WindowBackgroundAppearance::Blurred,
        _ => WindowBackgroundAppearance::Transparent,
    }
}

/// The editor surfaces: white wells, fully opaque.
///
/// The wells are the same white as the chrome, so their edges come from the
/// Textarea's own border rather than from a fill step. Text never depends on
/// anything behind the window, because there is nothing behind it.
pub fn field(_cx: &App) -> Hsla {
    solid(0xffffff)
}
