use std::net::SocketAddrV4;

/// Read the original server.met caches without changing or rewriting them.
pub fn parse_met(bytes: &[u8]) -> Result<Vec<String>, &'static str> {
    if bytes.len() > 1024 * 1024 || bytes.len() < 5 || !matches!(bytes[0], 0x0e | 0x0f | 0xe0) {
        return Err("Invalid server.met header");
    }
    let count = u32::from_le_bytes(bytes[1..5].try_into().unwrap());
    if count > 1024 {
        return Err("Too many eD2k servers");
    }
    let mut remaining = &bytes[5..];
    let mut entries = Vec::new();
    for _ in 0..count {
        let header = remaining.get(..10).ok_or("Truncated eD2k server")?;
        let ip = std::net::Ipv4Addr::new(header[0], header[1], header[2], header[3]);
        let port = u16::from_le_bytes(header[4..6].try_into().unwrap());
        let tags = u32::from_le_bytes(header[6..10].try_into().unwrap());
        if tags > 128 {
            return Err("Too many server tags");
        }
        remaining = &remaining[10..];
        for _ in 0..tags {
            remaining = super::protocol::read_tag(remaining)
                .ok_or("Invalid server tag")?
                .2;
        }
        if !ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast() && port != 0 {
            entries.push(format!("{ip}:{port}"));
        }
    }
    Ok(entries)
}

#[derive(Debug, Clone)]
pub struct ServerEntry {
    pub ip: String,
    pub port: u16,
    pub name: String,
}

impl ServerEntry {
    fn new(ip: &str, port: u16, name: &str) -> Self {
        Self {
            ip: ip.to_string(),
            port,
            name: name.to_string(),
        }
    }

    pub fn to_socket_addr(&self) -> Option<SocketAddrV4> {
        let ip = self.ip.parse().ok()?;
        Some(SocketAddrV4::new(ip, self.port))
    }
}

pub fn server_list(entries: &[String]) -> Vec<ServerEntry> {
    let servers: Vec<ServerEntry> = entries
        .iter()
        .filter_map(|s| {
            let (ip, port) = s.split_once(':')?;
            let _: std::net::Ipv4Addr = ip.parse().ok()?;
            Some(ServerEntry::new(ip, port.parse().ok()?, "Custom"))
        })
        .collect();
    if !servers.is_empty() {
        return servers;
    }
    vec![
        ServerEntry::new("176.123.5.89", 4725, "eMule Sunrise"),
        ServerEntry::new("45.82.80.155", 5687, "eMule Security"),
        ServerEntry::new("85.239.33.123", 4232, "!! Sharing-Devils No.2 !!"),
        ServerEntry::new("91.208.162.87", 4232, "!! Sharing-Devils No.1 !!"),
        ServerEntry::new("145.239.2.134", 4661, "GrupoTS Server"),
    ]
}
