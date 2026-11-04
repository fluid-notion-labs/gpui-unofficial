// Repro for pa-gpui's blank transcript: ListState starts empty,
// FollowMode::Tail set, then items splice in (0 → 2). Does anything paint?
use crate::{
    self as gpui, px, size, AppContext, Context, FollowMode, IntoElement, ListState, Point, Render,
    Styled, TestAppContext, VisualTestContext, Window, div, list,
};
use crate::element::{Element as _, ParentElement};

struct TestView {
    state: ListState,
}
impl Render for TestView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        list(self.state.clone(), |ix, _, _| {
            div()
                .h(px(50.))
                .w_full()
                .bg(gpui::black())
                .child(format!("item {ix}"))
                .into_any()
        })
        .w_full()
        .h_full()
    }
}

/// Painted quad count for the window's last drawn frame.
fn quad_count(cx: &mut VisualTestContext) -> usize {
    cx.update(|window, _| window.rendered_frame.scene.quads.len())
}

#[gpui::test]
fn splice_into_empty_follow_tail_paints(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();

    let state = ListState::new(0, crate::ListAlignment::Top, px(1200.));
    state.set_follow_mode(FollowMode::Tail);

    let view = cx.update(|_, cx| {
        cx.new(|_| TestView { state: state.clone() })
    });

    // Frame 1: empty list.
    cx.draw(
        Point::new(px(0.), px(0.)),
        size(px(100.), px(200.)),
        |_, _| view.clone().into_any_element(),
    );
    let quads_before = quad_count(cx);

    // Splice two items in (what TranscriptView does when messages arrive).
    state.splice(0..0, 2);
    cx.draw(
        Point::new(px(0.), px(0.)),
        size(px(100.), px(200.)),
        |_, _| view.clone().into_any_element(),
    );

    let quads_after = quad_count(cx);
    eprintln!("follow: quads before splice: {quads_before}, after: {quads_after}");
    assert!(
        quads_after > quads_before,
        "no quads painted after splicing into an empty tail-following list (before={quads_before}, after={quads_after})"
    );
}

/// pa-gpui's container chain: the list is a bare child of a definite-height
/// flex column, with no h_full/flex_1 of its own (as in TranscriptView).
struct ChainView(ListState);
impl Render for ChainView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .min_w_0()
                    .h_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .px(px(12.))
                    .py(px(8.))
                    .child(list(self.0.clone(), |ix, _, _| {
                        div()
                            .h(px(50.))
                            .w_full()
                            .bg(gpui::black())
                            .children([format!("item {ix}")])
                            .into_any()
                    })
                    .w_full()
                    .h_full()),
            )
    }
}

#[gpui::test]
fn bare_list_in_transcript_chain_paints(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();

    let state = ListState::new(0, crate::ListAlignment::Top, px(1200.));
    state.set_follow_mode(FollowMode::Tail);

    let view = cx.update(|_, cx| {
        cx.new(|_| ChainView(state.clone()))
    });

    cx.draw(
        Point::new(px(0.), px(0.)),
        size(px(1040.), px(800.)),
        |_, _| view.clone().into_any_element(),
    );
    let quads_before = quad_count(cx);

    state.splice(0..0, 2);
    cx.draw(
        Point::new(px(0.), px(0.)),
        size(px(1040.), px(800.)),
        |_, _| view.clone().into_any_element(),
    );

    let quads_after = quad_count(cx);
    eprintln!("chain: quads before splice: {quads_before}, after: {quads_after}");
    assert!(
        quads_after > quads_before,
        "no quads painted for a bare list inside pa-gpui's container chain (before={quads_before}, after={quads_after})"
    );
}

#[gpui::test]
fn splice_into_empty_no_follow_paints(cx: &mut TestAppContext) {
    // Control: same flow without FollowMode::Tail.
    let cx = cx.add_empty_window();

    let state = ListState::new(0, crate::ListAlignment::Top, px(1200.));

    let view = cx.update(|_, cx| {
        cx.new(|_| TestView { state: state.clone() })
    });

    cx.draw(
        Point::new(px(0.), px(0.)),
        size(px(100.), px(200.)),
        |_, _| view.clone().into_any_element(),
    );
    let quads_before = quad_count(cx);

    state.splice(0..0, 2);
    cx.draw(
        Point::new(px(0.), px(0.)),
        size(px(100.), px(200.)),
        |_, _| view.clone().into_any_element(),
    );

    let quads_after = quad_count(cx);
    eprintln!("no-follow: quads before splice: {quads_before}, after: {quads_after}");
    assert!(
        quads_after > quads_before,
        "no quads painted after splicing into an empty list without follow (before={quads_before}, after={quads_after})"
    );
}
