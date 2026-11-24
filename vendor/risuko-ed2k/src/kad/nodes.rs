//! Bounded, read-only import of the original eMule/jed2k nodes.dat bootstrap cache.
use std::net::{Ipv4Addr, SocketAddrV4};

pub fn parse(bytes: &[u8]) -> Result<Vec<SocketAddrV4>, &'static str> {
    if bytes.len() > 1024 * 1024 {
        return Err("nodes.dat too large");
    }
    let word = |at: usize| -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(
            bytes
                .get(at..at + 4)
                .ok_or("Truncated nodes.dat")?
                .try_into()
                .unwrap(),
        ))
    };
    let first = word(0)?;
    let (offset, count, stride) = if first != 0 {
        (4, first, 25)
    } else {
        let version = word(4)?;
        match version {
            1 => (12, word(8)?, 25),
            2 => (12, word(8)?, 34),
            3 => {
                let edition = word(8)?;
                (16, word(12)?, if edition == 1 { 25 } else { 34 })
            }
            _ => return Err("Unsupported nodes.dat version"),
        }
    };
    if count > 32768 || bytes.len() < offset + count as usize * stride {
        return Err("Truncated nodes.dat contacts");
    }
    let mut seeds = Vec::new();
    for entry in bytes[offset..offset + count as usize * stride].chunks_exact(stride) {
        let ip =
            Ipv4Addr::from(u32::from_le_bytes(entry[16..20].try_into().unwrap()).to_be_bytes());
        let port = u16::from_le_bytes(entry[20..22].try_into().unwrap());
        let addr = SocketAddrV4::new(ip, port);
        if super::routing::is_public_ipv4(ip)
            && port != 0
            && entry[24] >= 2
            && !seeds.contains(&addr)
        {
            seeds.push(addr);
        }
        if seeds.len() == 32 {
            break;
        }
    }
    Ok(seeds)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_legacy_and_all_supported_nodes_dat_versions_without_private_endpoints() {
        for (version, edition, stride) in
            [(0, 0, 25), (1, 0, 25), (2, 0, 34), (3, 0, 34), (3, 1, 25)]
        {
            let mut bytes = Vec::new();
            if version == 0 {
                bytes.extend_from_slice(&2u32.to_le_bytes());
            } else {
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.extend_from_slice(&(version as u32).to_le_bytes());
                if version == 3 {
                    bytes.extend_from_slice(&(edition as u32).to_le_bytes());
                }
                bytes.extend_from_slice(&2u32.to_le_bytes());
            }
            for ip in [Ipv4Addr::new(78, 201, 39, 76), Ipv4Addr::LOCALHOST] {
                let mut entry = vec![0; stride];
                entry[0] = 1;
                entry[16..20].copy_from_slice(&u32::from_be_bytes(ip.octets()).to_le_bytes());
                entry[20..22].copy_from_slice(&4672u16.to_le_bytes());
                entry[24] = 8;
                bytes.extend(entry);
            }
            assert_eq!(
                parse(&bytes).unwrap(),
                vec!["78.201.39.76:4672".parse().unwrap()]
            );
            assert!(parse(&bytes[..bytes.len() - 1]).is_err());
        }
        assert!(parse(&[]).is_err());
    }
}
