//! Background connection import: parse once, share the draft, commit with an
//! optimistic file revision and return only the newly added models to the UI.
use crate::backup::Control;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
use tomato_core::{
    Connection,
    config::{ImportBaseline, ImportPlan},
    repository::{ConnectionFileRepository, SaveGuard, SaveReceipt},
};
const LIMIT: usize = 32 * 1024 * 1024;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Request {
    Preview {
        source: PathBuf,
        baseline: ImportBaseline,
    },
    Commit {
        source: PathBuf,
        source_signature: [u8; 32],
        guard: SaveGuard,
        selected: BTreeSet<usize>,
        #[serde(skip)]
        prepared: Option<Arc<ImportPlan>>,
    },
}
impl Request {
    pub fn retained_bytes(&self) -> usize {
        match self {
            Self::Preview { baseline, .. } => baseline.retained_bytes(),
            Self::Commit {
                selected, prepared, ..
            } => selected.len() * 64 + prepared.as_ref().map_or(0, |p| p.retained_bytes()),
        }
    }
}
#[derive(Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Output {
    Preview {
        source: PathBuf,
        count: usize,
        duplicates: usize,
        #[serde(skip_serializing)]
        plan: Arc<ImportPlan>,
    },
    Committed {
        count: usize,
        #[serde(skip_serializing)]
        added: Vec<Connection>,
        #[serde(skip_serializing)]
        receipt: SaveReceipt,
    },
}
impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preview {
                count, duplicates, ..
            } => f
                .debug_struct("ConnectionImportPreview")
                .field("count", count)
                .field("duplicates", duplicates)
                .finish_non_exhaustive(),
            Self::Committed { count, .. } => f
                .debug_struct("ConnectionImportCommitted")
                .field("count", count)
                .finish_non_exhaustive(),
        }
    }
}
fn read_source(source: &Path, control: &Control) -> Result<String> {
    let mut file = std::fs::File::open(source).context("无法读取导入文件")?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        control.check()?;
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        ensure!(bytes.len() + count <= LIMIT, "导入文件超过32 MiB");
        bytes.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8(bytes).context("导入文件不是 UTF-8")
}
fn check_source(source: &Path, expected: &[u8; 32], control: &Control) -> Result<()> {
    let mut file = std::fs::File::open(source).context("无法重新读取导入文件")?;
    let mut hash = Sha256::new();
    let mut total = 0usize;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        control.check()?;
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count);
        ensure!(total <= LIMIT, "导入文件超过32 MiB");
        hash.update(&chunk[..count]);
    }
    ensure!(
        <[u8; 32]>::from(hash.finalize()) == *expected,
        "导入文件已变化，请重新选择文件；本次未修改配置"
    );
    Ok(())
}
pub fn execute(request: Request, control: Control) -> Result<Output> {
    control.check()?;
    match request {
        Request::Preview { source, baseline } => {
            let content = read_source(&source, &control)?;
            let plan = ImportPlan::parse_with(&content, baseline)?;
            control.check()?;
            ensure!(
                plan.retained_bytes() <= LIMIT,
                "导入项目占用超过32 MiB，请拆分文件后导入"
            );
            Ok(Output::Preview {
                count: plan.connections.len(),
                duplicates: (0..plan.connections.len())
                    .filter(|i| plan.duplicate(*i))
                    .count(),
                source,
                plan: Arc::new(plan),
            })
        }
        Request::Commit {
            source,
            source_signature,
            guard,
            selected,
            prepared,
        } => {
            ensure!(!selected.is_empty(), "请至少选择一个要导入的项目");
            check_source(&source, &source_signature, &control)?;
            let repository = ConnectionFileRepository::new(&guard.path);
            let mut store = repository.load_guarded(&guard)?;
            let plan = match prepared {
                Some(plan) => plan,
                None => Arc::new(ImportPlan::parse(&read_source(&source, &control)?, &store)?),
            };
            ensure!(
                plan.source_signature() == source_signature,
                "导入内容已变化，请重新选择文件"
            );
            let added = plan.prepare_checked(&store, &selected, || control.check())?;
            ensure!(
                added.iter().map(Connection::retained_bytes).sum::<usize>() <= LIMIT,
                "导入项目占用超过32 MiB，请减少选择项"
            );
            let original = store.connections.len();
            store.connections.extend(added);
            check_source(&source, &source_signature, &control)?;
            repository.save_checked(&store, || control.check())?;
            let added = store.connections.split_off(original);
            let receipt = repository.saved_receipt(guard)?;
            Ok(Output::Committed {
                count: added.len(),
                added,
                receipt,
            })
        }
    }
}
