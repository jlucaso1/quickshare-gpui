use gpui::*;
use gpui_component::button::Button;
use gpui_component::switch::Switch;

use crate::state::AppState;

pub fn render_settings_overlay<T: 'static>(
    state: Entity<AppState>,
    _window: &mut Window,
    cx: &mut Context<'_, T>,
) -> impl IntoElement {
    let s = state.read(cx);
    let autostart = s.settings.autostart;
    let realclose = s.settings.realclose;
    let startminimized = s.settings.startminimized;
    let download_path = s
        .settings
        .download_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "Default (~/Downloads)".to_string());

    let state_close = state.clone();
    let state_autostart = state.clone();
    let state_realclose = state.clone();
    let state_minimized = state.clone();
    let state_dlpath = state.clone();

    // Full-screen overlay with centered modal
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            // Backdrop
            div()
                .id("settings-backdrop")
                .absolute()
                .inset_0()
                .bg(gpui::rgba(0x00000066))
                .on_click(move |_, _window: &mut Window, cx: &mut App| {
                    state_close.update(cx, |s, cx| {
                        s.show_settings = false;
                        cx.notify();
                    });
                }),
        )
        .child(
            // Modal card
            div()
                .relative()
                .w(px(400.0))
                .bg(gpui::white())
                .rounded_xl()
                .shadow_lg()
                .flex()
                .flex_col()
                .overflow_hidden()
                // Header
                .child(
                    div()
                        .px_4()
                        .py_3()
                        .border_b_1()
                        .border_color(gpui::rgb(0xe0e0e0))
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Settings"),
                        ),
                )
                // Body
                .child(
                    div()
                        .px_4()
                        .py_4()
                        .flex()
                        .flex_col()
                        .gap_4()
                        // Autostart
                        .child(setting_row(
                            "autostart",
                            "Autostart",
                            "Start on system boot",
                            autostart,
                            move |val, _window, cx| {
                                state_autostart.update(cx, |s, cx| {
                                    s.settings.autostart = val;
                                    let _ = s.settings.save();
                                    cx.notify();
                                });
                            },
                        ))
                        // Keep running on close
                        .child(setting_row(
                            "realclose",
                            "Keep running on close",
                            "Minimize to background instead of quitting",
                            realclose,
                            move |val, _window, cx| {
                                state_realclose.update(cx, |s, cx| {
                                    s.settings.realclose = val;
                                    let _ = s.settings.save();
                                    cx.notify();
                                });
                            },
                        ))
                        // Start minimized
                        .child(setting_row(
                            "startmin",
                            "Start minimized",
                            "Hide window on launch",
                            startminimized,
                            move |val, _window, cx| {
                                state_minimized.update(cx, |s, cx| {
                                    s.settings.startminimized = val;
                                    let _ = s.settings.save();
                                    cx.notify();
                                });
                            },
                        ))
                        // Download path
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(setting_label("Download path", &download_path))
                                .child(
                                    Button::new("pick-download-path")
                                        .label("Change")
                                        .compact()
                                        .on_click(move |_, _window: &mut Window, cx: &mut App| {
                                            let state = state_dlpath.clone();
                                            cx.spawn(async move |cx| {
                                                let folder = rfd::AsyncFileDialog::new()
                                                    .set_title("Select download folder")
                                                    .pick_folder()
                                                    .await;
                                                if let Some(folder) = folder {
                                                    let path = folder.path().to_path_buf();
                                                    let _ = cx.update(|cx| {
                                                        state.update(cx, |s, cx| {
                                                            s.set_download_path(Some(path), cx);
                                                        });
                                                    });
                                                }
                                            })
                                            .detach();
                                        }),
                                ),
                        ),
                ),
        )
}

fn setting_label(label: &str, description: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(gpui::rgb(0x888888))
                .child(description.to_string()),
        )
}

fn setting_row(
    switch_id: &str,
    label: &str,
    description: &str,
    checked: bool,
    on_toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(setting_label(label, description))
        .child(
            Switch::new(SharedString::from(switch_id.to_string()))
                .checked(checked)
                .on_click(move |checked, window, cx| {
                    on_toggle(*checked, window, cx);
                }),
        )
}
