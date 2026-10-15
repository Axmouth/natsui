use async_nats::rustls::{
    self,
    pki_types::{PrivateKeyDer, ServerName, pem::PemObject},
};
use axum::body::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use std::{io::Read, net::SocketAddr, path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};
use tokio_rustls::{TlsAcceptor, TlsConnector, server::TlsStream};

pub struct Configuration {
    server: Arc<rustls::ServerConfig>,
    roots: rustls::RootCertStore,
    origin: reqwest::Url,
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|_| "HTTPS file is not readable")?;
    let metadata = file
        .metadata()
        .map_err(|_| "HTTPS file metadata is unavailable")?;
    if !metadata.is_file() || metadata.len() > 1_048_576 {
        return Err("HTTPS material must be a regular file at most 1 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| "HTTPS file is not readable")?;
    if bytes.is_empty() || bytes.len() > 1_048_576 {
        return Err("Invalid HTTPS file size".into());
    }
    Ok(bytes)
}

impl Configuration {
    pub fn from_env() -> Result<Option<Self>, String> {
        let cert = std::env::var_os("NATSUI_HTTPS_CERT");
        let key = std::env::var_os("NATSUI_HTTPS_KEY");
        let ca = std::env::var_os("NATSUI_HTTPS_CA");
        let (cert, key) = match (cert, key) {
            (None, None) if ca.is_none() => return Ok(None),
            (Some(cert), Some(key)) => (cert, key),
            _ => {
                return Err(
                    "Native HTTPS requires both NATSUI_HTTPS_CERT and NATSUI_HTTPS_KEY".into(),
                );
            }
        };
        let origin = std::env::var("NATSUI_PUBLIC_URL")
            .map_err(|_| "Native HTTPS requires NATSUI_PUBLIC_URL")?;
        let auth = crate::auth::Auth::from_env()?;
        auth.configure_origin(&origin)?;
        let origin = auth
            .origin()
            .ok_or("Native HTTPS requires authenticated access and an HTTPS public origin")?;
        let certs = crate::connection::certificates(&read(Path::new(&cert))?)?;
        let key = PrivateKeyDer::from_pem_slice(&read(Path::new(&key))?)
            .map_err(|_| "NATSUI_HTTPS_KEY requires an unencrypted PEM private key")?;
        let mut server = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|_| "Invalid or mismatched HTTPS certificate and key")?;
        server.alpn_protocols = vec![b"http/1.1".to_vec()];
        let trusted = if let Some(path) = ca {
            crate::connection::certificates(&read(Path::new(&path))?)?
        } else {
            rustls_native_certs::load_native_certs()
                .map_err(|_| "System trust store is unavailable")?
        };
        let mut roots = rustls::RootCertStore::empty();
        for cert in trusted {
            roots
                .add(cert)
                .map_err(|_| "Invalid HTTPS trust certificate")?;
        }
        if roots.is_empty() {
            return Err("HTTPS trust store is empty".into());
        }
        Ok(Some(Self {
            server: Arc::new(server),
            roots,
            origin,
        }))
    }
    pub fn listener(self, tcp: TcpListener) -> Listener {
        Listener {
            tcp,
            acceptor: TlsAcceptor::from(self.server),
            pending: JoinSet::new(),
        }
    }
}

pub struct Listener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
    pending: JoinSet<Option<(TlsStream<TcpStream>, SocketAddr)>>,
}
impl axum::serve::Listener for Listener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            tokio::select! {
                result = self.pending.join_next(), if !self.pending.is_empty() => {
                    if let Some(Ok(Some(stream))) = result { return stream; }
                }
                accepted = self.tcp.accept(), if self.pending.len() < 64 => {
                    match accepted {
                        Ok((socket, address)) => {
                            let acceptor = self.acceptor.clone();
                            // Incomplete handshakes cannot serialize or grow without bound.
                            self.pending.spawn(async move {
                                tokio::time::timeout(Duration::from_secs(5), acceptor.accept(socket)).await.ok()?.ok().map(|stream| (stream, address))
                            });
                        }
                        Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                    }
                }
            }
        }
    }
    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.tcp.local_addr()
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

// CLI credentials never follow public DNS or redirects. TLS still verifies the
// configured public hostname and CA while the socket always targets loopback.
pub async fn local_request(
    port: u16,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<(axum::http::StatusCode, Bytes), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let tls = Configuration::from_env()?;
        let socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        let (socket, host): (Box<dyn Io>, String) = if let Some(tls) = tls {
            let host = tls.origin.host_str().ok_or("Missing HTTPS hostname")?;
            let unbracketed = host
                .strip_prefix('[')
                .and_then(|h| h.strip_suffix(']'))
                .unwrap_or(host);
            let name = match unbracketed.parse::<std::net::IpAddr>() {
                Ok(ip) => ServerName::IpAddress(ip.into()),
                Err(_) => ServerName::try_from(host.to_owned())?,
            };
            let client = rustls::ClientConfig::builder()
                .with_root_certificates(tls.roots)
                .with_no_client_auth();
            let socket = TlsConnector::from(Arc::new(client))
                .connect(name, socket)
                .await?;
            (
                Box::new(socket),
                tls.origin
                    .origin()
                    .ascii_serialization()
                    .trim_start_matches("https://")
                    .to_owned(),
            )
        } else {
            (Box::new(socket), format!("127.0.0.1:{port}"))
        };
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(socket)).await?;
        let task = tokio::spawn(async move {
            let _ = connection.await;
        });
        // Dropping on timeout also closes the connection task and its socket.
        struct Abort(tokio::task::JoinHandle<()>);
        impl Drop for Abort {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _connection = Abort(task);
        let request = axum::http::Request::builder()
            .method(if body.is_some() { "POST" } else { "GET" })
            .uri(path)
            .header("host", host)
            .header("x-natsui-request", "1")
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(
                body.map(|v| v.to_string()).unwrap_or_default(),
            )))?;
        let response = sender.send_request(request).await?;
        let status = response.status();
        let bytes = Limited::new(response.into_body(), 65536)
            .collect()
            .await
            .map_err(|_| "Local HTTP response exceeded its limit or was interrupted")?
            .to_bytes();
        Ok::<_, Box<dyn std::error::Error>>((status, bytes))
    })
    .await?
}
