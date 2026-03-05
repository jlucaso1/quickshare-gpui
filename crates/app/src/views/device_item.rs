use gpui::*;
use gpui_component::{Icon, IconName};

use crate::state::AppState;
use rqs_lib::{DeviceType, EndpointInfo};

fn device_icon(dt: &Option<DeviceType>) -> IconName {
    match dt {
        Some(DeviceType::Phone) => IconName::CircleUser,
        Some(DeviceType::Tablet) => IconName::LayoutDashboard,
        Some(DeviceType::Laptop) => IconName::SquareTerminal,
        _ => IconName::Info,
    }
}

pub fn render_device_item(device: EndpointInfo, state: Entity<AppState>) -> impl IntoElement {
    let name = device
        .name
        .clone()
        .unwrap_or_else(|| "Unknown Device".to_string());
    let icon = device_icon(&device.rtype);
    let id = SharedString::from(format!("device-{}", device.id));

    div()
        .id(id)
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(gpui::rgb(0xe0e0e0))
        .bg(gpui::white())
        .hover(|s| s.bg(gpui::rgb(0xf0f7ff)).border_color(gpui::rgb(0x2196f3)))
        .on_click(move |_, _window: &mut Window, cx: &mut App| {
            state.update(cx, |s, cx| s.send_to_device(&device, cx));
        })
        .child(Icon::new(icon).size_5().text_color(gpui::rgb(0x666666)))
        .child(
            div().flex().flex_col().child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(gpui::rgb(0x333333))
                    .child(name),
            ),
        )
}
