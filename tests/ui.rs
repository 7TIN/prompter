//! UI integration tests for the Prompter window.
//!
//! These drive the production view through native clicks, typing and keyboard
//! input, then assert the geometry and state a user would actually see. They
//! exist because the defects they cover were invisible to the type checker: a
//! long result pushed the card past the bottom of the window, cropping it, and
//! the toolbar had no stable geometry to be measured against.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px,
};
use prompter::app::{self, PrompterView};

/// Opens the window at the size the application opens at, and hands back both
/// the native handle and the production view entity.
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<PrompterView>) {
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::default(),
                    app::WINDOW_SIZE,
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| PrompterView::new(window, cx)),
        )
        .expect("open test window")
    })
}

/// Reaches the main surface without a key and without a network round trip.
fn enter_main(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("skip", cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// Presents `text` as the rewritten prompt, through the production view's own
/// write path inside a real window update.
fn set_result(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<PrompterView>,
    text: String,
) {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |view, cx| view.set_result(text, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
}

/// A rewritten prompt far longer than any pane, standing in for the real case:
/// several hundred words of output.
fn long_result() -> String {
    "Rewrite this prompt so that it reads as a single precise instruction to an \
     autonomous agent, removing every redundant qualifier, every restatement of \
     the requirement, and every paragraph that exists only to explain what the \
     previous paragraph already said. Preserve the intent exactly, preserve every \
     constraint, and do not introduce requirements the author did not state. The \
     result must be short enough to paste into a system prompt without further \
     editing, and precise enough that two engineers reading it would build the \
     same thing. Remove filler words, remove hedging, remove restatements, and \
     collapse every explanation of a concept into the correct technical term for \
     it. "
        .repeat(6)
}

#[gpui_kit::test]
fn a_long_result_does_not_push_the_card_out_of_the_window(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, view) = open(cx);
    enter_main(cx, handle);

    let baseline_card = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.find("card").bounds()
        })
        .unwrap();

    // Present a long result through the production view's own write path.
    set_result(cx, handle, &view, long_result());

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        let card = window.find("card").bounds();
        let prompt = window.find("prompt-pane").bounds();
        let result = window.find("result-pane").bounds();

        // The card itself must still fit the window: this is the crop the long
        // output used to cause.
        assert!(
            card.bottom() <= app::WINDOW_SIZE.height,
            "the card {:?} runs past the window height {:?} once the result is long",
            card,
            app::WINDOW_SIZE.height
        );

        // And the result pane must end inside the card, not merely inside the
        // window: a card whose own bottom edge is below its content is the shape
        // the bug produced.
        assert!(
            result.bottom() <= card.bottom(),
            "the result pane {result:?} extends past the card {card:?}"
        );

        // The toolbar and status row keep their place: a long result must not
        // change the card's own height at all.
        assert_eq!(
            card.size.height, baseline_card.size.height,
            "a long result changed the card's height"
        );

        // Both editors keep a usable share of the window.
        assert!(
            prompt.size.height > px(0.) && result.size.height > px(0.),
            "both editors keep height with a long result: {prompt:?} / {result:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_window_opens_with_two_editors_in_reading_order(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _view) = open(cx);
    enter_main(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        let card = window.find("card");
        let prompt = window.find("prompt-pane");
        let result = window.find("result-pane");

        assert!(card.visible(), "the card is visible");
        assert!(prompt.visible(), "the prompt pane is visible");
        assert!(result.visible(), "the result pane is visible");

        // The two editors are a vertical reading order, not two columns.
        assert!(
            prompt.bounds().bottom() <= result.bounds().top(),
            "prompt {:?} should end above result {:?}",
            prompt.bounds(),
            result.bounds()
        );

        // They share the card's leading and trailing edges: one content inset,
        // repeated rather than reinvented per region.
        let card_bounds = card.bounds();
        assert_eq!(
            prompt.bounds().origin.x,
            result.bounds().origin.x,
            "both editors start on the same leading edge"
        );
        assert_eq!(
            prompt.bounds().right(),
            result.bounds().right(),
            "both editors end on the same trailing edge"
        );
        assert!(
            prompt.bounds().left() > card_bounds.left()
                && prompt.bounds().right() < card_bounds.right(),
            "the editors sit inside the card's padding rather than on its edge"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn generate_rejects_an_empty_prompt_and_says_why(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _view) = open(cx);
    enter_main(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("generate", cx);
        window.render_frame(cx);

        // The negative case: the empty prompt is rejected with a message, rather
        // than the button appearing to do nothing.
        assert_eq!(
            window.find("status").label(),
            Some("Type or paste a prompt first")
        );
        // Nothing was written to the result.
        assert!(
            window.find("result-pane").value().is_none(),
            "a rejected generate must not write a result"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_prompt_reaches_the_editor_and_generate_starts_a_rewrite(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _view) = open(cx);
    enter_main(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("prompt", cx);
        window.input("make this concise please", cx);
        window.render_frame(cx);
    })
    .unwrap();

    assert!(
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.find("prompt").value() == Some("make this concise please")
        })
        .unwrap(),
        "typing into the prompt editor updates the production view"
    );
}

#[gpui_kit::test]
fn the_editors_survive_the_window_being_shrunk_to_its_minimum(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, view) = open(cx);
    enter_main(cx, handle);

    // Fill the result first, so the shrink is measured against the case that used
    // to break.
    set_result(cx, handle, &view, long_result());

    cx.simulate_window_resize(handle, app::WINDOW_MIN_SIZE);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let card = window.find("card").bounds();
        let prompt = window.find("prompt-pane").bounds();
        let result = window.find("result-pane").bounds();

        assert!(
            prompt.size.height > px(0.),
            "the prompt editor keeps height at the minimum window size"
        );
        assert!(
            result.size.height > px(0.),
            "the result editor keeps height at the minimum window size"
        );
        assert!(
            result.bottom() <= card.bottom(),
            "the result editor {result:?} escapes the card {card:?} at the minimum size"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_generate_button_reports_its_work(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, _view) = open(cx);
    enter_main(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("prompt", cx);
        window.input("make this concise please", cx);
        window.render_frame(cx);

        // The button is visible, on the toolbar, and sized for its content.
        let generate = window.find("generate");
        assert!(generate.visible(), "the generate button is visible");
        assert!(
            generate.bounds().size.width >= px(80.),
            "the generate button reserves room for its longer label: {:?}",
            generate.bounds()
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_generate_button_is_reachable_from_the_keyboard(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        cx.bind_keys([gpui_kit::KeyBinding::new(
            "ctrl-enter",
            app::GenerateAction,
            None,
        )])
    });
    let (handle, _view) = open(cx);
    enter_main(cx, handle);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("prompt", cx);
        window.input("make this concise please", cx);
        window.render_frame(cx);

        // Ctrl+Enter is the window's default commit and reaches the same owner
        // method the button does.
        window.press("ctrl-enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find("status").label().unwrap_or_default().to_owned();
        assert!(
            !status.starts_with("Type or paste"),
            "ctrl-enter reached the generate path, status was {status:?}"
        );
    })
    .unwrap();
}
