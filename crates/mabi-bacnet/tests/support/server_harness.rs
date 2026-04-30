use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::timeout;

use mabi_bacnet::prelude::{BACnetServer, ServerEvent};

pub struct BacnetServerHarness {
    server: Arc<BACnetServer>,
    handle: JoinHandle<mabi_bacnet::BacnetResult<()>>,
    addr: std::net::SocketAddr,
}

impl BacnetServerHarness {
    pub async fn start(server: BACnetServer) -> Self {
        let server = Arc::new(server);
        let mut events = server.subscribe();
        let run_server = Arc::clone(&server);
        let handle = tokio::spawn(async move { run_server.run().await });

        let addr = loop {
            match timeout(Duration::from_secs(2), events.recv()).await {
                Ok(Ok(ServerEvent::Started { address })) => break address,
                Ok(Ok(_)) => continue,
                Ok(Err(error)) => panic!("failed to receive server start event: {error}"),
                Err(_) => panic!("timed out waiting for BACnet server start"),
            }
        };

        Self {
            server,
            handle,
            addr,
        }
    }

    pub fn addr(&self) -> std::net::SocketAddr {
        self.addr
    }

    pub fn server(&self) -> &Arc<BACnetServer> {
        &self.server
    }

    pub async fn shutdown(self) {
        self.server.shutdown();
        match timeout(Duration::from_secs(3), self.handle).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => panic!("BACnet server returned error on shutdown: {error}"),
            Ok(Err(error)) => panic!("BACnet server join failed: {error}"),
            Err(_) => panic!("timed out waiting for BACnet server shutdown"),
        }
    }
}
