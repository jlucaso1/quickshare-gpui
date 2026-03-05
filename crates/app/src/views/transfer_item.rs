use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::progress::Progress;

use crate::state::{AppState, TransferItem};
use rqs_lib::{format_bytes, State};

pub fn render_transfer_item(transfer: TransferItem, state: Entity<AppState>) -> impl IntoElement {
    let id = SharedString::from(format!("transfer-{}", transfer.id));

    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_2()
        .px_3()
        .py_3()
        .rounded_lg()
        .border_1()
        .border_color(gpui::rgb(0xe0e0e0))
        .bg(gpui::white())
        .child(render_transfer_header(&transfer))
        .child(render_transfer_body(&transfer, state))
}

fn render_transfer_header(transfer: &TransferItem) -> impl IntoElement {
    let device_name = if transfer.device_name.is_empty() {
        "Unknown".to_string()
    } else {
        transfer.device_name.clone()
    };

    let status_text = match &transfer.state {
        State::WaitingForUserConsent => "Waiting for approval".to_string(),
        State::SendingFiles => "Sending...".to_string(),
        State::ReceivingFiles => "Receiving...".to_string(),
        State::Finished => "Completed".to_string(),
        State::Cancelled => "Cancelled".to_string(),
        State::Rejected => "Rejected".to_string(),
        State::Disconnected => "Disconnected".to_string(),
        _ => format!("{:?}", transfer.state),
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(gpui::rgb(0x333333))
                .child(device_name),
        )
        .child(
            div()
                .text_xs()
                .text_color(status_color(&transfer.state))
                .child(status_text),
        )
}

fn status_color(state: &State) -> Hsla {
    match state {
        State::Finished => gpui::rgb(0x4caf50).into(),
        State::Cancelled | State::Rejected | State::Disconnected => gpui::rgb(0xf44336).into(),
        State::WaitingForUserConsent => gpui::rgb(0xff9800).into(),
        _ => gpui::rgb(0x2196f3).into(),
    }
}

fn render_transfer_body(transfer: &TransferItem, state: Entity<AppState>) -> impl IntoElement {
    match &transfer.state {
        State::WaitingForUserConsent => render_consent(transfer, state).into_any_element(),
        State::SendingFiles | State::ReceivingFiles => {
            render_progress(transfer, state).into_any_element()
        }
        State::Finished => render_finished(transfer, state).into_any_element(),
        _ => render_terminal(transfer, state).into_any_element(),
    }
}

fn render_consent(transfer: &TransferItem, state: Entity<AppState>) -> impl IntoElement {
    let tid = transfer.id.clone();
    let tid2 = transfer.id.clone();
    let pin = transfer.pin_code.clone().unwrap_or_default();
    let has_pin = !pin.is_empty();
    let has_files = !transfer.files.is_empty();

    let state2 = state.clone();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .when(has_pin, |this: Div| {
            this.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(0x666666))
                            .child("PIN:"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(gpui::rgb(0x333333))
                            .child(pin),
                    ),
            )
        })
        .when(has_files, |this: Div| {
            this.child(files_label(&transfer.files))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(Button::new("accept").label("Accept").primary().on_click(
                    move |_, _window: &mut Window, cx: &mut App| {
                        state.update(cx, |s, _cx| s.accept_transfer(&tid));
                    },
                ))
                .child(Button::new("reject").label("Reject").on_click(
                    move |_, _window: &mut Window, cx: &mut App| {
                        state2.update(cx, |s, _cx| s.reject_transfer(&tid2));
                    },
                )),
        )
}

fn files_label(files: &[String]) -> Div {
    div()
        .text_xs()
        .text_color(gpui::rgb(0x666666))
        .text_ellipsis()
        .child(files.join(", "))
}

fn render_progress(transfer: &TransferItem, state: Entity<AppState>) -> impl IntoElement {
    let tid = transfer.id.clone();
    let progress = if transfer.total_bytes > 0 {
        (transfer.ack_bytes as f32 / transfer.total_bytes as f32) * 100.0
    } else {
        0.0
    };
    let has_files = !transfer.files.is_empty();
    let speed = transfer.speed;

    div()
        .flex()
        .flex_col()
        .gap_2()
        .when(has_files, |this: Div| {
            this.child(files_label(&transfer.files))
        })
        .child(Progress::new().value(progress))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(0x999999))
                        .child(if speed > 0.0 {
                            format!("{:.0}% — {}/s", progress, format_bytes(speed))
                        } else {
                            format!("{:.0}%", progress)
                        }),
                )
                .child(
                    Button::new("cancel-transfer")
                        .label("Cancel")
                        .compact()
                        .on_click(move |_, _window: &mut Window, cx: &mut App| {
                            state.update(cx, |s, _cx| s.cancel_transfer(&tid));
                        }),
                ),
        )
}

fn render_finished(transfer: &TransferItem, state: Entity<AppState>) -> impl IntoElement {
    let tid = transfer.id.clone();
    let has_files = !transfer.files.is_empty();
    let dest = transfer.destination.clone();
    let has_dest = dest.is_some();
    let text_payload = transfer.text_payload.clone();
    let has_text = text_payload.is_some();

    let state2 = state.clone();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .when(has_files, |this: Div| {
            this.child(files_label(&transfer.files))
        })
        .when(has_text, |this: Div| {
            let text = text_payload.unwrap_or_default();
            this.child(div().text_xs().text_color(gpui::rgb(0x333333)).child(text))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .when(has_dest, |this: Div| {
                    let dest = dest.unwrap_or_default();
                    this.child(Button::new("open-file").label("Open").compact().on_click(
                        move |_, _window: &mut Window, _cx: &mut App| {
                            if let Err(e) = open::that(&dest) {
                                log::error!("Failed to open: {e}");
                            }
                        },
                    ))
                })
                .child(Button::new("clear").label("Clear").compact().on_click(
                    move |_, _window: &mut Window, cx: &mut App| {
                        state2.update(cx, |s, cx| s.clear_transfer(&tid, cx));
                    },
                )),
        )
}

fn render_terminal(transfer: &TransferItem, state: Entity<AppState>) -> impl IntoElement {
    let tid = transfer.id.clone();

    div().child(
        Button::new("clear-terminal")
            .label("Clear")
            .compact()
            .on_click(move |_, _window: &mut Window, cx: &mut App| {
                state.update(cx, |s, cx| s.clear_transfer(&tid, cx));
            }),
    )
}
