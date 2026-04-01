use std::net::SocketAddr;
use std::sync::Arc;

use crate::error::OpcUaResult;
use crate::transport::https_listener::{HttpsTransportConfig, OpcUaHttpsListener};
use crate::transport::metrics::TransportMetrics;
use crate::transport::tcp_listener::{OpcUaTcpListener, TcpTransportConfig};
use crate::transport::tcp_reverse_connector::{TcpReverseConnectConfig, TcpReverseConnector};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionInitiationMode {
    Listener,
    ReverseConnect,
}

impl Default for ConnectionInitiationMode {
    fn default() -> Self {
        Self::Listener
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportAdapterKind {
    TcpListener,
    TcpReverse,
    Https,
}

#[derive(Debug, Clone)]
pub(crate) enum TransportAdapterConfig {
    TcpListener(TcpTransportConfig),
    TcpReverse(TcpReverseConnectConfig),
    Https(HttpsTransportConfig),
}

impl TransportAdapterConfig {
    pub(crate) fn kind(&self) -> TransportAdapterKind {
        match self {
            Self::TcpListener(_) => TransportAdapterKind::TcpListener,
            Self::TcpReverse(_) => TransportAdapterKind::TcpReverse,
            Self::Https(_) => TransportAdapterKind::Https,
        }
    }

    pub(crate) fn initiation_mode(&self) -> ConnectionInitiationMode {
        match self {
            Self::TcpListener(config) => config.initiation_mode,
            Self::TcpReverse(config) => config.initiation_mode,
            Self::Https(config) => config.initiation_mode,
        }
    }

    pub(crate) fn socket_address(&self) -> SocketAddr {
        match self {
            Self::TcpListener(config) => config.bind_address,
            Self::TcpReverse(config) => config.target_address,
            Self::Https(config) => config.bind_address,
        }
    }
}

#[derive(Clone)]
pub(crate) enum TransportListener {
    TcpListener(Arc<OpcUaTcpListener>),
    TcpReverse(Arc<TcpReverseConnector>),
    Https(Arc<OpcUaHttpsListener>),
}

impl TransportListener {
    pub(crate) fn metrics(&self) -> &Arc<TransportMetrics> {
        match self {
            Self::TcpListener(listener) => listener.metrics(),
            Self::TcpReverse(listener) => listener.metrics(),
            Self::Https(listener) => listener.metrics(),
        }
    }

    pub(crate) fn shutdown(&self) {
        match self {
            Self::TcpListener(listener) => listener.shutdown(),
            Self::TcpReverse(listener) => listener.shutdown(),
            Self::Https(listener) => listener.shutdown(),
        }
    }

    pub(crate) async fn run(&self) -> OpcUaResult<()> {
        match self {
            Self::TcpListener(listener) => listener.run().await,
            Self::TcpReverse(listener) => listener.run().await,
            Self::Https(listener) => listener.run().await,
        }
    }
}
