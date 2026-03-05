use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};

use crate::state::{AppState, ContentMode};
use crate::views::device_item::render_device_item;
use crate::views::transfer_item::render_transfer_item;

pub struct ContentView {
    state: Entity<AppState>,
}

impl ContentView {
    pub fn new(state: Entity<AppState>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { state }
    }
}

impl Render for ContentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state_ref = self.state.read(cx);
        let mode = state_ref.content_mode.clone();
        let devices: Vec<_> = state_ref.discovered_devices.values().cloned().collect();
        let transfers: Vec<_> = state_ref.transfers.values().cloned().collect();
        let _ = state_ref;

        div()
            .flex_1()
            .flex()
            .flex_col()
            .p_4()
            .overflow_hidden()
            .child(match mode {
                ContentMode::Idle => render_idle(self.state.clone()).into_any_element(),
                ContentMode::Discovery => {
                    render_discovery(devices, transfers, self.state.clone()).into_any_element()
                }
                ContentMode::Transfers => {
                    render_transfers(transfers, self.state.clone()).into_any_element()
                }
            })
    }
}

fn empty_placeholder(text: &str) -> Div {
    div()
        .py_8()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(gpui::rgb(0x999999))
        .child(text.to_string())
}

fn render_idle(state: Entity<AppState>) -> impl IntoElement {
    let state_clone = state.clone();

    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .child(
            div()
                .text_xl()
                .text_color(gpui::rgb(0x999999))
                .child("Select files to share"),
        )
        .child(
            Button::new("select-files")
                .label("Select Files")
                .primary()
                .on_click(move |_, _window: &mut Window, cx: &mut App| {
                    let state = state_clone.clone();
                    cx.spawn(async move |cx| {
                        let files = rfd::AsyncFileDialog::new()
                            .set_title("Select files to share")
                            .pick_files()
                            .await;

                        if let Some(files) = files {
                            let paths: Vec<_> =
                                files.into_iter().map(|f| f.path().to_path_buf()).collect();
                            let _ = cx.update(|cx| {
                                state.update(cx, |s, cx| s.select_files(paths, cx));
                            });
                        }
                    })
                    .detach();
                }),
        )
}

fn render_discovery(
    devices: Vec<rqs_lib::EndpointInfo>,
    transfers: Vec<crate::state::TransferItem>,
    entity: Entity<AppState>,
) -> impl IntoElement {
    let has_transfers = !transfers.is_empty();

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(0x333333))
                .child("Nearby Devices"),
        )
        .when(devices.is_empty(), |this: Div| {
            this.child(empty_placeholder("Searching for nearby devices..."))
        })
        .when(!devices.is_empty(), |this: Div| {
            this.child(
                div().flex().flex_col().gap_2().children(
                    devices
                        .into_iter()
                        .map(|d| render_device_item(d, entity.clone())),
                ),
            )
        })
        .when(has_transfers, |this: Div| {
            this.child(
                div()
                    .mt_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gpui::rgb(0x333333))
                            .child("Transfers"),
                    )
                    .children(
                        transfers
                            .into_iter()
                            .map(|t| render_transfer_item(t, entity.clone())),
                    ),
            )
        })
}

fn render_transfers(
    transfers: Vec<crate::state::TransferItem>,
    entity: Entity<AppState>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(0x333333))
                .child("Transfers"),
        )
        .when(transfers.is_empty(), |this: Div| {
            this.child(empty_placeholder("No active transfers"))
        })
        .when(!transfers.is_empty(), |this: Div| {
            this.child(
                div().flex().flex_col().gap_2().children(
                    transfers
                        .into_iter()
                        .map(|t| render_transfer_item(t, entity.clone())),
                ),
            )
        })
}
