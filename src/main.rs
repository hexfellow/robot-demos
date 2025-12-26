use mdns_sd::{ServiceDaemon, ServiceEvent};

#[tokio::main]
async fn main() {
    // Create a daemon
    let mdns = ServiceDaemon::new().expect("Failed to create daemon");

    // Browse for a service type.
    let service_type = "_hexfellow._tcp.local.";
    let receiver = mdns.browse(service_type).expect("Failed to browse");
    while let Ok(event) = receiver.recv_async().await {
        #[allow(clippy::single_match)]
        match event {
            ServiceEvent::ServiceResolved(resolved) => {
                println!(
                    "Found {:?}: {:?}, {:?}",
                    resolved.get_hostname(),
                    resolved.get_addresses(),
                    resolved.txt_properties
                );
            }
            _ => {}
        }
    }
}
