use std::sync::atomic::AtomicBool;

use mdns_sd::{ServiceDaemon, ServiceInfo, Error, ServiceEvent, Receiver};
use tauri::async_runtime::Mutex;

pub const SERVICE_TYPE: &str = "_ktu_fileshare._tcp.local.";

pub struct MdnsDaemon {
    mdns: ServiceDaemon,
    registered_service: ServiceInfo,
    service_receiver: Option<Receiver<ServiceEvent>>,
    received_services: Mutex<Vec<ServiceInfo>>,
    is_browsing: bool
}

impl MdnsDaemon {
    pub fn new(mdns: ServiceDaemon, registered_service: ServiceInfo, service_collection: Mutex<Vec<ServiceInfo>>) -> Self {
        Self {
            mdns, registered_service, service_receiver: None, received_services: service_collection, is_browsing: false
        }
    }

    pub fn notify(&self) -> Result<(), Error> {
        self.mdns.register(self.registered_service.clone())
    }

    pub fn start_query(&mut self) -> Result<(), Error> {
        let receiver = self.mdns.browse(SERVICE_TYPE)?;

        self.service_receiver = Some(receiver);
        self.is_browsing = true;

        Ok(())
    }

    pub async fn loop_add_services(&self) {
        if let Some(receiver) = &self.service_receiver {
            while self.is_browsing {
                if let Ok(event) = receiver.recv_async().await {

                    match event {
                        ServiceEvent::ServiceResolved(service) => {
                            let mut services = self.received_services.lock().await;

                            services.push(service);
                        },
                        ServiceEvent::ServiceRemoved(service_type, fullname) => {
                            let mut services = self.received_services.lock().await;

                            services.retain(|serv| {
                                serv.get_fullname() != fullname
                            });
                        }
                        _ => ()
                    }

                }
            }
        }
    }
}