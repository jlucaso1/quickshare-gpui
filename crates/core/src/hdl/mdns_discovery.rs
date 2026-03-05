use std::collections::HashMap;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::utils::{is_not_self_ip, parse_mdns_endpoint_info};
use crate::DeviceType;

const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Default, Deserialize, Serialize)]

pub struct EndpointInfo {
    pub fullname: String,
    pub id: String,
    pub name: Option<String>,
    pub ip: Option<String>,
    pub port: Option<String>,
    pub rtype: Option<DeviceType>,
    pub present: Option<bool>,
}

pub struct MDnsDiscovery {
    daemon: ServiceDaemon,
    sender: broadcast::Sender<EndpointInfo>,
}

const SERVICE_TYPE: &str = "_FC9F5ED42C8A._tcp.local.";

impl MDnsDiscovery {
    pub fn new(sender: broadcast::Sender<EndpointInfo>) -> Result<Self, anyhow::Error> {
        let daemon = ServiceDaemon::new()?;
        Ok(Self { daemon, sender })
    }

    pub async fn run(self, ctk: CancellationToken) -> Result<(), anyhow::Error> {
        info!("MDnsDiscovery: service starting");

        let receiver = self.daemon.browse(SERVICE_TYPE)?;

        // Map with fullname as key and EndpointInfo as value
        let mut cache: HashMap<String, EndpointInfo> = HashMap::new();

        loop {
            tokio::select! {
                _ = ctk.cancelled() => {
                    info!("MDnsDiscovery: tracker cancelled, breaking");
                    break;
                }
                r = receiver.recv_async() => {
                    match r {
                        Ok(event) => {
                            match event {
                                ServiceEvent::ServiceResolved(info) => {
                                    let fullname = info.fullname.clone();
                                    let port = info.get_port();

                                    let ip = match info.get_addresses_v4().into_iter().next() {
                                        Some(ip) => ip,
                                        None => {
                                            debug!("MDnsDiscovery: {fullname} has no IPv4 addresses, skipping");
                                            continue;
                                        }
                                    };

                                    if !is_not_self_ip(&ip) {
                                        debug!("MDnsDiscovery: {fullname} is self IP ({ip}), skipping");
                                        continue;
                                    }

                                    let n = match info.get_property("n") {
                                        Some(prop) => prop,
                                        None => {
                                            debug!("MDnsDiscovery: {fullname} has no 'n' property, skipping");
                                            continue;
                                        }
                                    };

                                    let (dt, dn) = match parse_mdns_endpoint_info(n.val_str()) {
                                        Ok(r) => r,
                                        Err(e) => {
                                            debug!("MDnsDiscovery: {fullname} failed to parse endpoint info: {e}");
                                            continue;
                                        }
                                    };

                                    let ip_port = format!("{ip}:{port}");

                                    // Skip if already cached with same address (re-announcements)
                                    if cache.get(&fullname).is_some_and(|ei| ei.id == ip_port) {
                                        trace!("MDnsDiscovery: {fullname} already cached, skipping");
                                        continue;
                                    }

                                    debug!("MDnsDiscovery: checking reachability of {dn} at {ip_port}");
                                    match timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(&ip_port)).await {
                                        Ok(Ok(_)) => {
                                            let ei = EndpointInfo {
                                                fullname: fullname.clone(),
                                                id: ip_port,
                                                name: Some(dn),
                                                ip: Some(ip.to_string()),
                                                port: Some(port.to_string()),
                                                rtype: Some(dt),
                                                present: Some(true),
                                            };
                                            info!("ServiceResolved: Resolved a new service: {:?}", ei);
                                            cache.insert(fullname, ei.clone());
                                            let _ = self.sender.send(ei);
                                        }
                                        Ok(Err(e)) => {
                                            debug!("MDnsDiscovery: TCP connect to {ip_port} failed: {e}");
                                        }
                                        Err(_) => {
                                            debug!("MDnsDiscovery: TCP connect to {ip_port} timed out after {}s", TCP_CONNECT_TIMEOUT.as_secs());
                                        }
                                    }
                                }
                                ServiceEvent::ServiceRemoved(_, fullname) => {
                                    if let Some(ei) = cache.remove(&fullname) {
                                        info!("ServiceRemoved: {fullname}");
                                        let _ = self.sender.send(EndpointInfo {
                                            id: ei.id,
                                            ..Default::default()
                                        });
                                    }
                                }
                                ServiceEvent::ServiceFound(st, fullname) => {
                                    debug!("MDnsDiscovery: ServiceFound: {st} {fullname}");
                                }
                                ServiceEvent::SearchStarted(st) => {
                                    debug!("MDnsDiscovery: search started for {st}");
                                }
                                ServiceEvent::SearchStopped(st) => {
                                    debug!("MDnsDiscovery: search stopped for {st}");
                                }
                                _ => {
                                    trace!("MDnsDiscovery: unhandled event");
                                }
                            }
                        },
                        Err(err) => error!("MDnsDiscovery: error: {}", err),
                    }
                }
            }
        }

        // Stop browsing while the receiver is still alive, so the daemon
        // processes the stop command before the channel closes. This prevents
        // "Failed to send SearchStarted: sending on a closed channel" errors.
        let _ = self.daemon.stop_browse(SERVICE_TYPE);

        // Drain until we get SearchStopped, confirming the daemon has fully
        // stopped the browse operation before we drop the receiver channel.
        loop {
            match receiver.recv_async().await {
                Ok(ServiceEvent::SearchStopped(_)) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }

        Ok(())
    }
}
