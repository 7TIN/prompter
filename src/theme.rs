//! The product's theme layer.
//!
//! Prompter is a glass window, the way a native Windows acrylic window is: the
//! *chrome* — the window surface itself — is translucent, so the desktop the
//! platform blurs behind it reads through, and the *content* surfaces sit on top
//! of it. That split is the whole design, and it is why the alphas live here
//! rather than at the call sites.
//!
//! How the window actually becomes transparent, on every platform:
//!
//! - `WindowBackgroundAppearance::Blurred` asks the OS for the blur (acrylic on
//!   Windows 10 1903+, Mica elsewhere). That alone does nothing — the window
//!   still paints its own background.
//! - The component layer's window root paints `theme.tokens.background`, which
//!   is an opaque palette colour. So the background token's alpha is what has to
//!   come down, and that is what [`install`] does. Because `Theme::update` is
//!   the write path that keeps the solid colour, the renderable background and
//!   the base-layer projection in step, one edit moves all three.
//!
//! What is deliberately *not* translucent: buttons, icons, and the editors. The
//! window is glass; the things you read and click are not, because a control
//! you cannot see the edge of is not a control.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::theme::Theme;
use gpui_kit::{
    App, Background, Hsla, Pixels, WindowBackgroundAppearance, linear_color_stop, linear_gradient,
    px,
};

/// Tint opacity of the window surface, as `(light, dark)`.
///
/// This is the whole glass effect. The window is composited by the platform
/// (acrylic on Windows) and the application paints one translucent layer over
/// it, so this number decides how much of the desktop reads through.
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
const BACKDROP_ALPHA: (f32, f32) = (0.30, 0.26);

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
const EDGE_ALPHA: f32 = 0.6;

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

/// The window's hairline edge.
///
/// The window already has a frame drawn by the platform's border layer; this one
/// belongs to the rounded card that sits inside it, at a different edge.
pub fn edge(cx: &App) -> Hsla {
    cx.theme().border.opacity(EDGE_ALPHA)
}

/// The window's glass, built from raw GPUI rather than from a component.
///
/// The component layer can only give the window one flat translucent rectangle:
/// `WindowState::style` sets a single background colour on the root surface, and
/// a flat tint is exactly what reads as grey paint. A real acrylic surface has
/// *depth* — the part nearest the light is lighter, and the tint falls away
/// toward the bottom — and that needs a second layer the component layer has no
/// concept of.
///
/// So the window is composed here instead: the root keeps its translucent tint,
/// and this gradient is painted over it, spanning the title bar as well, so the
/// light falls across the whole window the way it does on a native one.
///
/// `angle` is 180°, top to bottom. The first stop sits at the top edge.
pub fn glass_depth(cx: &App) -> Background {
    let surface = cx.theme().popover;
    linear_gradient(
        180.,
        linear_color_stop(surface.opacity(DEPTH_ALPHA), 0.),
        linear_color_stop(cx.theme().transparent, 1.),
    )
}

/// How strongly the glass lightens toward the top of the window.
///
/// Deliberately tiny, and deliberately *additive* to [`BACKDROP_ALPHA`] rather
/// than part of it: two translucent layers compound, so `0.30` and `0.12` cover
/// about `0.38` of the backdrop at the top and `0.30` at the bottom. That budget
/// is the whole look. Raising either number without lowering the other is what
/// turned the window into flat grey paint.
const DEPTH_ALPHA: f32 = 0.12;

/// Which backdrop material the window asks the platform for.
///
/// This maps directly onto Microsoft's two system backdrops:
///
/// - `Blurred` is **Desktop Acrylic**: "a semi-transparent material that
///   replicates the effect of frosted glass". It is the only one that lets the
///   desktop read through, so it is the default and the right choice here.
/// - `Mica` is the opposite — Microsoft describes it as "an *opaque* material
///   that incorporates the user's theme and desktop wallpaper", sampled from the
///   wallpaper once for performance. It will never look see-through, whatever
///   alpha this application paints. It is selectable only as a control: if
///   switching to it visibly changes the window, the material path is alive.
///
/// Microsoft's own guidance is that Acrylic belongs on transient, light-dismiss
/// surfaces and that a persistent app window should use Mica. That guidance is
/// deliberately not followed here: it is a trade of legibility for convention,
/// and this window's whole purpose is to show the desktop behind it.
///
/// ```text
/// PROMPTER_BACKDROP=mica cargo run
/// ```
pub fn backdrop() -> WindowBackgroundAppearance {
    match std::env::var("PROMPTER_BACKDROP").ok().as_deref() {
        Some("mica") => WindowBackgroundAppearance::MicaBackdrop,
        Some("mica-alt") => WindowBackgroundAppearance::MicaAltBackdrop,
        Some("transparent") => WindowBackgroundAppearance::Transparent,
        Some("opaque") => WindowBackgroundAppearance::Opaque,
        // Acrylic. Passed to Windows as `ACCENT_ENABLE_ACRYLIC_BLIND` with a
        // zeroed gradient, which means the OS contributes no tint of its own and
        // the translucency is entirely this application's to choose.
        _ => WindowBackgroundAppearance::Blurred,
    }
}

/// The editor surfaces: fully opaque, in the window background's own colour.
///
/// Text has to be readable over whatever happens to be behind the window, and an
/// editor is where the reading happens. This restores the alpha that
/// [`install`] removed from the background token — it is the same colour, not a
/// second one — so the editors look exactly as they do in an opaque theme while
/// the chrome around them stays glass.
pub fn field(cx: &App) -> Hsla {
    let background = cx.theme().background;
    Hsla {
        a: 1.0,
        ..background
    }
}
