//! Bounded MS-RDPBCGR enhanced-security redirection. Never log one-time secrets.
use crate::config::{Config, Destination};
use anyhow::{Context as _, Result, ensure};
#[cfg(test)]
mod tests;

pub(crate) struct Redirection {
    pub session_id: u32,
    flags: u32,
    pub routing: Option<String>,
    username: Option<String>,
    domain: Option<String>,
    password: Vec<u8>,
    target: Option<String>,
    guid: Vec<u8>,
}

pub(super) fn detect(payload: &[u8], io_channel: u16) -> Result<Option<Redirection>> {
    let Ok(mcs) = ironrdp_pdu::mcs::decode_send_data_indication(payload) else {
        return Ok(None);
    };
    let data = mcs.user_data;
    if mcs.channel_id != io_channel
        || data.len() < 6
        || u16::from_le_bytes([data[2], data[3]]) & 15 != 10
    {
        return Ok(None);
    }
    let total = usize::from(u16::from_le_bytes([data[0], data[1]]));
    ensure!(
        u16::from_le_bytes([data[2], data[3]]) & !15 == 0x10,
        "Invalid redirection Share Control version"
    );
    ensure!(
        (20..=65535).contains(&total) && total <= data.len(),
        "Invalid server redirection size"
    );
    let packet = &data[8..total]; // Share Control header + pad2Octets.
    ensure!(
        u16::from_le_bytes([packet[0], packet[1]]) == 0x400,
        "Invalid server redirection identifier"
    );
    let length = usize::from(u16::from_le_bytes([packet[2], packet[3]]));
    ensure!(
        length >= 12 && length <= packet.len(),
        "Invalid server redirection packet length"
    );
    let session_id = u32::from_le_bytes(packet[4..8].try_into()?);
    let flags = u32::from_le_bytes(packet[8..12].try_into()?);
    let mut input = &packet[12..length];
    let mut fields = std::collections::BTreeMap::new();
    for bit in [1, 2, 4, 8, 16, 256, 512, 4096, 32768, 65536, 2048] {
        if flags & bit != 0 {
            fields.insert(bit, blob(&mut input)?);
        }
    }
    let text = |bit| fields.get(&bit).map(|bytes| unicode(bytes)).transpose();
    let routing = fields
        .get(&2)
        .filter(|v| !v.is_empty())
        .map(|bytes| {
            let text = std::str::from_utf8(bytes).context("Invalid redirection routing token")?;
            let text = text.strip_suffix("\r\n").unwrap_or(text);
            ensure!(
                !text.is_empty()
                    && text.len() <= 238
                    && text.bytes().all(|b| (32..=126).contains(&b)),
                "Invalid redirection routing token length or characters"
            );
            Ok::<_, anyhow::Error>(text.to_owned())
        })
        .transpose()?;
    let mut target = text(256)?.or(text(1)?).or(text(512)?);
    if let Some(addresses) = fields.get(&2048) {
        ensure!(addresses.len() >= 4, "Truncated redirection address list");
        let count = u32::from_le_bytes(addresses[..4].try_into()?);
        ensure!(count <= 128, "Too many redirection addresses");
        let mut addresses = &addresses[4..];
        for _ in 0..count {
            let address = unicode(&blob(&mut addresses)?)?;
            if target.is_none() {
                target = Some(address);
            }
        }
    }
    let username = text(4)?;
    let domain = text(8)?;
    Ok(Some(Redirection {
        session_id,
        flags,
        routing,
        username,
        domain,
        password: fields.remove(&16).unwrap_or_default(),
        target,
        guid: fields.remove(&32768).unwrap_or_default(),
    }))
}

fn blob(input: &mut &[u8]) -> Result<Vec<u8>> {
    ensure!(
        input.len() >= 4,
        "Truncated server redirection field length"
    );
    let length = usize::try_from(u32::from_le_bytes(input[..4].try_into()?))?;
    *input = &input[4..];
    ensure!(
        length <= 65535 && length <= input.len(),
        "Invalid server redirection field length"
    );
    let value = input[..length].to_vec();
    *input = &input[length..];
    Ok(value)
}

fn unicode(bytes: &[u8]) -> Result<String> {
    ensure!(
        bytes.len() >= 2 && bytes.len() % 2 == 0,
        "Invalid redirection UTF-16 length"
    );
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    ensure!(
        units.last() == Some(&0),
        "Unterminated redirection UTF-16 string"
    );
    ensure!(
        !units[..units.len() - 1].contains(&0),
        "Embedded NUL in redirection text"
    );
    Ok(String::from_utf16(&units[..units.len() - 1])?)
}

impl Redirection {
    pub(super) fn informational(&self) -> bool {
        self.flags & 128 != 0
    }
    pub(super) fn opaque_password(&self) -> Option<&[u8]> {
        (self.flags & 0x4000 != 0).then_some(self.password.as_slice())
    }
    pub(super) fn apply(&self, config: &mut Config) -> Result<()> {
        let username = self
            .username
            .clone()
            .context("Redirection missing one-time username")?;
        let password = if self.opaque_password().is_some() {
            ensure!(
                !self.guid.is_empty() && !self.password.is_empty(),
                "RDSTLS redirection is missing its GUID or encrypted password"
            );
            String::new()
        } else {
            unicode(&self.password)?
        };
        if self.routing.is_none()
            && let Some(host) = &self.target
        {
            ensure!(
                !host.is_empty() && host.len() <= 512 && !host.chars().any(char::is_control),
                "Invalid redirection target"
            );
            config.destination = Destination::from_parts(host, config.destination.port());
        }
        config.connector.credentials =
            ironrdp_connector::Credentials::UsernamePassword { username, password };
        config.connector.domain = self.domain.clone();
        config.connector.autologon = true;
        Ok(())
    }
}

pub(super) async fn authenticate<S>(stream: &mut S, redirect: &Redirection) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    tokio::time::timeout(core::time::Duration::from_secs(15), async {
        let mut capabilities = [0; 8];
        stream.read_exact(&mut capabilities).await?;
        ensure!(
            capabilities[..6] == [1, 0, 1, 0, 1, 0]
                && u16::from_le_bytes([capabilities[6], capabilities[7]]) & 1 != 0,
            "Invalid RDSTLS v1 capabilities"
        );
        let mut request = vec![1, 0, 2, 0, 1, 0];
        let mut field = |data: &[u8]| -> Result<()> {
            request.extend_from_slice(
                &u16::try_from(data.len())
                    .context("RDSTLS field too long")?
                    .to_le_bytes(),
            );
            request.extend_from_slice(data);
            Ok(())
        };
        field(&redirect.guid)?;
        let utf16 = |value: &str| {
            value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        field(&utf16(
            redirect
                .username
                .as_deref()
                .context("RDSTLS missing username")?,
        ))?;
        field(&utf16(redirect.domain.as_deref().unwrap_or("")))?;
        field(&redirect.password)?;
        stream.write_all(&request).await?;
        stream.flush().await?;
        let mut response = [0; 10];
        stream.read_exact(&mut response).await?;
        ensure!(
            response[..6] == [1, 0, 4, 0, 1, 0],
            "Invalid RDSTLS authentication response"
        );
        let code = u32::from_le_bytes(response[6..].try_into()?);
        ensure!(
            code == 0,
            "RDSTLS one-time credentials rejected (0x{code:08x})"
        );
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("RDSTLS authentication timed out")?
}
