#[cfg(feature = "https")]
use std::collections::BTreeMap;
#[cfg(feature = "https")]
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::PathBuf;
#[cfg(feature = "https")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(feature = "https")]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(feature = "https")]
use tokio::net::TcpListener;
#[cfg(feature = "https")]
use tokio::sync::{broadcast, Semaphore};
#[cfg(feature = "https")]
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
#[cfg(feature = "https")]
use tokio_rustls::rustls::ServerConfig as RustlsServerConfig;
#[cfg(feature = "https")]
use tokio_rustls::TlsAcceptor;
#[cfg(feature = "https")]
use tracing::{error, info, warn};

#[cfg(feature = "https")]
use crate::channel::secure_channel::SecureChannel;
use crate::error::{OpcUaError, OpcUaResult};
use crate::transport::adapter::ConnectionInitiationMode;
use crate::transport::runtime::TransportRuntime;

#[cfg(feature = "https")]
const MAX_HTTP_HEADER_BYTES: usize = 64 * 1024;
#[cfg(feature = "https")]
const MAX_HTTP_BODY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct HttpsTransportConfig {
    pub bind_address: SocketAddr,
    pub endpoint_path: String,
    pub max_connections: usize,
    pub connection_timeout: std::time::Duration,
    pub(crate) initiation_mode: ConnectionInitiationMode,
    pub certificate_path: Option<PathBuf>,
    pub private_key_path: Option<PathBuf>,
}

impl Default for HttpsTransportConfig {
    fn default() -> Self {
        Self {
            bind_address: SocketAddr::from(([0, 0, 0, 0], 4840)),
            endpoint_path: "/opcua".to_string(),
            max_connections: 1000,
            connection_timeout: std::time::Duration::from_secs(60),
            initiation_mode: ConnectionInitiationMode::Listener,
            certificate_path: None,
            private_key_path: None,
        }
    }
}

#[cfg(feature = "https")]
pub struct OpcUaHttpsListener {
    config: HttpsTransportConfig,
    runtime: Arc<TransportRuntime>,
    shutdown: Arc<AtomicBool>,
    shutdown_tx: broadcast::Sender<()>,
}

#[cfg(not(feature = "https"))]
pub struct OpcUaHttpsListener {
    runtime: Arc<TransportRuntime>,
}

#[cfg(feature = "https")]
impl OpcUaHttpsListener {
    pub(crate) fn new(config: HttpsTransportConfig, runtime: Arc<TransportRuntime>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            runtime,
            shutdown: Arc::new(AtomicBool::new(false)),
            shutdown_tx,
        }
    }

    pub fn metrics(&self) -> &Arc<crate::transport::metrics::TransportMetrics> {
        self.runtime.metrics()
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.shutdown_tx.send(());
    }

    pub async fn run(&self) -> OpcUaResult<()> {
        let tls_acceptor = build_tls_acceptor(&self.config)?;
        let listener = TcpListener::bind(self.config.bind_address)
            .await
            .map_err(|error| OpcUaError::Bind {
                address: self.config.bind_address,
                reason: error.to_string(),
            })?;

        info!(address = %self.config.bind_address, path = %self.config.endpoint_path, "OPC UA HTTPS server listening");

        let semaphore = Arc::new(Semaphore::new(self.config.max_connections));
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            let permit = match semaphore.clone().try_acquire_owned() {
                                Ok(permit) => permit,
                                Err(_) => {
                                    warn!(peer = %peer_addr, "Max HTTPS connections reached, rejecting");
                                    self.runtime.record_rejection();
                                    drop(stream);
                                    continue;
                                }
                            };

                            let runtime = self.runtime.clone();
                            let shutdown = self.shutdown.clone();
                            let endpoint_path = self.config.endpoint_path.clone();
                            let tls_acceptor = tls_acceptor.clone();
                            tokio::spawn(async move {
                                let result = async {
                                    let tls_stream = tls_acceptor.accept(stream).await.map_err(|error| {
                                        OpcUaError::Connection(format!("HTTPS TLS accept failed: {}", error))
                                    })?;
                                    handle_https_connection(
                                        tls_stream,
                                        runtime,
                                        shutdown,
                                        endpoint_path,
                                    )
                                    .await
                                }
                                .await;
                                if let Err(error) = result {
                                    warn!(peer = %peer_addr, error = %error, "HTTPS connection error");
                                }
                                drop(permit);
                            });
                        }
                        Err(error) => {
                            error!(error = %error, "Failed to accept HTTPS connection");
                            self.runtime.metrics().record_error();
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("HTTPS listener shutdown signal received");
                    break;
                }
            }
        }

        info!("OPC UA HTTPS server stopped");
        Ok(())
    }
}

impl HttpsTransportConfig {
    pub(crate) fn validate(&self) -> OpcUaResult<()> {
        #[cfg(not(feature = "https"))]
        {
            return Err(OpcUaError::Config(
                "HTTPS transport requires the mabi-opcua `https` feature".to_string(),
            ));
        }

        #[cfg(feature = "https")]
        {
            if self.certificate_path.is_none() {
                return Err(OpcUaError::Config(
                    "HTTPS transport requires certificate_path".to_string(),
                ));
            }
            if self.private_key_path.is_none() {
                return Err(OpcUaError::Config(
                    "HTTPS transport requires private_key_path".to_string(),
                ));
            }
            Ok(())
        }
    }
}

#[cfg(not(feature = "https"))]
impl OpcUaHttpsListener {
    pub(crate) fn new(_config: HttpsTransportConfig, runtime: Arc<TransportRuntime>) -> Self {
        Self { runtime }
    }

    pub fn metrics(&self) -> &Arc<crate::transport::metrics::TransportMetrics> {
        self.runtime.metrics()
    }

    pub fn shutdown(&self) {}

    pub async fn run(&self) -> OpcUaResult<()> {
        Err(OpcUaError::Config(
            "HTTPS transport requires the mabi-opcua `https` feature".to_string(),
        ))
    }
}

#[cfg(feature = "https")]
struct HttpRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    keep_alive: bool,
}

#[cfg(feature = "https")]
async fn handle_https_connection<S>(
    mut stream: S,
    runtime: Arc<TransportRuntime>,
    shutdown: Arc<AtomicBool>,
    endpoint_path: String,
) -> OpcUaResult<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    runtime.record_connection();
    let context = runtime.build_service_context(Arc::new(SecureChannel::new_unsecured()));
    let mut read_buffer = Vec::new();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let request = match tokio::time::timeout(
            std::time::Duration::from_secs(60),
            read_http_request(&mut stream, &mut read_buffer),
        )
        .await
        {
            Ok(Ok(Some(request))) => request,
            Ok(Ok(None)) => break,
            Ok(Err(error)) => {
                runtime.record_error();
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "Bad Request",
                    &[("content-type", "text/plain; charset=utf-8")],
                    error.to_string().as_bytes(),
                    false,
                )
                .await;
                break;
            }
            Err(_) => {
                let _ = write_http_response(
                    &mut stream,
                    408,
                    "Request Timeout",
                    &[("content-type", "text/plain; charset=utf-8")],
                    b"request timeout",
                    false,
                )
                .await;
                break;
            }
        };

        if request.method == "GET" && request.path == "/health" {
            write_http_response(
                &mut stream,
                200,
                "OK",
                &[("content-type", "application/json")],
                br#"{"status":"ok","protocol":"https"}"#,
                request.keep_alive,
            )
            .await?;
            continue;
        }

        if request.method != "POST" {
            write_http_response(
                &mut stream,
                405,
                "Method Not Allowed",
                &[("content-type", "text/plain; charset=utf-8")],
                b"only POST is supported for OPC UA HTTPS requests",
                false,
            )
            .await?;
            break;
        }

        if request.path != endpoint_path {
            write_http_response(
                &mut stream,
                404,
                "Not Found",
                &[("content-type", "text/plain; charset=utf-8")],
                b"unknown OPC UA HTTPS endpoint",
                request.keep_alive,
            )
            .await?;
            if !request.keep_alive {
                break;
            }
            continue;
        }

        runtime.record_message_received(request.body.len());
        match runtime
            .dispatch_service_payload(&request.body, &context)
            .await
        {
            Ok(response_body) => {
                runtime.record_message_sent(response_body.len());
                write_http_response(
                    &mut stream,
                    200,
                    "OK",
                    &[("content-type", "application/octet-stream")],
                    &response_body,
                    request.keep_alive,
                )
                .await?;
            }
            Err(error) => {
                runtime.record_error();
                write_http_response(
                    &mut stream,
                    500,
                    "Internal Server Error",
                    &[("content-type", "text/plain; charset=utf-8")],
                    error.to_string().as_bytes(),
                    false,
                )
                .await?;
                break;
            }
        }

        if !request.keep_alive {
            break;
        }
    }

    runtime.record_disconnection();
    Ok(())
}

#[cfg(feature = "https")]
async fn read_http_request<S>(
    stream: &mut S,
    buffer: &mut Vec<u8>,
) -> OpcUaResult<Option<HttpRequest>>
where
    S: AsyncRead + Unpin,
{
    loop {
        if let Some(header_end) = find_header_end(buffer) {
            let header_bytes = &buffer[..header_end];
            let header_text = std::str::from_utf8(header_bytes).map_err(|error| {
                OpcUaError::ProtocolError(format!("invalid HTTP header utf8: {}", error))
            })?;
            let mut lines = header_text.split("\r\n");
            let request_line = lines
                .next()
                .ok_or_else(|| OpcUaError::ProtocolError("missing HTTP request line".into()))?;
            let mut request_parts = request_line.split_whitespace();
            let method = request_parts
                .next()
                .ok_or_else(|| OpcUaError::ProtocolError("missing HTTP method".into()))?
                .to_string();
            let path = request_parts
                .next()
                .ok_or_else(|| OpcUaError::ProtocolError("missing HTTP path".into()))?
                .to_string();

            let mut headers = BTreeMap::new();
            for line in lines {
                if line.is_empty() {
                    continue;
                }
                let (name, value) = line.split_once(':').ok_or_else(|| {
                    OpcUaError::ProtocolError(format!("malformed HTTP header '{}'", line))
                })?;
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }

            let content_length = headers
                .get("content-length")
                .map(|value| {
                    value.parse::<usize>().map_err(|error| {
                        OpcUaError::ProtocolError(format!(
                            "invalid content-length '{}': {}",
                            value, error
                        ))
                    })
                })
                .transpose()?
                .unwrap_or(0);

            if content_length > MAX_HTTP_BODY_BYTES {
                return Err(OpcUaError::MessageTooLarge {
                    size: content_length,
                    max: MAX_HTTP_BODY_BYTES,
                });
            }

            let total_len = header_end + 4 + content_length;
            while buffer.len() < total_len {
                let mut chunk = [0u8; 4096];
                let read = stream.read(&mut chunk).await?;
                if read == 0 {
                    return Err(OpcUaError::ProtocolError(
                        "connection closed before HTTP body completed".into(),
                    ));
                }
                buffer.extend_from_slice(&chunk[..read]);
            }

            let body = buffer[header_end + 4..total_len].to_vec();
            buffer.drain(..total_len);

            let keep_alive = headers
                .get("connection")
                .map(|value| !value.eq_ignore_ascii_case("close"))
                .unwrap_or(true);

            return Ok(Some(HttpRequest {
                method,
                path,
                headers,
                body,
                keep_alive,
            }));
        }

        if buffer.len() > MAX_HTTP_HEADER_BYTES {
            return Err(OpcUaError::MessageTooLarge {
                size: buffer.len(),
                max: MAX_HTTP_HEADER_BYTES,
            });
        }

        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            if buffer.is_empty() {
                return Ok(None);
            }
            return Err(OpcUaError::ProtocolError(
                "connection closed before HTTP headers completed".into(),
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

#[cfg(feature = "https")]
async fn write_http_response<S>(
    stream: &mut S,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    keep_alive: bool,
) -> OpcUaResult<()>
where
    S: AsyncWrite + Unpin,
{
    let mut response = format!("HTTP/1.1 {} {}\r\n", status, reason);
    response.push_str(&format!("content-length: {}\r\n", body.len()));
    response.push_str(&format!(
        "connection: {}\r\n",
        if keep_alive { "keep-alive" } else { "close" }
    ));
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");

    stream.write_all(response.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(feature = "https")]
fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

#[cfg(feature = "https")]
fn build_tls_acceptor(config: &HttpsTransportConfig) -> OpcUaResult<TlsAcceptor> {
    let certificate_path = config.certificate_path.as_ref().ok_or_else(|| {
        OpcUaError::Config("HTTPS transport requires certificate_path".to_string())
    })?;
    let private_key_path = config.private_key_path.as_ref().ok_or_else(|| {
        OpcUaError::Config("HTTPS transport requires private_key_path".to_string())
    })?;

    let certs = load_certificates(certificate_path)?;
    let key = load_private_key(private_key_path)?;
    let server_config = RustlsServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|error| {
            OpcUaError::Security(format!("HTTPS TLS configuration failed: {}", error))
        })?;
    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

#[cfg(feature = "https")]
fn load_certificates(path: &PathBuf) -> OpcUaResult<Vec<CertificateDer<'static>>> {
    let bytes = std::fs::read(path).map_err(|error| {
        OpcUaError::Security(format!(
            "failed to read HTTPS certificate '{}': {}",
            path.display(),
            error
        ))
    })?;
    if bytes.starts_with(b"-----BEGIN") {
        let mut reader = BufReader::new(bytes.as_slice());
        let certs = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                OpcUaError::Security(format!(
                    "failed to parse HTTPS certificate PEM '{}': {}",
                    path.display(),
                    error
                ))
            })?;
        if certs.is_empty() {
            return Err(OpcUaError::Security(format!(
                "no certificates found in '{}'",
                path.display()
            )));
        }
        Ok(certs)
    } else {
        Ok(vec![CertificateDer::from(bytes)])
    }
}

#[cfg(feature = "https")]
fn load_private_key(path: &PathBuf) -> OpcUaResult<PrivateKeyDer<'static>> {
    let bytes = std::fs::read(path).map_err(|error| {
        OpcUaError::Security(format!(
            "failed to read HTTPS private key '{}': {}",
            path.display(),
            error
        ))
    })?;
    if bytes.starts_with(b"-----BEGIN") {
        let mut reader = BufReader::new(bytes.as_slice());
        rustls_pemfile::private_key(&mut reader)
            .map_err(|error| {
                OpcUaError::Security(format!(
                    "failed to parse HTTPS private key PEM '{}': {}",
                    path.display(),
                    error
                ))
            })?
            .ok_or_else(|| {
                OpcUaError::Security(format!("no private key found in '{}'", path.display()))
            })
    } else {
        Ok(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(bytes)))
    }
}

#[cfg(all(test, feature = "https"))]
mod tests {
    use super::*;

    #[test]
    fn http_header_end_is_detected() {
        assert_eq!(
            find_header_end(b"GET / HTTP/1.1\r\nHost: x\r\n\r\nbody"),
            Some(23)
        );
    }

    #[tokio::test]
    async fn http_response_writes_expected_status_line() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let writer = tokio::spawn(async move {
            write_http_response(
                &mut server,
                200,
                "OK",
                &[("content-type", "text/plain")],
                b"hello",
                false,
            )
            .await
            .unwrap();
        });

        let mut bytes = Vec::new();
        client.read_to_end(&mut bytes).await.unwrap();
        writer.await.unwrap();

        let response = String::from_utf8(bytes).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("content-length: 5\r\n"));
        assert!(response.ends_with("hello"));
    }
}
