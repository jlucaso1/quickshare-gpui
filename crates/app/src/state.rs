use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gpui::{App, Context, Entity};
use rqs_lib::channel::{ChannelAction, ChannelDirection, ChannelMessage};
use rqs_lib::{DeviceType, EndpointInfo, OutboundPayload, SendInfo, State, Visibility, RQS};
use rqs_settings::Settings;
use tokio::sync::{broadcast, mpsc};

use crate::notification;

#[derive(Debug, Clone, PartialEq)]
pub enum ContentMode {
    Idle,
    Discovery,
    Transfers,
}

#[derive(Debug, Clone)]
pub struct TransferItem {
    pub id: String,
    pub direction: TransferDirection,
    pub state: State,
    pub device_name: String,
    pub device_type: DeviceType,
    pub pin_code: Option<String>,
    pub files: Vec<String>,
    pub destination: Option<String>,
    pub text_payload: Option<String>,
    pub total_bytes: u64,
    pub ack_bytes: u64,
    /// Transfer speed in bytes per second (rolling average over last window)
    pub speed: f64,
    /// When the transfer started (first ack_bytes update)
    pub transfer_start: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransferDirection {
    Inbound,
    Outbound,
}

pub struct AppState {
    pub settings: Settings,
    pub hostname: String,
    pub transfers: HashMap<String, TransferItem>,
    pub discovered_devices: HashMap<String, EndpointInfo>,
    pub files_to_send: Vec<PathBuf>,
    pub content_mode: ContentMode,
    pub show_settings: bool,
    pub sender_file: Option<mpsc::Sender<SendInfo>>,
    pub message_sender: Option<broadcast::Sender<ChannelMessage>>,
    pub dch_sender: broadcast::Sender<EndpointInfo>,
    pub rqs: Option<Arc<Mutex<RQS>>>,
}

impl AppState {
    pub fn new(settings: Settings, hostname: String) -> Self {
        let (dch_sender, _) = broadcast::channel(50);
        Self {
            settings,
            hostname,
            transfers: HashMap::new(),
            discovered_devices: HashMap::new(),
            files_to_send: Vec::new(),
            content_mode: ContentMode::Idle,
            show_settings: false,
            sender_file: None,
            message_sender: None,
            dch_sender,
            rqs: None,
        }
    }

    pub fn handle_channel_message(&mut self, msg: ChannelMessage, cx: &mut Context<Self>) {
        if msg.direction != ChannelDirection::LibToFront {
            return;
        }

        let id = msg.id.clone();
        let state = msg.state.clone().unwrap_or_default();

        let transfer = self.transfers.entry(id.clone()).or_insert_with(|| {
            let direction = match msg.rtype {
                Some(rqs_lib::channel::TransferType::Outbound) => TransferDirection::Outbound,
                _ => TransferDirection::Inbound,
            };
            TransferItem {
                id: id.clone(),
                direction,
                state: State::Initial,
                device_name: String::new(),
                device_type: DeviceType::Unknown,
                pin_code: None,
                files: Vec::new(),
                destination: None,
                text_payload: None,
                total_bytes: 0,
                ack_bytes: 0,
                speed: 0.0,
                transfer_start: None,
            }
        });

        transfer.state = state.clone();

        if let Some(meta) = &msg.meta {
            if let Some(src) = &meta.source {
                transfer.device_name = src.name.clone();
                transfer.device_type = src.device_type.clone();
            }
            if let Some(pin) = &meta.pin_code {
                transfer.pin_code = Some(pin.clone());
            }
            if let Some(files) = &meta.files {
                transfer.files = files.clone();
            }
            if let Some(dest) = &meta.destination {
                transfer.destination = Some(dest.clone());
            }
            if let Some(text) = &meta.text_payload {
                transfer.text_payload = Some(text.clone());
            }
            transfer.total_bytes = meta.total_bytes;
            if meta.ack_bytes != transfer.ack_bytes {
                let start = *transfer.transfer_start.get_or_insert_with(Instant::now);
                let elapsed = start.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    transfer.speed = meta.ack_bytes as f64 / elapsed;
                }
                transfer.ack_bytes = meta.ack_bytes;
            }
        }

        // Show notification for incoming consent requests
        if state == State::WaitingForUserConsent {
            let device = transfer.device_name.clone();
            let files = transfer.files.clone();
            notification::notify_incoming_transfer(&device, &files);
        }

        // Notify on completion
        if state == State::Finished && transfer.direction == TransferDirection::Inbound {
            let files = transfer.files.clone();
            let dest = transfer.destination.clone();
            notification::notify_transfer_complete(&files, dest.as_deref());
        }

        // Switch to transfers view if we have active transfers
        if !self.transfers.is_empty() {
            let has_active = self.transfers.values().any(|t| {
                !matches!(
                    t.state,
                    State::Finished | State::Cancelled | State::Rejected | State::Disconnected
                )
            });
            if has_active && self.content_mode != ContentMode::Discovery {
                self.content_mode = ContentMode::Transfers;
            }
        }

        cx.notify();
    }

    pub fn handle_endpoint_info(&mut self, info: EndpointInfo, cx: &mut Context<Self>) {
        // Core sends EndpointInfo with present=None and no name on ServiceRemoved,
        // so treat anything that isn't explicitly present=Some(true) with a name as a removal.
        if info.present == Some(true) && info.name.is_some() {
            self.discovered_devices.insert(info.id.clone(), info);
        } else {
            self.discovered_devices.remove(&info.id);
        }
        cx.notify();
    }

    pub fn accept_transfer(&self, id: &str) {
        self.send_action(id, ChannelAction::AcceptTransfer);
    }

    pub fn reject_transfer(&self, id: &str) {
        self.send_action(id, ChannelAction::RejectTransfer);
    }

    pub fn cancel_transfer(&self, id: &str) {
        self.send_action(id, ChannelAction::CancelTransfer);
    }

    fn send_action(&self, id: &str, action: ChannelAction) {
        if let Some(sender) = &self.message_sender {
            let msg = ChannelMessage {
                id: id.to_string(),
                direction: ChannelDirection::FrontToLib,
                action: Some(action),
                rtype: None,
                state: None,
                meta: None,
            };
            let _ = sender.send(msg);
        }
    }

    pub fn clear_transfer(&mut self, id: &str, cx: &mut Context<Self>) {
        self.transfers.remove(id);
        if self.transfers.is_empty() && self.content_mode == ContentMode::Transfers {
            self.content_mode = ContentMode::Idle;
        }
        cx.notify();
    }

    pub fn send_to_device(&mut self, device: &EndpointInfo, cx: &mut Context<Self>) {
        if self.files_to_send.is_empty() {
            return;
        }
        let Some(sender) = &self.sender_file else {
            return;
        };

        let addr = format!(
            "{}:{}",
            device.ip.as_deref().unwrap_or(""),
            device.port.as_deref().unwrap_or("")
        );

        let file_paths: Vec<String> = self
            .files_to_send
            .iter()
            .filter_map(|p| p.to_str().map(String::from))
            .collect();

        let info = SendInfo {
            id: device.id.clone(),
            name: device.name.clone().unwrap_or_default(),
            addr,
            ob: OutboundPayload::Files(file_paths),
        };

        let sender = sender.clone();
        tokio::spawn(async move {
            if let Err(e) = sender.send(info).await {
                log::error!("Failed to send files: {e}");
            }
        });

        // Clear files and stop discovery after initiating send
        self.files_to_send.clear();
        self.stop_discovery();
        self.content_mode = ContentMode::Transfers;
        cx.notify();
    }

    pub fn start_discovery(&mut self) {
        if let Some(rqs) = &self.rqs {
            let mut rqs = rqs.lock().unwrap();
            if let Err(e) = rqs.discovery(self.dch_sender.clone()) {
                log::error!("Failed to start discovery: {e}");
            }
        }
    }

    pub fn stop_discovery(&mut self) {
        if let Some(rqs) = &self.rqs {
            rqs.lock().unwrap().stop_discovery();
        }
        self.discovered_devices.clear();
    }

    pub fn change_visibility(&mut self, v: Visibility, cx: &mut Context<Self>) {
        if let Some(rqs) = &self.rqs {
            rqs.lock().unwrap().change_visibility(v);
        }
        self.settings.visibility = v as u8;
        if let Err(e) = self.settings.save() {
            log::error!("Failed to save settings: {e}");
        }
        cx.notify();
    }

    pub fn select_files(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.files_to_send = files;
        if !self.files_to_send.is_empty() {
            self.content_mode = ContentMode::Discovery;
            self.start_discovery();
        }
        cx.notify();
    }

    pub fn cancel_send(&mut self, cx: &mut Context<Self>) {
        self.files_to_send.clear();
        self.stop_discovery();
        self.content_mode = if self.transfers.is_empty() {
            ContentMode::Idle
        } else {
            ContentMode::Transfers
        };
        cx.notify();
    }

    pub fn set_download_path(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if let Some(rqs) = &self.rqs {
            rqs.lock().unwrap().set_download_path(path.clone());
        }
        self.settings.download_path = path;
        if let Err(e) = self.settings.save() {
            log::error!("Failed to save settings: {e}");
        }
        cx.notify();
    }

    pub fn visibility(&self) -> Visibility {
        Visibility::from_raw_value(self.settings.visibility as u64)
    }
}

fn spawn_broadcast_listener<T: Clone + Send + 'static>(
    state: Entity<AppState>,
    mut rx: broadcast::Receiver<T>,
    label: &'static str,
    handler: fn(&mut AppState, T, &mut Context<AppState>),
    cx: &App,
) {
    cx.spawn(async move |cx| loop {
        match rx.recv().await {
            Ok(msg) => {
                let _ = cx.update(|cx| {
                    state.update(cx, |s, cx| handler(s, msg, cx));
                });
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                log::warn!("{label} lagged by {n} messages");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    })
    .detach();
}

pub fn spawn_channel_listener(
    state: Entity<AppState>,
    rx: broadcast::Receiver<ChannelMessage>,
    cx: &App,
) {
    spawn_broadcast_listener(
        state,
        rx,
        "Channel listener",
        AppState::handle_channel_message,
        cx,
    );
}

pub fn spawn_discovery_listener(
    state: Entity<AppState>,
    rx: broadcast::Receiver<EndpointInfo>,
    cx: &App,
) {
    spawn_broadcast_listener(
        state,
        rx,
        "Discovery listener",
        AppState::handle_endpoint_info,
        cx,
    );
}
