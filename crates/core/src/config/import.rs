//! Java ConnectionImportPlanner: independently selected duplicates and reference remapping.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
mod tree;
pub use tree::Row;
mod baseline;
pub use baseline::Baseline;
#[cfg(test)]
mod parity_tests;

#[derive(Clone)]
enum Target {
    Existing(String),
    Incoming(usize),
}
pub struct ImportPlan {
    pub connections: Vec<Connection>,
    duplicates: Vec<Option<Target>>,
    rows: Vec<Row>,
    existing_signature: [u8; 32],
    input_signature: [u8; 32],
    source_signature: [u8; 32],
}
impl ImportPlan {
    pub fn parse(content: &str, existing: &Store) -> Result<Self> {
        Self::parse_with(content, Baseline::new(existing)?)
    }
    pub fn parse_with(content: &str, baseline: Baseline) -> Result<Self> {
        anyhow::ensure!(content.len() <= 32 * 1024 * 1024, "导入文件超过32 MiB");
        let connections = crate::legacy::decode_import(content)?;
        anyhow::ensure!(connections.len() <= 50_000, "导入项目超过50,000项");
        let rows = tree::rows(&connections)?;
        let mut seen = HashMap::new();
        for (id, key) in baseline.keys {
            seen.entry(key).or_insert(Target::Existing(id));
        }
        let duplicates = connections
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let key: [u8; 32] = Sha256::digest(duplicate_key(c).as_bytes()).into();
                let duplicate = seen.get(&key).cloned();
                seen.entry(key).or_insert(Target::Incoming(i));
                duplicate
            })
            .collect();
        let input_signature = signature(&connections)?;
        Ok(Self {
            connections,
            duplicates,
            rows,
            existing_signature: baseline.signature,
            input_signature,
            source_signature: Sha256::digest(content.as_bytes()).into(),
        })
    }
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn source_signature(&self) -> [u8; 32] {
        self.source_signature
    }
    pub fn retained_bytes(&self) -> usize {
        self.connections
            .iter()
            .fold(
                self.rows.capacity() * std::mem::size_of::<Row>()
                    + self.duplicates.capacity() * std::mem::size_of::<Option<Target>>(),
                |sum, c| sum.saturating_add(c.retained_bytes()),
            )
            .saturating_add(
                self.duplicates
                    .iter()
                    .map(|target| match target {
                        Some(Target::Existing(id)) => id.capacity(),
                        _ => 0,
                    })
                    .sum::<usize>(),
            )
    }
    pub fn matches_existing(&self, store: &Store) -> Result<()> {
        anyhow::ensure!(
            signature(&store.connections)? == self.existing_signature,
            "当前连接列表已变化，请重新选择导入文件；本次未修改配置"
        );
        Ok(())
    }
    pub fn duplicate(&self, index: usize) -> bool {
        self.duplicates.get(index).is_some_and(Option::is_some)
    }
    pub fn apply(&self, store: &mut Store, selected: &BTreeSet<usize>) -> Result<usize> {
        let added = self.prepare(store, selected)?;
        let count = added.len();
        store.connections.extend(added);
        Ok(count)
    }
    pub fn prepare(&self, store: &Store, selected: &BTreeSet<usize>) -> Result<Vec<Connection>> {
        self.prepare_checked(store, selected, || Ok(()))
    }
    pub fn prepare_checked(
        &self,
        store: &Store,
        selected: &BTreeSet<usize>,
        check: impl Fn() -> Result<()>,
    ) -> Result<Vec<Connection>> {
        check()?;
        self.matches_existing(store)?;
        anyhow::ensure!(
            signature(&self.connections)? == self.input_signature,
            "导入内容已变化，请重新选择导入文件；本次未修改配置"
        );
        if selected.iter().any(|i| *i >= self.connections.len()) {
            bail!("导入选择已失效");
        }
        let ids: Vec<_> = (0..self.connections.len())
            .map(|i| selected.contains(&i).then(new_id))
            .collect();
        let by_id = tree::first_ids(&self.connections);
        let existing: HashSet<_> = store.connections.iter().map(|c| c.id.as_str()).collect();
        let resolve = |old: &str| -> Option<String> {
            if old.trim().is_empty() {
                return None;
            }
            let Some(&mut_index) = by_id.get(old) else {
                return existing.contains(old).then(|| old.to_owned());
            };
            let mut index = mut_index;
            let mut visited = HashSet::new();
            loop {
                if let Some(id) = &ids[index] {
                    return Some(id.clone());
                }
                if !visited.insert(index) {
                    return None;
                }
                match self.duplicates[index].as_ref()? {
                    Target::Existing(id) => {
                        return existing.contains(id.as_str()).then(|| id.clone());
                    }
                    Target::Incoming(i) => index = *i,
                }
            }
        };
        let mut names: HashSet<_> = store.connections.iter().map(|c| c.name.clone()).collect();
        // Names only accumulate. Retaining the next suffix per base preserves
        // Java's first-free-name rule without rescanning 1..N for every copy.
        let mut suffixes = HashMap::<&str, usize>::new();
        let mut added = Vec::with_capacity(selected.len());
        for (offset, (i, source)) in self
            .connections
            .iter()
            .enumerate()
            .filter(|(i, _)| selected.contains(i))
            .enumerate()
        {
            if offset % 256 == 0 {
                check()?;
            }
            let mut copy = source.clone();
            copy.id = ids[i].clone().expect("selected item has an ID");
            copy.parent_id = source.parent_id.as_deref().and_then(resolve);
            copy.set(
                "sshTunnelHostId",
                resolve(source.text("sshTunnelHostId"))
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            let base = if source.name.trim().is_empty() {
                "未命名连接"
            } else {
                &source.name
            };
            copy.name = base.to_owned();
            if names.contains(base) {
                let suffix = suffixes.entry(base).or_insert(1);
                loop {
                    copy.name = format!("{base} ({suffix})");
                    *suffix += 1;
                    if !names.contains(&copy.name) {
                        break;
                    }
                }
            }
            names.insert(copy.name.clone());
            added.push(copy);
        }
        super::validate_refs(store.connections.iter().chain(&added), false)?;
        check()?;
        Ok(added)
    }
}

impl std::fmt::Debug for ImportPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportPlan")
            .field("connections", &self.connections.len())
            .finish_non_exhaustive()
    }
}
impl serde::Serialize for ImportPlan {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ImportPlan", 2)?;
        state.serialize_field("connections", &self.connections.len())?;
        state.serialize_field(
            "duplicates",
            &self.duplicates.iter().filter(|d| d.is_some()).count(),
        )?;
        state.end()
    }
}

fn signature(connections: &[Connection]) -> Result<[u8; 32]> {
    struct Writer(Sha256);
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer(Sha256::new());
    serde_json::to_writer(&mut writer, connections)?;
    Ok(writer.0.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_outside_the_import_file_resolve_existing_nodes_and_clear_missing_nodes()
    -> Result<()> {
        let mut store = Store {
            connections: parse_legacy(
                r#"[{"id":"group","name":"分组"},{"id":"jump","name":"跳板","type":"SSH"}]"#,
            )?,
            ..Default::default()
        };
        let input = r#"[{"id":"db","name":"数据库","type":"MYSQL","parentId":"group","sshTunnelHostId":"jump"}]"#;
        let plan = ImportPlan::parse(input, &store)?;
        plan.apply(&mut store, &BTreeSet::from([0]))?;
        assert_eq!(store.connections[2].parent_id.as_deref(), Some("group"));
        assert_eq!(store.connections[2].text("sshTunnelHostId"), "jump");
        let mut empty = Store::default();
        ImportPlan::parse(input, &empty)?.apply(&mut empty, &BTreeSet::from([0]))?;
        assert!(empty.connections[0].parent_id.is_none());
        assert!(empty.connections[0].text("sshTunnelHostId").is_empty());
        assert!(
            ImportPlan::parse(
                r#"[{"id":"a","parentId":"b"},{"id":"b","parentId":"a"}]"#,
                &empty
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn duplicate_comparison_matches_gson_defaults_and_sorted_nested_fields_without_rewriting_options()
    -> Result<()> {
        let store = Store {
            connections: parse_legacy(
                r#"[{"id":"old","name":"主机","type":"SSH","port":0,"screenWidth":1024,"usePassword":true,"directoryType":"LOCAL","future":{"b":2,"a":1}}]"#,
            )?,
            ..Default::default()
        };
        let input = r#"[{"id":"minimal","name":"主机","type":"SSH","future":{"a":1,"b":2}}]"#;
        let plan = ImportPlan::parse(input, &store)?;
        assert!(plan.duplicate(0));
        assert!(!plan.connections[0].options.contains_key("screenWidth"));
        assert!(
            !ImportPlan::parse(
                &input.replace("\"type\":\"SSH\"", "\"type\":\"SSH\",\"usePassword\":false"),
                &store
            )?
            .duplicate(0)
        );
        Ok(())
    }
    #[test]
    fn manual_duplicates_are_copied_and_unselected_references_follow_existing_targets() -> Result<()>
    {
        let mut store = Store::default();
        store.import(r#"[{"id":"old","name":"组"},{"id":"jump","name":"跳板","type":"SSH"}]"#)?;
        let folder = store.connections[0].id.clone();
        let jump = store.connections[1].id.clone();
        let text = r#"[{"id":"g","name":"组"},{"id":"s","name":"跳板","type":"SSH"},{"id":"db","name":"数据库","type":"MYSQL","parentId":"g","sshTunnelHostId":"s","future":{"value":42}}]"#;
        let plan = ImportPlan::parse(text, &store)?;
        assert!(plan.duplicate(0) && plan.duplicate(1) && !plan.duplicate(2));
        assert_eq!(plan.apply(&mut store, &BTreeSet::from([2]))?, 1);
        assert_eq!(
            store.connections[2].parent_id.as_deref(),
            Some(folder.as_str())
        );
        assert_eq!(store.connections[2].text("sshTunnelHostId"), jump);
        let plan = ImportPlan::parse(text, &store)?;
        plan.apply(&mut store, &BTreeSet::from([0, 1, 2]))?;
        assert_eq!(
            store.connections[5].parent_id.as_deref(),
            Some(store.connections[3].id.as_str())
        );
        assert_eq!(
            store.connections[5].text("sshTunnelHostId"),
            store.connections[4].id
        );
        assert_eq!(store.connections[5].name, "数据库 (1)");
        assert_eq!(store.connections[5].options["future"]["value"], 42);
        let plan = ImportPlan::parse(text, &Store::default())?;
        let mut isolated = Store::default();
        plan.apply(&mut isolated, &BTreeSet::from([2]))?;
        assert!(isolated.connections[0].parent_id.is_none());
        assert!(isolated.connections[0].text("sshTunnelHostId").is_empty());
        Ok(())
    }
}
