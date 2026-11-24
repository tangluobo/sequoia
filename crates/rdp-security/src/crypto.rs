//! MS-RDPBCGR 5.3: negotiated legacy protection, isolated from the TLS path.
use anyhow::{Result, ensure};
use md5::{Digest as _, Md5};
use rc4::{
    KeyInit as _, Rc4, StreamCipher as _,
    consts::{U8, U16},
};
use sha1::Sha1;
use subtle::ConstantTimeEq as _;
use zeroize::Zeroize as _;

#[derive(Clone, Copy, Debug)]
pub enum Strength {
    Bits40,
    Bits56,
    Bits128,
}
enum Cipher {
    Short(Box<Rc4<U8>>),
    Long(Box<Rc4<U16>>),
}
impl Cipher {
    fn new(key: &[u8]) -> Self {
        if key.len() == 8 {
            Self::Short(Box::new(Rc4::new(key.into())))
        } else {
            Self::Long(Box::new(Rc4::new(key.into())))
        }
    }
    fn apply(&mut self, data: &mut [u8]) {
        match self {
            Self::Short(c) => c.apply_keystream(data),
            Self::Long(c) => c.apply_keystream(data),
        }
    }
}
fn hash(parts: &[&[u8]]) -> [u8; 16] {
    let mut h = Md5::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}
fn salt(secret: &[u8], label: &[u8], client: &[u8], server: &[u8]) -> [u8; 16] {
    let mut sha = Sha1::new();
    for data in [label, secret, client, server] {
        sha.update(data);
    }
    hash(&[secret, &sha.finalize()])
}
fn reduce(mut key: Vec<u8>, strength: Strength) -> Vec<u8> {
    match strength {
        Strength::Bits128 => {}
        Strength::Bits56 => {
            key.truncate(8);
            key[0] = 0xd1;
        }
        Strength::Bits40 => {
            key.truncate(8);
            key[..3].copy_from_slice(&[0xd1, 0x26, 0x9e]);
        }
    }
    key
}
pub struct Direction {
    strength: Strength,
    initial: Vec<u8>,
    key: Vec<u8>,
    mac: Vec<u8>,
    cipher: Cipher,
    uses: u32,
    total: u32,
}
impl Drop for Direction {
    fn drop(&mut self) {
        self.initial.zeroize();
        self.key.zeroize();
        self.mac.zeroize();
    }
}
impl Direction {
    fn new(key: Vec<u8>, mac: Vec<u8>, strength: Strength) -> Self {
        Self {
            cipher: Cipher::new(&key),
            initial: key.clone(),
            key,
            mac,
            strength,
            uses: 0,
            total: 0,
        }
    }
    fn renew(&mut self) {
        if self.uses != 4096 {
            return;
        }
        let mut sha = Sha1::new();
        for data in [&self.initial[..], &[0x36; 40], &self.key[..]] {
            sha.update(data);
        }
        let mut key =
            hash(&[&self.initial, &[0x5c; 48], &sha.finalize()])[..self.key.len()].to_vec();
        Cipher::new(&key).apply(&mut key);
        self.key = reduce(key, self.strength);
        self.cipher = Cipher::new(&self.key);
        self.uses = 0;
    }
    fn signature(&self, data: &[u8], salted: bool) -> [u8; 8] {
        let mut sha = Sha1::new();
        for p in [
            &self.mac[..],
            &[0x36; 40],
            &(data.len() as u32).to_le_bytes(),
            data,
        ] {
            sha.update(p);
        }
        if salted {
            sha.update(self.total.to_le_bytes());
        }
        hash(&[&self.mac, &[0x5c; 48], &sha.finalize()])[..8]
            .try_into()
            .expect("MAC width")
    }
    pub fn encrypt(&mut self, data: &[u8], salted: bool) -> Vec<u8> {
        self.renew();
        let mut out = Vec::with_capacity(data.len() + 8);
        out.extend_from_slice(&self.signature(data, salted));
        out.extend_from_slice(data);
        self.cipher.apply(&mut out[8..]);
        self.uses += 1;
        self.total = self.total.wrapping_add(1);
        out
    }
    pub fn decrypt(&mut self, data: &[u8], salted: bool) -> Result<Vec<u8>> {
        ensure!(data.len() >= 8, "RDP 加密数据缺少校验码");
        self.renew();
        let mut plain = data[8..].to_vec();
        self.cipher.apply(&mut plain);
        ensure!(
            bool::from(self.signature(&plain, salted).ct_eq(&data[..8])),
            "RDP 数据完整性校验失败"
        );
        self.uses += 1;
        self.total = self.total.wrapping_add(1);
        Ok(plain)
    }
}
pub fn keys(client: &[u8; 32], server: &[u8; 32], strength: Strength) -> (Direction, Direction) {
    let mut premaster = zeroize::Zeroizing::new(client[..24].to_vec());
    premaster.extend_from_slice(&server[..24]);
    let mut master = zeroize::Zeroizing::new(Vec::with_capacity(48));
    for label in [&b"A"[..], b"BB", b"CCC"] {
        master.extend_from_slice(&salt(&premaster, label, client, server));
    }
    let mut session = zeroize::Zeroizing::new(Vec::with_capacity(48));
    for label in [&b"X"[..], b"YY", b"ZZZ"] {
        session.extend_from_slice(&salt(&master, label, client, server));
    }
    let mac = reduce(session[..16].to_vec(), strength);
    let receive = reduce(hash(&[&session[16..32], client, server]).to_vec(), strength);
    let send = reduce(hash(&[&session[32..48], client, server]).to_vec(), strength);
    (
        Direction::new(send, mac.clone(), strength),
        Direction::new(receive, mac, strength),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jca_reference_matches_both_directions_salted_mac_and_two_key_renewals() {
        // Generated by tests/RdpCryptoReference.java using JDK 24's JCA RC4,
        // SHA-1 and MD5 providers, independently of this implementation.
        let expected = [
            (
                Strength::Bits40,
                [
                    "778530df160b2010c0e1d77af7de7f8eb0ebc8679fe4cca4085f7e0e20e46258",
                    "b06b48609e3ccd3132f99247bb04a82468af9b5420a18b20eb26f1aedf03fb17",
                ],
            ),
            (
                Strength::Bits56,
                [
                    "1245ea53c24d7891ba88e05aa61e8ce67df2166ea5fd72fd595699d89d5afd3c",
                    "47b6b6c280615a11d8888c3340c31a640b10b4b448a543d5c207415bba1e14be",
                ],
            ),
            (
                Strength::Bits128,
                [
                    "72714da5deb5e9ec959ed44e73928639c8943debea502c2ff88cb08b4b535b78",
                    "3f9763d145c93bffe8f91e16538e59861c5066574a7d88df73b57717fe6c1100",
                ],
            ),
        ];
        let client = std::array::from_fn(|i| i as u8);
        let server = std::array::from_fn(|i| (i + 32) as u8);
        for (strength, expected) in expected {
            let (send, receive) = keys(&client, &server, strength);
            for (mut cipher, expected) in [send, receive].into_iter().zip(expected) {
                let mut aggregate = sha2::Sha256::new();
                for count in 0..8195 {
                    let data = (0..127 + count % 4096)
                        .map(|i| (i * 17 + count) as u8)
                        .collect::<Vec<_>>();
                    aggregate.update(cipher.encrypt(&data, count % 2 == 0));
                }
                let actual = aggregate
                    .finalize()
                    .iter()
                    .map(|v| format!("{v:02x}"))
                    .collect::<String>();
                assert_eq!(actual, expected);
            }
        }
    }
}
