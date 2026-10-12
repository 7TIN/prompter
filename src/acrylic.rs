//! Window backdrop, applied through the Win32 accent policy directly.
//!
//! # Why this file exists
//!
//! `gpui-kit` exposes exactly one Windows backdrop knob,
//! `WindowBackgroundAppearance`, and the mapping is:
//!
//! | Variant      | Accent state | Material                    |
//! |--------------|--------------|-----------------------------|
//! | `Opaque`     | —            | none                        |
//! | `Transparent`| 2            | transparent gradient        |
//! | `Blurred`    | 4            | Acrylic                     |
//! | `Mica*`      | —            | `DWMWA_SYSTEMBACKDROP_TYPE` |
//!
//! `Blurred` is the only variant that blurs, so it is the only one worth using
//! for glass. But it does not blur usefully. In `gpui-pre-windows/src/window.rs`:
//!
//! ```text
//! let mut color = color.unwrap_or_default();   // Blurred passes Some((0, 0, 0, 0))
//! let is_acrylic = state == 4;
//! if is_acrylic && color.3 == 0 {
//!     color.3 = 1;                              // alpha 0 -> 255
//! }
//! let accent = AccentPolicy {
//!     accent_state: state,
//!     accent_flags: if is_acrylic { 0 } else { 2 },
//!     gradient_color: ... | ((color.3 as u32) << 24),   // 0xFF000000
//!     animation_id: 0,
//! };
//! ```
//!
//! That `color.3 = 1` sends an **opaque black** gradient, so DWM blurs the
//! desktop and then paints solid black across the result. Windows' own acrylic
//! sample sends `ABGR(0, 0, 0, 0)` — alpha **0**, meaning "no tint" — which is a
//! different request. No alpha we paint over the top can undo an opaque layer
//! underneath, which is why tuning the application's own tint changed nothing.
//!
//! # What this module does
//!
//! It writes the accent policy itself, after the window exists, so it overwrites
//! what gpui wrote at creation. Two knobs are exposed as environment variables so
//! the material can be chosen without a rebuild:
//!
//! - `PROMPTER_ACCENT` — `blurbehind` (default), `acrylic`, `transparent`,
//!   `transient`, or `off`. Overrides `PROMPTER_BACKDROP` when set.
//!
//! `transient` is the escape hatch. It skips the legacy accent policy entirely
//! and uses the serviced Windows 11 backdrop API,
//! `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW)` —
//! the system's own acrylic-like material for transient windows, which travels
//! through DWM's current backdrop pipeline instead of the legacy one. The legacy
//! accent is explicitly disabled first so the two cannot stack.
//! - `PROMPTER_FLAGS` — the raw `accent_flags` value (decimal or `0x` hex).
//!   Defaults to `0`. Gpui itself sends `2` for every non-acrylic state, so when
//!   a state misbehaves this is the first thing to vary.
//! - `PROMPTER_TINT` — the tint layer as `0xRRGGBBAA`. Defaults to `0x00000000`,
//!   fully untinted, so the blurred backdrop is the only thing colouring the
//!   window.
//!
//! `blurbehind` is the default because `ACCENT_ENABLE_ACRYLIC_BLIND` is the state
//! that misbehaves here. `ACCENT_ENABLE_BLURBEHIND` is the long-standing Aero
//! blur and reliably produces visible blur with real transparency behind it.
//!
//! `PROMPTER_BACKDROP` still selects the *renderer* behaviour and, for the Mica
//! variants, the material gpui writes. Mica and Mica Alt are left entirely to
//! gpui: they are opaque by definition and there is nothing useful to add.
//!
//! # Why the window is found by title
//!
//! `PlatformWindow::platform_window_handle` is `pub(crate)` in gpui, and
//! `Window` exposes no accessor, so an application cannot reach its own HWND.
//! `FindWindowW` is the supported alternative for a single-window application.
//! Every failure path returns without touching the window, so a miss degrades to
//! whatever gpui already set rather than to a broken window.
//!
//! # The real fix belongs upstream
//!
//! Deleting `color.3 = 1` from `set_window_composition_attribute` in
//! `gpui-pre-windows/src/window.rs` would make `Blurred` behave as documented and
//! retire this module.

use gpui_kit::WindowBackgroundAppearance;

/// The Win32 accent states this application can request.
///
/// The numeric values are the `ACCENT_STATE` enum and are part of the ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    /// No blur, no tint. Requests nothing, so the window is simply drawn.
    Off,
    /// Blur behind the window, with no tint. The Aero-style blur.
    BlurBehind,
    /// Desktop Acrylic. Blurs and tints, but is the state that misbehaves here.
    Acrylic,
    /// A fully transparent gradient. Blurs nothing; see-through only.
    TransparentGradient,
    /// The serviced Windows 11 backdrop (`DWMSBT_TRANSIENTWINDOW`), applied
    /// through `DwmSetWindowAttribute` with the legacy accent disabled. The
    /// alternative when the legacy blur states composite as black.
    Transient,
}

impl Accent {
    /// The `ACCENT_STATE` value sent to Win32. Meaningless for
    /// [`Accent::Transient`], which does not use the accent policy.
    fn state(self) -> u32 {
        match self {
            Accent::Off => 0,
            Accent::BlurBehind => 3,
            Accent::Acrylic => 4,
            Accent::TransparentGradient => 2,
            Accent::Transient => 0,
        }
    }

    /// The name this accent is selected by in `PROMPTER_ACCENT`.
    fn name(self) -> &'static str {
        match self {
            Accent::Off => "off",
            Accent::BlurBehind => "blurbehind",
            Accent::Acrylic => "acrylic",
            Accent::TransparentGradient => "transparent",
            Accent::Transient => "transient",
        }
    }
}

/// Decides which accent to write, or `None` to leave the backdrop to gpui.
///
/// `PROMPTER_ACCENT` wins when set. Otherwise the choice follows
/// `PROMPTER_BACKDROP` so that the two variables cannot contradict each other, and
/// an unrecognised value resolves to the default rather than failing — a mistyped
/// variable should not stop the window from opening.
fn requested_accent() -> Option<Accent> {
    if let Ok(raw) = std::env::var("PROMPTER_ACCENT") {
        return Some(match raw.to_ascii_lowercase().as_str() {
            "off" | "none" => Accent::Off,
            "acrylic" | "blurred" => Accent::Acrylic,
            "transparent" => Accent::TransparentGradient,
            "transient" => Accent::Transient,
            _ => Accent::BlurBehind,
        });
    }

    Some(match crate::theme::backdrop() {
        // Mica goes through a different Win32 attribute entirely and is opaque by
        // definition, so there is nothing to add and nothing to override.
        WindowBackgroundAppearance::MicaBackdrop | WindowBackgroundAppearance::MicaAltBackdrop => {
            return None;
        }
        WindowBackgroundAppearance::Transparent => Accent::TransparentGradient,
        WindowBackgroundAppearance::Opaque => Accent::Off,
        WindowBackgroundAppearance::Blurred => Accent::BlurBehind,
    })
}

/// Applies the requested backdrop to the window titled `window_title`.
///
/// Call this *after* the window exists. Always safe: every failure path reports
/// itself and leaves the window as gpui configured it.
pub fn apply(window_title: &str) {
    let Some(accent) = requested_accent() else {
        println!("acrylic: Mica selected, leaving the backdrop to gpui");
        return;
    };

    if accent == Accent::Off {
        println!("acrylic: accent=off, leaving the backdrop to gpui");
        return;
    }

    let tint = tint_from_env();
    let flags = flags_from_env();
    match apply_accent_policy(window_title, accent, flags, tint) {
        Ok(()) => {
            let detail = if accent == Accent::Transient {
                "system backdrop transient".to_owned()
            } else {
                format!("state {}, flags {flags}", accent.state())
            };
            println!(
                "acrylic: applied {} ({detail}) with tint 0x{tint:08X}",
                accent.name()
            );
        }
        Err(reason) => println!("acrylic: not applied ({reason}); gpui's own backdrop stays"),
    }
}

/// Reads `PROMPTER_FLAGS` as a decimal or `0x`-prefixed hex integer.
///
/// Defaults to `0`: no border lines, no extra behaviour. Gpui sends `2` for
/// non-acrylic states, so `PROMPTER_FLAGS=2` reproduces gpui's own working
/// `transparent` call exactly, state for state.
fn flags_from_env() -> u32 {
    std::env::var("PROMPTER_FLAGS")
        .ok()
        .and_then(|raw| {
            let raw = raw.trim();
            let (digits, radix) = match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
                Some(hex) => (hex, 16),
                None => (raw, 10),
            };
            u32::from_str_radix(digits, radix).ok()
        })
        .unwrap_or(0)
}

/// Reads `PROMPTER_TINT` as a `0xRRGGBBAA` colour.
///
/// An absent or unparseable value means "no tint", which is what glass wants: the
/// blurred backdrop should be the only thing tinting the window.
fn tint_from_env() -> u32 {
    std::env::var("PROMPTER_TINT")
        .ok()
        .and_then(|raw| {
            let raw = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
            u32::from_str_radix(raw, 16).ok()
        })
        .unwrap_or(0)
}

/// Writes the accent policy, or explains which step could not be completed.
#[cfg(windows)]
fn apply_accent_policy(
    window_title: &str,
    accent: Accent,
    flags: u32,
    tint: u32,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows::Win32::Foundation::{HMODULE, HWND};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    use windows::core::{PCSTR, PCWSTR};

    // `FindWindowW` takes a NUL-terminated wide string.
    let title: Vec<u16> = std::ffi::OsStr::new(window_title)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: the class argument is null (documented as "any class"), and `title`
    // is NUL-terminated and outlives the call. `FindWindowW` reads both only for
    // the duration of the call.
    let hwnd = match unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) } {
        Ok(hwnd) => hwnd,
        Err(_) => return Err(format!("no window titled {window_title:?}")),
    };

    if accent == Accent::Transient {
        return apply_system_backdrop(hwnd);
    }

    // `SetWindowCompositionAttribute` is undocumented and so is not bound by the
    // `windows` crate; it is resolved the same way gpui resolves it.
    //
    // SAFETY: `user32.dll` is loaded into every process, so this cannot fail for
    // a desktop application.
    let user32 = match unsafe { GetModuleHandleW(PCWSTR(to_wide(USER32_DLL).as_ptr())) } {
        Ok(handle) => handle,
        Err(_) => return Err("user32.dll is not loaded".into()),
    };

    type SetWindowCompositionAttribute = unsafe extern "system" fn(HWND, *mut Data) -> i32;

    // SAFETY: `SetWindowCompositionAttribute` is exported by `user32.dll` with
    // this exact signature; transmuting the address is what gpui does too. The
    // resulting pointer is only ever called with a correctly laid out `Data`.
    let raw = unsafe {
        GetProcAddress(
            HMODULE(user32.0),
            PCSTR(SET_WINDOW_COMPOSITION_ATTRIBUTE.as_ptr()),
        )
    };
    let Some(raw) = raw else {
        return Err("user32.dll does not export SetWindowCompositionAttribute".into());
    };
    let set_window_composition_attribute: SetWindowCompositionAttribute =
        unsafe { std::mem::transmute(raw) };

    let mut policy = AccentPolicy {
        accent_state: accent.state(),
        // Gpui sends 2 here for non-acrylic states; 0 skips the OS-drawn border
        // lines since the window draws its own hairline. Tunable at runtime via
        // `PROMPTER_FLAGS` because this value measurably changes compositing.
        accent_flags: flags,
        gradient_color: tint,
        animation_id: 0,
    };
    let mut data = Data {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy,
        size_of_data: std::mem::size_of::<AccentPolicy>(),
    };

    // SAFETY: `data` is a correctly sized and initialised
    // `WINDOWCOMPOSITIONATTRIBDATA` whose inner pointer is valid and aligned, and
    // `cbData` is the size of the policy it points at.
    if unsafe { set_window_composition_attribute(hwnd, &mut data) } == 0 {
        return Err("SetWindowCompositionAttribute declined the policy".into());
    }
    Ok(())
}

/// Applies the serviced Windows 11 backdrop instead of the legacy accent.
///
/// Two steps, in order: the legacy accent is disabled first so gpui's
/// opaque-black acrylic cannot sit on top of the new material, and then
/// `DWMWA_SYSTEMBACKDROP_TYPE` selects `DWMSBT_TRANSIENTWINDOW` — the system's
/// acrylic-like material, which composites through DWM's current pipeline.
/// Requires Windows 11 22H2 or later; that gate is checked, not assumed.
#[cfg(windows)]
fn apply_system_backdrop(hwnd: windows::Win32::Foundation::HWND) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWM_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
        DwmSetWindowAttribute,
    };
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::core::{PCSTR, PCWSTR};

    // Presence check first: `DwmSetWindowAttribute` predates the backdrop
    // attribute by a decade, so resolving is not proof the attribute exists.
    // `RtlGetVersion` is what gpui itself gates on; build 22621 is where
    // `DWMWA_SYSTEMBACKDROP_TYPE` arrived.
    let build = windows_build_number().ok_or("could not read the Windows build number")?;
    if build < 22621 {
        return Err(format!(
            "system backdrops need build 22621+, this is {build}"
        ));
    }

    // Step one: neutralise the legacy accent. State 0 is `ACCENT_DISABLED` —
    // the window composites normally, with per-pixel alpha and nothing else.
    let user32 = unsafe { GetModuleHandleW(PCWSTR(to_wide(USER32_DLL).as_ptr())) }
        .map_err(|_| "user32.dll is not loaded".to_owned())?;
    type SetWindowCompositionAttribute = unsafe extern "system" fn(HWND, *mut Data) -> i32;
    let raw = unsafe {
        GetProcAddress(
            windows::Win32::Foundation::HMODULE(user32.0),
            PCSTR(SET_WINDOW_COMPOSITION_ATTRIBUTE.as_ptr()),
        )
    };
    let Some(raw) = raw else {
        return Err("user32.dll does not export SetWindowCompositionAttribute".into());
    };
    let set_window_composition_attribute: SetWindowCompositionAttribute =
        unsafe { std::mem::transmute(raw) };
    let mut policy = AccentPolicy {
        accent_state: Accent::Off.state(),
        accent_flags: 0,
        gradient_color: 0,
        animation_id: 0,
    };
    let mut data = Data {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy,
        size_of_data: std::mem::size_of::<AccentPolicy>(),
    };
    // SAFETY: as documented on the legacy path above.
    unsafe {
        set_window_composition_attribute(hwnd, &mut data);
    }

    // Step two: select the material. The value is passed by pointer with an
    // explicit byte count, exactly as the API contract requires.
    let backdrop: DWM_SYSTEMBACKDROP_TYPE = DWMSBT_TRANSIENTWINDOW;
    // SAFETY: `backdrop` is a valid `DWM_SYSTEMBACKDROP_TYPE`, it outlives the
    // call, and the byte count matches its size. The attribute constant selects
    // `DWMWA_SYSTEMBACKDROP_TYPE`, whose payload type is documented.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            std::ptr::addr_of!(backdrop).cast(),
            std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        )
    }
    .map_err(|_| "DwmSetWindowAttribute declined the system backdrop".to_owned())
}

/// Reads the Windows build number through `RtlGetVersion`, the same source gpui
/// gates its own Mica path on.
#[cfg(windows)]
fn windows_build_number() -> Option<u32> {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::Foundation::NTSTATUS;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        dwMajorVersion: 0,
        dwMinorVersion: 0,
        dwBuildNumber: 0,
        dwPlatformId: 0,
        szCSDVersion: [0; 128],
    };
    // SAFETY: `info` is the exact `OSVERSIONINFOW` layout the signature
    // requires, its size field is initialised, and it outlives the call.
    let status: NTSTATUS = unsafe { RtlGetVersion(&mut info) };
    (status.0 == 0).then_some(info.dwBuildNumber)
}

/// Off Windows there is no accent policy to write.
#[cfg(not(windows))]
fn apply_accent_policy(
    _window_title: &str,
    _accent: Accent,
    _flags: u32,
    _tint: u32,
) -> Result<(), String> {
    Err("not a Windows build".into())
}

#[cfg(windows)]
const USER32_DLL: &[u8] = b"user32.dll\0";

/// Widens an ASCII NUL-terminated literal into a `PCWSTR`.
///
/// Built at runtime rather than as a constant because `u16::from` is not a
/// const fn.
#[cfg(windows)]
fn to_wide(literal: &[u8]) -> Vec<u16> {
    literal.iter().map(|&byte| u16::from(byte)).collect()
}

#[cfg(windows)]
const SET_WINDOW_COMPOSITION_ATTRIBUTE: &[u8] = b"SetWindowCompositionAttribute\0";

/// `WCA_ACCENT_POLICY`, the attribute this module writes.
#[cfg(windows)]
const WCA_ACCENT_POLICY: u32 = 0x13;

/// `ACCENT_POLICY`, laid out exactly as Win32 expects.
///
/// Declared locally rather than imported: the `windows` crate does not bind this
/// undocumented type, and its layout is part of the ABI contract relied on here.
#[cfg(windows)]
#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

/// `WINDOWCOMPOSITIONATTRIBDATA`, local for the same reason.
///
/// `size_of_data` is a `SIZE_T` in the Win32 declaration; it is declared `usize`
/// here so the field lands at the offset the API reads it from on 64-bit.
#[cfg(windows)]
#[repr(C)]
struct Data {
    attribute: u32,
    data: *mut AccentPolicy,
    size_of_data: usize,
}
