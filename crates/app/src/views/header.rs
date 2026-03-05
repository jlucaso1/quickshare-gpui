use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::IconName;

use crate::state::AppState;

pub struct HeaderView {
    state: Entity<AppState>,
}

impl HeaderView {
    pub fn new(state: Entity<AppState>) -> Self {
        Self { state }
    }
}

impl Render for HeaderView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let hostname = state.hostname.clone();
        let state_entity = self.state.clone();

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(gpui::rgb(0xe0e0e0))
            .bg(gpui::white())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gpui::rgb(0x1a1a1a))
                            .child("RQuickShare"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(0x666666))
                            .child(hostname),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(0x999999))
                            .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(
                        Button::new("settings")
                            .icon(IconName::Settings)
                            .ghost()
                            .on_click(move |_, _window: &mut Window, cx: &mut App| {
                                state_entity.update(cx, |s, cx| {
                                    s.show_settings = !s.show_settings;
                                    cx.notify();
                                });
                            }),
                    ),
            )
    }
}
