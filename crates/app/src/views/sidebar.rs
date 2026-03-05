use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::Button;

use crate::state::AppState;
use rqs_lib::Visibility;

pub struct SidebarView {
    state: Entity<AppState>,
}

impl SidebarView {
    pub fn new(state: Entity<AppState>) -> Self {
        Self { state }
    }
}

impl Render for SidebarView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let has_files = !state.files_to_send.is_empty();
        let current_visibility = state.visibility();
        let files: Vec<_> = state.files_to_send.clone();

        div()
            .w(px(220.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(gpui::rgb(0xe0e0e0))
            .bg(gpui::white())
            .p_3()
            .gap_3()
            .when(!has_files, |this: Div| {
                this.child(render_visibility_selector(
                    current_visibility,
                    self.state.clone(),
                ))
            })
            .when(has_files, |this: Div| {
                this.child(render_file_list(files, self.state.clone()))
            })
    }
}

fn render_visibility_selector(current: Visibility, state: Entity<AppState>) -> impl IntoElement {
    let options = [
        (
            Visibility::Visible,
            "Visible",
            "Everyone nearby can see you",
        ),
        (Visibility::Invisible, "Invisible", "No one can see you"),
        (
            Visibility::Temporarily,
            "Temporary",
            "Visible for a short time",
        ),
    ];

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(0x333333))
                .child("Visibility"),
        )
        .children(options.map(|(vis, label, desc)| {
            let is_selected = current == vis;
            let state = state.clone();

            div()
                .id(SharedString::from(format!("vis-{label}")))
                .cursor_pointer()
                .rounded_lg()
                .px_3()
                .py_2()
                .when(is_selected, |s| {
                    s.bg(gpui::rgb(0xe3f2fd))
                        .border_1()
                        .border_color(gpui::rgb(0x2196f3))
                })
                .when(!is_selected, |s| {
                    s.bg(gpui::rgb(0xf5f5f5))
                        .border_1()
                        .border_color(gpui::rgb(0xe0e0e0))
                        .hover(|s| s.bg(gpui::rgb(0xeeeeee)))
                })
                .on_click(move |_, _window: &mut Window, cx: &mut App| {
                    state.update(cx, |s, cx| s.change_visibility(vis, cx));
                })
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
                .child(div().text_xs().text_color(gpui::rgb(0x888888)).child(desc))
        }))
}

fn render_file_list(files: Vec<std::path::PathBuf>, state: Entity<AppState>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(0x333333))
                .child(format!("Files to send ({})", files.len())),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .overflow_hidden()
                .children(files.iter().map(|f| {
                    let name = f
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(0x555555))
                        .text_ellipsis()
                        .child(name)
                })),
        )
        .child(Button::new("cancel-send").label("Cancel").on_click(
            move |_, _window: &mut Window, cx: &mut App| {
                state.update(cx, |s, cx| s.cancel_send(cx));
            },
        ))
}
