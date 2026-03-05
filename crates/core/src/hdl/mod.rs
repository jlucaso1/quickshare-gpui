use std::collections::HashMap;
use std::time::Instant;

use p256::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};

use self::info::{InternalFileInfo, TransferMetadata};
use crate::securegcm::ukey2_client_init::CipherCommitment;
use crate::utils::RemoteDeviceInfo;

#[cfg(feature = "experimental")]
mod ble;
#[cfg(feature = "experimental")]
pub use ble::*;
#[cfg(all(feature = "experimental", target_os = "linux"))]
mod blea;
#[cfg(all(feature = "experimental", target_os = "linux"))]
pub use blea::*;
mod inbound;
pub use inbound::*;
pub(crate) mod info;
mod mdns_discovery;
pub use mdns_discovery::*;
mod mdns;
pub use mdns::*;
mod outbound;
pub use outbound::*;

#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]

pub enum State {
    #[default]
    Initial,
    ReceivedConnectionRequest,
    SentUkeyServerInit,
    SentUkeyClientInit,
    SentUkeyClientFinish,
    SentPairedKeyEncryption,
    ReceivedUkeyClientFinish,
    SentConnectionResponse,
    SentPairedKeyResult,
    SentIntroduction,
    ReceivedPairedKeyResult,
    WaitingForUserConsent,
    ReceivingFiles,
    SendingFiles,
    Disconnected,
    Rejected,
    Cancelled,
    Finished,
}

#[derive(Debug, Default)]
pub struct InnerState {
    pub id: String,
    pub server_seq: i32,
    pub client_seq: i32,
    pub encryption_done: bool,

    // Subject to be used-facing for progress, ...
    pub state: State,
    pub remote_device_info: Option<RemoteDeviceInfo>,
    pub pin_code: Option<String>,
    pub transfer_metadata: Option<TransferMetadata>,
    pub transferred_files: HashMap<i64, InternalFileInfo>,

    // Everything needed for encryption/decryption/verif
    pub cipher_commitment: Option<CipherCommitment>,
    pub private_key: Option<SecretKey>,
    pub public_key: Option<PublicKey>,
    pub server_init_data: Option<Vec<u8>>,
    pub client_init_msg_data: Option<Vec<u8>>,
    pub ukey_client_finish_msg_data: Option<Vec<u8>>,
    pub decrypt_key: Option<Vec<u8>>,
    pub recv_hmac_key: Option<Vec<u8>>,
    pub encrypt_key: Option<Vec<u8>>,
    pub send_hmac_key: Option<Vec<u8>>,

    // Used to handle/track ingress transfer
    pub text_payload: Option<TextPayloadInfo>,
    pub payload_buffers: HashMap<i64, Vec<u8>>,

    // Throttle progress notifications — only notify when bytes change by this threshold
    pub last_notified_bytes: u64,

    // When file transfer started (ReceivingFiles/SendingFiles)
    pub transfer_start: Option<Instant>,
}

#[derive(Debug, Clone)]
pub enum TextPayloadInfo {
    Url(i64),
    Text(i64),
    Wifi((i64, String)),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TextPayloadType {
    Url,
    Text,
    Wifi,
}

impl TextPayloadInfo {
    fn get_i64_value(&self) -> i64 {
        match self {
            TextPayloadInfo::Url(value)
            | TextPayloadInfo::Text(value)
            | TextPayloadInfo::Wifi((value, _)) => value.to_owned(),
        }
    }
}

pub fn format_bytes(bytes: f64) -> String {
    if bytes >= 1_000_000_000.0 {
        format!("{:.2} GB", bytes / 1_000_000_000.0)
    } else if bytes >= 1_000_000.0 {
        format!("{:.2} MB", bytes / 1_000_000.0)
    } else if bytes >= 1_000.0 {
        format!("{:.1} KB", bytes / 1_000.0)
    } else {
        format!("{bytes:.0} B")
    }
}

pub(crate) fn log_transfer_summary(state: &InnerState) {
    let total_bytes = state
        .transfer_metadata
        .as_ref()
        .map_or(0, |t| t.total_bytes);

    if let Some(start) = state.transfer_start {
        let elapsed = start.elapsed();
        let secs = elapsed.as_secs_f64();
        let avg_speed = if secs > 0.0 {
            total_bytes as f64 / secs
        } else {
            0.0
        };
        info!(
            "Transfer finished: {} in {:.1}s ({}/s avg)",
            format_bytes(total_bytes as f64),
            secs,
            format_bytes(avg_speed),
        );
    } else {
        info!("Transfer finished: {}", format_bytes(total_bytes as f64));
    }
}
