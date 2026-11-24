//! Direct TCP transport. No HTTP runtime, daemon, Java or process is used.
use std::net::SocketAddr;
#[derive(Clone, Debug)]
pub struct Connector {
    pub max_connections: usize,
    pub loopback_only: bool,
    pub client_hash: Option<[u8; 16]>,
}
impl Default for Connector {
    fn default() -> Self {
        Self::direct()
    }
}
impl Connector {
    pub fn direct() -> Self {
        Self {
            max_connections: 8,
            loopback_only: false,
            client_hash: None,
        }
    }
    pub async fn connect_tcp(
        &self,
        host: &str,
        port: u16,
    ) -> std::io::Result<tokio::net::TcpStream> {
        if self.loopback_only
            && !host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        {
            return Err(std::io::Error::other(
                "isolated fixture rejects non-loopback destinations",
            ));
        }
        let stream = tokio::net::TcpStream::connect((host, port)).await?;
        stream.set_nodelay(true)?;
        Ok(stream)
    }
}
pub enum DatagramSource {
    Ip(SocketAddr),
    Host(String, u16),
}
pub fn datagram_source_matches(source: &DatagramSource, target: SocketAddr) -> bool {
    match source {
        DatagramSource::Ip(addr) => *addr == target,
        DatagramSource::Host(host, port) => {
            *port == target.port()
                && host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip == target.ip())
        }
    }
}
