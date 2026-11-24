//! Hierarchical connection selection; credentials are included only by explicit choice.
use super::*;
use anyhow::ensure;
use std::collections::BTreeSet;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Credentials {
    #[default]
    Redacted,
    Included,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Check {
    #[default]
    Unchecked,
    Checked,
    Partial,
}
#[derive(Default)]
pub struct Selection {
    chosen: BTreeSet<String>,
}
pub struct Row<'a> {
    pub connection: &'a Connection,
    pub depth: usize,
    pub end: usize,
    pub check: Check,
}
pub struct Tree<'a> {
    pub rows: Vec<Row<'a>>,
}
impl Selection {
    pub fn all(&mut self, connections: &[Connection], selected: bool) {
        self.chosen.clear();
        if selected {
            self.chosen.extend(connections.iter().map(|c| c.id.clone()));
        }
    }
    pub fn set(&mut self, tree: &Tree<'_>, index: usize, selected: bool) {
        let Some(row) = tree.rows.get(index) else {
            return;
        };
        for row in &tree.rows[index..row.end] {
            if selected {
                self.chosen.insert(row.connection.id.clone());
            } else {
                self.chosen.remove(&row.connection.id);
            }
        }
    }
    pub fn tree<'a>(&self, connections: &'a [Connection]) -> Tree<'a> {
        let traversal = crate::tree::ConnectionTree::new(connections).rows();
        let mut rows = Vec::<Row<'a>>::with_capacity(traversal.len());
        let mut parents = vec![None; traversal.len()];
        let mut stack = Vec::<usize>::new();
        for (index, (connection, depth)) in traversal.into_iter().enumerate() {
            while stack.last().is_some_and(|p| rows[*p].depth >= depth) {
                rows[stack.pop().unwrap()].end = index;
            }
            parents[index] = stack.last().copied();
            rows.push(Row {
                connection,
                depth,
                end: connections.len(),
                check: Check::Unchecked,
            });
            stack.push(index);
        }
        let len = rows.len();
        for p in stack {
            rows[p].end = len;
        }
        let mut children = vec![(0usize, 0usize, false); len];
        for index in (0..len).rev() {
            let (total, checked, any) = children[index];
            rows[index].check = if total == 0 {
                if self.chosen.contains(&rows[index].connection.id) {
                    Check::Checked
                } else {
                    Check::Unchecked
                }
            } else if total == checked {
                Check::Checked
            } else if any {
                Check::Partial
            } else {
                Check::Unchecked
            };
            if let Some(parent) = parents[index] {
                children[parent].0 += 1;
                children[parent].1 += usize::from(rows[index].check == Check::Checked);
                children[parent].2 |= rows[index].check != Check::Unchecked;
            }
        }
        Tree { rows }
    }
    pub fn prepare(
        &self,
        connections: &[Connection],
        credentials: Credentials,
    ) -> Result<Prepared> {
        validate(connections)?;
        let known: HashSet<_> = connections.iter().map(|c| c.id.as_str()).collect();
        ensure!(
            self.chosen.iter().all(|id| known.contains(id.as_str())),
            "连接列表已变化，请重新打开导出窗口"
        );
        let connections = self
            .tree(connections)
            .rows
            .into_iter()
            .filter(|r| r.check != Check::Unchecked)
            .map(|r| {
                if credentials == Credentials::Included {
                    r.connection.clone()
                } else {
                    r.connection.redacted()
                }
            })
            .collect::<Vec<_>>();
        ensure!(!connections.is_empty(), "请至少选择一个连接");
        Ok(Prepared {
            connections,
            credentials,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Prepared {
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub credentials: Credentials,
}
impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionExport")
            .field("items", &self.connections.len())
            .field("credentials", &self.credentials)
            .finish()
    }
}
impl Prepared {
    pub fn encode(&self) -> Result<String> {
        self.encode_checked(|| Ok(()))
    }
    pub fn encode_checked(&self, check: impl Fn() -> Result<()>) -> Result<String> {
        check()?;
        validate(&self.connections)?;
        #[derive(Serialize)]
        struct Envelope<'a> {
            format: &'static str,
            version: u32,
            connections: &'a [Connection],
        }
        // Apply the policy again at the persistence boundary, including deserialized requests.
        let redacted;
        let connections = if self.credentials == Credentials::Redacted {
            redacted = self
                .connections
                .iter()
                .map(Connection::redacted)
                .collect::<Vec<_>>();
            redacted.as_slice()
        } else {
            &self.connections
        };
        // Import accepts at most 32 MiB, including the AES/base64 envelope.
        let limit = (32 * 1024 * 1024 - crate::legacy::MARKER.len()) / 4 * 3 - 16;
        struct Bounded<'a, F> {
            bytes: Vec<u8>,
            limit: usize,
            check: &'a F,
        }
        impl<F: Fn() -> Result<()>> std::io::Write for Bounded<'_, F> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                (self.check)().map_err(std::io::Error::other)?;
                if self.bytes.len().saturating_add(bytes.len()) > self.limit {
                    return Err(std::io::Error::other("导出内容超过32 MiB"));
                }
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut json = Bounded {
            bytes: Vec::new(),
            limit,
            check: &check,
        };
        serde_json::to_writer_pretty(
            &mut json,
            &Envelope {
                format: "tomato-connections-export",
                version: 1,
                connections,
            },
        )?;
        check()?;
        Ok(format!(
            "{}{}",
            crate::legacy::MARKER,
            crate::legacy::encrypt_text(&json.bytes)
        ))
    }
}
