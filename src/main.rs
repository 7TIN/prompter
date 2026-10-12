use gpui_kit::component::TitleBar;
use gpui_kit::{AppContext as _, Bounds, WindowBounds, WindowOptions};
use prompter::app::{self, PrompterView};

/// The window title, used both as the window's title and as the lookup key
/// `acrylic::apply` needs, since gpui exposes no accessor for a window's HWND.
const WINDOW_TITLE: &str = "Prompter";

fn main() {
    let _hotkeys = app::spawn_hotkey_listener();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            app::prepare_app(cx);

            let bounds = Bounds::centered(None, app::WINDOW_SIZE, cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(app::WINDOW_MIN_SIZE),
                // Raw GPUI window configuration. `window_background` is the
                // platform's backdrop material, defaulting to opaque; the glass
                // variant behind `PROMPTER_BACKDROP=blurred` is shelved but
                // still selectable. The app paints its own surfaces over it —
                // see `theme`.
                window_background: app::theme::backdrop(),
                // Base options come from the title bar, not from `Default`, so
                // the platform's own title bar is suppressed and the window draws
                // its own. `appears_transparent` is what removes the native
                // caption on Windows; without it the window keeps the system
                // header above the content area.
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title(WINDOW_TITLE);
                cx.new(|cx| PrompterView::new(window, cx))
            })
            .expect("failed to open window");

            // Written after the window exists, so it overwrites the accent
            // policy gpui set at creation. That policy carries an opaque black
            // tint, which is why the built-in blur renders as flat grey; see
            // `acrylic` for the full account.
            prompter::acrylic::apply(WINDOW_TITLE);
        });
}
