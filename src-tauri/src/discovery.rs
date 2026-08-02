use crate::models::DiscoveredPrinter;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};

const SERVICE_TYPE: &str = "_snapmaker._tcp.local.";

pub fn discover(timeout: Duration) -> anyhow::Result<Vec<DiscoveredPrinter>> {
    let daemon = ServiceDaemon::new()?;
    let receiver = daemon.browse(SERVICE_TYPE)?;
    let started = Instant::now();
    let mut printers = HashMap::<String, DiscoveredPrinter>::new();

    while started.elapsed() < timeout {
        let remaining = timeout.saturating_sub(started.elapsed());
        let wait = remaining.min(Duration::from_millis(350));
        match receiver.recv_timeout(wait) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let addresses = info
                    .get_addresses()
                    .iter()
                    .map(|address| address.to_ip_addr())
                    .collect();
                let Some(address) = preferred_address(&addresses) else {
                    continue;
                };
                let sn = info
                    .get_property_val_str("sn")
                    .unwrap_or_default()
                    .trim()
                    .to_uppercase();
                let id = if sn.is_empty() {
                    info.get_fullname().to_string()
                } else {
                    sn.clone()
                };
                let device_name = info
                    .get_property_val_str("device_name")
                    .unwrap_or_default()
                    .to_string();
                let machine_type = info
                    .get_property_val_str("machine_type")
                    .unwrap_or("Snapmaker U1")
                    .to_string();
                let display_name = if device_name.is_empty() {
                    if sn.is_empty() {
                        "Snapmaker U1".to_string()
                    } else {
                        format!("Snapmaker U1 · {sn}")
                    }
                } else {
                    device_name.clone()
                };

                printers.insert(
                    id.clone(),
                    DiscoveredPrinter {
                        id,
                        name: display_name,
                        host: address.to_string(),
                        port: info.get_port(),
                        sn,
                        machine_type,
                        device_name,
                        link_mode: info
                            .get_property_val_str("link_mode")
                            .unwrap_or_default()
                            .to_string(),
                        region: info
                            .get_property_val_str("region")
                            .unwrap_or_default()
                            .to_string(),
                    },
                );
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }

    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();

    let mut result: Vec<_> = printers.into_values().collect();
    result.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(result)
}

fn preferred_address(addresses: &std::collections::HashSet<IpAddr>) -> Option<IpAddr> {
    addresses
        .iter()
        .copied()
        .find(IpAddr::is_ipv4)
        .or_else(|| addresses.iter().copied().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_is_preferred_for_lan_connections() {
        let addresses = ["fe80::1".parse().unwrap(), "192.168.1.42".parse().unwrap()]
            .into_iter()
            .collect();
        assert_eq!(
            preferred_address(&addresses),
            Some("192.168.1.42".parse().unwrap())
        );
    }

    #[test]
    #[ignore = "requires a powered-on Snapmaker U1 on the same local network"]
    fn discovers_a_real_snapmaker_u1() {
        let printers = discover(Duration::from_secs(15)).unwrap();
        assert!(
            printers
                .iter()
                .any(|printer| printer.machine_type.contains("Snapmaker U1")),
            "no Snapmaker U1 responded to mDNS"
        );
    }
}
