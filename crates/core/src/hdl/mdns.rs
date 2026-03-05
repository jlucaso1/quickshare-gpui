use std::sync::{Arc, Mutex};
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceInfo};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::Receiver;
use tokio::sync::watch;
use tokio::time::{interval_at, Instant};
use tokio_util::sync::CancellationToken;

use crate::utils::{gen_mdns_endpoint_info, gen_mdns_name, local_ipv4_addrs, DeviceType};

const INNER_NAME: &str = "MDnsServer";
const TICK_INTERVAL: Duration = Duration::from_secs(60);
// Periodic re-announcement interval to help Android discover the service.
// Android may not always catch the initial mDNS announcement, so we resend
// periodically while visible.
const REANNOUNCE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]

pub enum Visibility {
    Visible = 0,
    Invisible = 1,
    Temporarily = 2,
}

#[allow(dead_code)]
impl Visibility {
    pub fn from_raw_value(value: u64) -> Self {
        match value {
            0 => Visibility::Visible,
            1 => Visibility::Invisible,
            2 => Visibility::Temporarily,
            _ => unreachable!(),
        }
    }
}

pub struct MDnsServer {
    daemon: ServiceDaemon,
    service_info: ServiceInfo,
    ble_receiver: Receiver<()>,
    visibility_sender: Arc<Mutex<watch::Sender<Visibility>>>,
    visibility_receiver: watch::Receiver<Visibility>,
}

impl MDnsServer {
    pub fn new(
        daemon: ServiceDaemon,
        endpoint_id: [u8; 4],
        service_port: u16,
        ble_receiver: Receiver<()>,
        visibility_sender: Arc<Mutex<watch::Sender<Visibility>>>,
        visibility_receiver: watch::Receiver<Visibility>,
    ) -> Result<Self, anyhow::Error> {
        let service_info = Self::build_service(endpoint_id, service_port, DeviceType::Laptop)?;

        Ok(Self {
            daemon,
            service_info,
            ble_receiver,
            visibility_sender,
            visibility_receiver,
        })
    }

    pub async fn run(&mut self, ctk: CancellationToken) -> Result<(), anyhow::Error> {
        info!("{INNER_NAME}: service starting");
        let monitor = self.daemon.monitor()?;
        let ble_receiver = &mut self.ble_receiver;
        let mut visibility = *self.visibility_receiver.borrow();
        let mut interval = interval_at(Instant::now() + TICK_INTERVAL, TICK_INTERVAL);
        let mut reannounce = interval_at(Instant::now() + REANNOUNCE_INTERVAL, REANNOUNCE_INTERVAL);

        // Register the service immediately if already visible
        if visibility == Visibility::Visible || visibility == Visibility::Temporarily {
            self.daemon.register(self.service_info.clone())?;
            info!("{INNER_NAME}: registered service (initial visibility: {visibility:?})");
        }

        loop {
            tokio::select! {
                _ = ctk.cancelled() => {
                    info!("{INNER_NAME}: tracker cancelled, breaking");
                    break;
                }
                r = monitor.recv_async() => {
                    match r {
                        Ok(_) => continue,
                        Err(err) => return Err(err.into()),
                    }
                },
                _ = self.visibility_receiver.changed() => {
                    visibility = *self.visibility_receiver.borrow_and_update();

                    debug!("{INNER_NAME}: visibility changed: {visibility:?}");
                    if visibility == Visibility::Visible {
                        self.daemon.register(self.service_info.clone())?;
                    } else if visibility == Visibility::Invisible {
                        let receiver = self.daemon.unregister(self.service_info.get_fullname())?;
                        let _ = receiver.recv();
                    } else if visibility == Visibility::Temporarily {
                        self.daemon.register(self.service_info.clone())?;
                        interval.reset();
                    }
                }
                _ = ble_receiver.recv() => {
                    if visibility == Visibility::Invisible {
                        continue;
                    }

                    // Re-announce so Android discovers us even if it missed
                    // the initial mDNS announcement.
                    debug!("{INNER_NAME}: ble_receiver: re-announcing service");
                    self.daemon.register(self.service_info.clone())?;
                },
                _ = reannounce.tick() => {
                    // Periodically re-announce while visible so Android's mDNS
                    // browser picks us up even if it missed the initial announcement.
                    if visibility == Visibility::Visible || visibility == Visibility::Temporarily {
                        trace!("{INNER_NAME}: periodic mDNS re-announcement");
                        self.daemon.register(self.service_info.clone())?;
                    }
                }
                _ = interval.tick() => {
                    if visibility != Visibility::Temporarily {
                        continue;
                    }

                    let receiver = self.daemon.unregister(self.service_info.get_fullname())?;
                    let _ = receiver.recv();
                    let _ = self.visibility_sender.lock().unwrap().send(Visibility::Invisible);
                }
            }
        }

        // Unregister the mDNS service - we're shutting down
        let receiver = self.daemon.unregister(self.service_info.get_fullname())?;
        if let Ok(event) = receiver.recv() {
            info!("MDnsServer: service unregistered: {:?}", &event);
        }

        Ok(())
    }

    fn build_service(
        endpoint_id: [u8; 4],
        service_port: u16,
        device_type: DeviceType,
    ) -> Result<ServiceInfo, anyhow::Error> {
        let name = gen_mdns_name(endpoint_id);

        let raw_hostname = gethostname::gethostname().to_string_lossy().into_owned();
        info!("Broadcasting with: {raw_hostname}");

        // The display name in the endpoint info uses the raw hostname
        let endpoint_info = gen_mdns_endpoint_info(device_type as u8, &raw_hostname);

        // mdns-sd v0.18 requires the hostname to end with ".local."
        let mdns_hostname = if raw_hostname.ends_with(".local.") {
            raw_hostname
        } else {
            format!("{raw_hostname}.local.")
        };

        // Explicitly provide IPv4 addresses instead of using enable_addr_auto(),
        // which includes IPv6 and breaks Nearby Share discovery on some devices.
        let ipv4_addrs = local_ipv4_addrs();
        let properties = [("n", endpoint_info)];
        let si = ServiceInfo::new(
            "_FC9F5ED42C8A._tcp.local.",
            &name,
            &mdns_hostname,
            ipv4_addrs.as_slice(),
            service_port,
            &properties[..],
        )?;

        Ok(si)
    }
}
