//! Codec matching Java ConfigManager / SecurityUtils, including its fixed AES key.
use crate::Connection;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, block_padding::Pkcs7};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

pub const MARKER: &str = "TOMATO_ENCRYPTED";
const KEY: &[u8; 16] = b"tomato_connect_k";

pub(crate) fn encrypt_text(text: &[u8]) -> String {
    STANDARD.encode(
        ecb::Encryptor::<aes::Aes128>::new(KEY.into()).encrypt_padded_vec_mut::<Pkcs7>(text),
    )
}
pub fn encode(connections: &[Connection]) -> Result<String> {
    encode_json(connections)
}

pub fn encode_json(value: &(impl serde::Serialize + ?Sized)) -> Result<String> {
    Ok(format!(
        "{MARKER}{}",
        encrypt_text(&serde_json::to_vec_pretty(value)?)
    ))
}

pub fn decode_encrypted_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    let encoded = text.strip_prefix(MARKER).context("缺少配置加密标记")?;
    Ok(serde_json::from_str(&decrypt(encoded)?)?)
}

pub(crate) fn decrypt(value: &str) -> Result<String> {
    let data = STANDARD
        .decode(value.trim())
        .context("旧配置 Base64 无效")?;
    let plain = ecb::Decryptor::<aes::Aes128>::new(KEY.into())
        .decrypt_padded_vec_mut::<Pkcs7>(&data)
        .map_err(|_| anyhow::anyhow!("旧配置解密失败"))?;
    String::from_utf8(plain).context("解密后的配置不是 UTF-8")
}

pub fn decode(content: &str) -> Result<Vec<Connection>> {
    decode_impl(content, false)
}

/// Import assigns fresh identities, including to legacy null IDs/names. Keep
/// loading an active repository strict; only the import planner opts into this.
pub(crate) fn decode_import(content: &str) -> Result<Vec<Connection>> {
    decode_impl(content, true)
}

fn decode_impl(content: &str, importing: bool) -> Result<Vec<Connection>> {
    let text = content.trim_start_matches('\u{feff}').trim();
    if text.is_empty() || text == "null" {
        return Ok(Vec::new());
    }
    let encrypted = text.starts_with(MARKER);
    let json = if let Some(data) = text.strip_prefix(MARKER) {
        decrypt(data)?
    } else {
        text.to_string()
    };
    let mut value: Value = serde_json::from_str(json.trim_start_matches('\u{feff}'))
        .context("连接配置不是有效 JSON")?;
    if importing {
        let array = if value.is_array() {
            value.as_array_mut()
        } else {
            value.get_mut("connections").and_then(Value::as_array_mut)
        };
        if let Some(array) = array {
            for item in array {
                for key in ["id", "name"] {
                    if item.get(key).is_some_and(Value::is_null) {
                        item[key] = Value::String(String::new());
                    }
                }
            }
        }
    }
    let mut connections: Vec<Connection> = if value.is_null() {
        Vec::new()
    } else if value.is_array() {
        serde_json::from_value(value)?
    } else if let Some(array) = value.get("connections") {
        serde_json::from_value(array.clone())?
    } else {
        bail!("需要连接数组或含 connections 的对象")
    };
    if !encrypted {
        for item in &mut connections {
            for field in ["password", "sshTunnelPassword"] {
                if let Ok(plain) = decrypt(item.text(field)) {
                    item.set(field, plain);
                }
            }
        }
    }
    Ok(connections)
}
