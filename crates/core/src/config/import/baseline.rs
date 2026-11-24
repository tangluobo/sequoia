use super::*;
use serde::{Deserialize, Serialize};

/// A bounded duplicate index for background parsing. It contains hashes and IDs,
/// never copied passwords or complete existing connection objects.
#[derive(Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub(super) signature: [u8; 32],
    pub(super) keys: Vec<(String, [u8; 32])>,
}
impl Baseline {
    pub fn new(store: &Store) -> Result<Self> {
        Ok(Self {
            signature: signature(&store.connections)?,
            keys: store
                .connections
                .iter()
                .map(|c| {
                    (
                        c.id.clone(),
                        Sha256::digest(duplicate_key(c).as_bytes()).into(),
                    )
                })
                .collect(),
        })
    }
    pub fn retained_bytes(&self) -> usize {
        self.keys.capacity() * std::mem::size_of::<(String, [u8; 32])>()
            + self.keys.iter().map(|(id, _)| id.capacity()).sum::<usize>()
    }
}
impl std::fmt::Debug for Baseline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportBaseline")
            .field("connections", &self.keys.len())
            .finish_non_exhaustive()
    }
}
