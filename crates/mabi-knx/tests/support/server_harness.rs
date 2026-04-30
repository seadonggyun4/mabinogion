use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use mabi_knx::{GroupObjectTable, IndividualAddress, KnxServer, KnxServerConfig, ServerEvent};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::TestResult;

pub struct ServerHarness {
    pub server: Arc<KnxServer>,
    pub addr: SocketAddr,
    join: Option<JoinHandle<mabi_knx::KnxResult<()>>>,
}

impl ServerHarness {
    pub async fn start_with_table(
        config: KnxServerConfig,
        table: Arc<GroupObjectTable>,
    ) -> TestResult<Self> {
        Self::start_with_table_at(
            config,
            table,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        )
        .await
    }

    pub async fn start_with_table_at(
        mut config: KnxServerConfig,
        table: Arc<GroupObjectTable>,
        bind_addr: SocketAddr,
    ) -> TestResult<Self> {
        config.bind_addr = bind_addr;
        config.individual_address = IndividualAddress::new(1, 1, 1);
        config.device_name = "Mabi KNX Integration".to_string();

        let server = Arc::new(KnxServer::new(config).with_group_objects(table));
        let mut events = server.subscribe();
        let task_server = Arc::clone(&server);
        let join = tokio::spawn(async move { task_server.start().await });

        let addr = timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await {
                    Ok(ServerEvent::Started { address }) => return Ok(address),
                    Ok(_) => {}
                    Err(error) => return Err(error),
                }
            }
        })
        .await??;

        Ok(Self {
            server,
            addr,
            join: Some(join),
        })
    }

    pub async fn start_default(table: Arc<GroupObjectTable>) -> TestResult<Self> {
        Self::start_with_table(KnxServerConfig::default(), table).await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.server.subscribe()
    }

    pub async fn shutdown(mut self) -> TestResult {
        self.server.stop().await?;
        if let Some(join) = self.join.take() {
            let _ = timeout(Duration::from_secs(2), join).await??;
        }
        Ok(())
    }
}

impl Drop for ServerHarness {
    fn drop(&mut self) {
        if let Some(join) = &self.join {
            if !join.is_finished() {
                join.abort();
            }
        }
    }
}
