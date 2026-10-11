use gpui_kit::component::TitleBar;
use gpui_kit::{AppContext as _, Bounds, WindowBounds, WindowOptions};
use prompter::app::{self, PrompterView};

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
                // platform's backdrop material, and the app paints its own
                // translucent layers over it — see `theme::backdrop` and
                // `theme::glass_depth`.
                window_background: app::theme::backdrop(),
                // Base options come from the title bar, not from `Default`, so
                // the platform's own title bar is suppressed and the window draws
                // its own. `appears_transparent` is what removes the native
                // caption on Windows; without it the window keeps the system
                // header and the blur only ever shows inside the content area.
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title("Prompter");
                cx.new(|cx| PrompterView::new(window, cx))
            })
            .expect("failed to open window");
        });
}
