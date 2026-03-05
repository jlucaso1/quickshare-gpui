use notify_rust::Notification;

pub fn notify_incoming_transfer(device_name: &str, file_names: &[String]) {
    let body = if file_names.is_empty() {
        format!("{device_name} wants to share with you")
    } else {
        let names = file_names.join(", ");
        format!("{device_name} wants to send: {names}")
    };

    if let Err(e) = Notification::new()
        .summary("RQuickShare")
        .body(&body)
        .icon("document-send")
        .show()
    {
        log::warn!("Failed to show notification: {e}");
    }
}

pub fn notify_transfer_complete(file_names: &[String], destination: Option<&str>) {
    let body = if let Some(dest) = destination {
        format!("Received {} file(s) to {dest}", file_names.len())
    } else {
        format!("Received {} file(s)", file_names.len())
    };

    if let Err(e) = Notification::new()
        .summary("RQuickShare")
        .body(&body)
        .icon("document-save")
        .show()
    {
        log::warn!("Failed to show notification: {e}");
    }
}
