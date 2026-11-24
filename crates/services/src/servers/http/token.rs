use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct Tokens(hmac::Key);
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Tokens {
    pub fn new() -> anyhow::Result<Self> {
        let mut bytes = [0; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| anyhow::anyhow!("无法生成下载令牌密钥"))?;
        Ok(Self(hmac::Key::new(hmac::HMAC_SHA256, &bytes)))
    }
    pub fn link(&self, path: &str) -> String {
        let expires = now() + 300;
        let signature = hmac::sign(&self.0, format!("GET\n{path}\n{expires}").as_bytes());
        format!(
            "{path}?token={expires}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        )
    }
    pub fn valid(&self, method: &http::Method, path: &str, token: &str) -> bool {
        if !matches!(*method, http::Method::GET | http::Method::HEAD) || token.len() > 256 {
            return false;
        }
        let Some((expires, signature)) = token.split_once('.') else {
            return false;
        };
        let Ok(expires) = expires.parse::<u64>() else {
            return false;
        };
        if expires < now() {
            return false;
        }
        let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
            return false;
        };
        hmac::verify(
            &self.0,
            format!("GET\n{path}\n{expires}").as_bytes(),
            &signature,
        )
        .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_hmac_wire_format_is_bound_to_path_method_expiry_and_process_key() {
        let tokens = Tokens(hmac::Key::new(hmac::HMAC_SHA256, &[7; 32]));
        // Independent Node crypto HMAC-SHA256 vector for the original Java payload.
        let value = "4102444800.nPcPWuYkiVSeu_aonG7JaUYEFRs_tRZAxEfiuesEfwM";
        assert!(tokens.valid(&http::Method::GET, "/fixture/a%2Bb.txt", value));
        assert!(tokens.valid(&http::Method::HEAD, "/fixture/a%2Bb.txt", value));
        assert!(!tokens.valid(&http::Method::PUT, "/fixture/a%2Bb.txt", value));
        assert!(!tokens.valid(&http::Method::GET, "/fixture/a+b.txt", value));
        assert!(!tokens.valid(
            &http::Method::GET,
            "/fixture/a%2Bb.txt",
            &value.replace("4102444800", "1")
        ));
        assert!(!tokens.valid(
            &http::Method::GET,
            "/fixture/a%2Bb.txt",
            &format!("{value}.extra")
        ));
        assert!(!Tokens::new().expect("OS random").valid(
            &http::Method::GET,
            "/fixture/a%2Bb.txt",
            value
        ));
    }
}
