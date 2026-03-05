use std::sync::Arc;
use std::time::Duration;

use bluer::adv::{Advertisement, AdvertisementHandle};
use bluer::UuidExt;
use rand::RngExt;
use tokio::sync::watch;
use tokio::time::{interval_at, Instant};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::hdl::mdns::Visibility;
use crate::utils::DeviceType;

// FastInitiation BLE advertisement header (protocol spec):
// [0xFC, 0x12, 0x8E] = model ID
// [version(3b)|type(3b)|uwb(1b)|cert(1b)] [adjusted_tx_power]
// [uwb_metadata(1)] [uwb_address(8)] [salt(1)] [secret_id_hash(8)]
// [flags(1)]
//
// The last 10 bytes (salt + secret_id_hash + flags) MUST be random per the
// protocol spec. Using static bytes causes Android to cache/ignore the
// advertisement.
const FAST_INIT_HEADER: [u8; 14] = [
    0xFC, 0x12, 0x8E, // model ID
    0x01, // version=0, type=0 (kNotify), uwb=0, cert=1
    0x42, // adjusted tx power
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // uwb_metadata + uwb_address
];

// NearbyConnections BLE endpoint advertisement:
// Version V1 (1 << 5) | PCP P2P_POINT_TO_POINT (2) = 0x22
// Format: [VERSION_PCP(1)][ENDPOINT_ID(4)][ENDPOINT_INFO_SIZE(1)][ENDPOINT_INFO(≤17)]
// Android sender scans for this on UUID 0xFEF3 (Copresence) to discover nearby receivers.
const VERSION_PCP: u8 = (1 << 5) | 2; // 0x22

const INNER_NAME: &str = "BleAdvertiser";

// Rotate the BLE advertisement periodically with fresh random bytes.
// This prevents Android from caching/ignoring a stale advertisement.
const ROTATE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct BleAdvertiser {
    adapter: Arc<bluer::Adapter>,
    endpoint_id: [u8; 4],
    visibility_receiver: watch::Receiver<Visibility>,
}

impl BleAdvertiser {
    pub async fn new(
        endpoint_id: [u8; 4],
        visibility_receiver: watch::Receiver<Visibility>,
    ) -> Result<Self, anyhow::Error> {
        let session = bluer::Session::new().await?;
        let adapter = session.default_adapter().await?;
        adapter.set_powered(true).await?;

        Ok(Self {
            adapter: Arc::new(adapter),
            endpoint_id,
            visibility_receiver,
        })
    }

    pub async fn run(mut self, ctk: CancellationToken) -> Result<(), anyhow::Error> {
        info!(
            "{INNER_NAME}: advertising on Bluetooth adapter {} with address {}",
            self.adapter.name(),
            self.adapter.address().await?
        );

        let fast_init_uuid = Uuid::from_u16(0xFE2C);
        let copresence_uuid = Uuid::from_u16(0xFEF3);
        let mut fast_init_handle: Option<AdvertisementHandle> = None;
        let mut nearby_conn_handle: Option<AdvertisementHandle> = None;
        let mut rotate = interval_at(Instant::now() + ROTATE_INTERVAL, ROTATE_INTERVAL);

        // Start advertising if already visible
        let mut visibility = *self.visibility_receiver.borrow();
        if visibility == Visibility::Visible || visibility == Visibility::Temporarily {
            fast_init_handle = Some(
                self.adapter
                    .advertise(Self::build_fast_init_advertisement(fast_init_uuid))
                    .await?,
            );
            match self
                .adapter
                .advertise(Self::build_nearby_conn_advertisement(
                    copresence_uuid,
                    &self.endpoint_id,
                ))
                .await
            {
                Ok(h) => nearby_conn_handle = Some(h),
                Err(e) => warn!("{INNER_NAME}: couldn't start NearbyConnections BLE ad: {e}"),
            }
            info!("{INNER_NAME}: started advertising (initial visibility: {visibility:?})");
        }

        loop {
            tokio::select! {
                _ = ctk.cancelled() => {
                    info!("{INNER_NAME}: tracker cancelled, returning");
                    break;
                }
                _ = self.visibility_receiver.changed() => {
                    visibility = *self.visibility_receiver.borrow_and_update();
                    debug!("{INNER_NAME}: visibility changed: {visibility:?}");

                    match visibility {
                        Visibility::Visible | Visibility::Temporarily => {
                            if fast_init_handle.is_none() {
                                fast_init_handle = Some(
                                    self.adapter
                                        .advertise(Self::build_fast_init_advertisement(fast_init_uuid))
                                        .await?,
                                );
                                info!("{INNER_NAME}: started FastInit advertising");
                            }
                            if nearby_conn_handle.is_none() {
                                match self.adapter.advertise(
                                    Self::build_nearby_conn_advertisement(copresence_uuid, &self.endpoint_id),
                                ).await {
                                    Ok(h) => {
                                        nearby_conn_handle = Some(h);
                                        info!("{INNER_NAME}: started NearbyConnections advertising");
                                    }
                                    Err(e) => warn!("{INNER_NAME}: couldn't start NearbyConnections BLE ad: {e}"),
                                }
                            }
                        }
                        Visibility::Invisible => {
                            if fast_init_handle.take().is_some() {
                                info!("{INNER_NAME}: stopped FastInit advertising");
                            }
                            if nearby_conn_handle.take().is_some() {
                                info!("{INNER_NAME}: stopped NearbyConnections advertising");
                            }
                        }
                    }
                }
                _ = rotate.tick() => {
                    // Rotate advertisements with fresh random bytes so Android
                    // doesn't cache/ignore them
                    if fast_init_handle.is_some() {
                        fast_init_handle.take();
                        fast_init_handle = Some(
                            self.adapter
                                .advertise(Self::build_fast_init_advertisement(fast_init_uuid))
                                .await?,
                        );
                    }
                    if nearby_conn_handle.is_some() {
                        nearby_conn_handle.take();
                        match self.adapter.advertise(
                            Self::build_nearby_conn_advertisement(copresence_uuid, &self.endpoint_id),
                        ).await {
                            Ok(h) => nearby_conn_handle = Some(h),
                            Err(e) => warn!("{INNER_NAME}: couldn't rotate NearbyConnections BLE ad: {e}"),
                        }
                    }
                    if fast_init_handle.is_some() || nearby_conn_handle.is_some() {
                        trace!("{INNER_NAME}: rotated advertisements with fresh random bytes");
                    }
                }
            }
        }

        drop(fast_init_handle);
        drop(nearby_conn_handle);
        Ok(())
    }

    fn build_fast_init_advertisement(service_uuid: Uuid) -> Advertisement {
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&FAST_INIT_HEADER);
        // Last 10 bytes: random per protocol spec
        let random_bytes: [u8; 10] = rand::rng().random();
        data.extend_from_slice(&random_bytes);

        Advertisement {
            advertisement_type: bluer::adv::Type::Broadcast,
            service_data: [(service_uuid, data)].into(),
            ..Default::default()
        }
    }

    /// Build a NearbyConnections BLE endpoint advertisement (fast variant).
    ///
    /// Format: [VERSION_PCP(1)][ENDPOINT_ID(4)][INFO_SIZE(1)][ENDPOINT_INFO(17)]
    /// Endpoint info (17 bytes): [device_type_bitfield(1)][16 random bytes]
    /// No device name — Android resolves it via mDNS using the endpoint_id.
    fn build_nearby_conn_advertisement(service_uuid: Uuid, endpoint_id: &[u8; 4]) -> Advertisement {
        let mut data = Vec::with_capacity(23);
        data.push(VERSION_PCP);
        data.extend_from_slice(endpoint_id);
        data.push(17); // endpoint_info size
        data.push((DeviceType::Laptop as u8) << 1); // bitfield: device type
        let random_bytes: [u8; 16] = rand::rng().random();
        data.extend_from_slice(&random_bytes);

        Advertisement {
            advertisement_type: bluer::adv::Type::Broadcast,
            service_data: [(service_uuid, data)].into(),
            ..Default::default()
        }
    }
}
