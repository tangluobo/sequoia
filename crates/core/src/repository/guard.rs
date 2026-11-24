use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveGuard {
    pub path: PathBuf,
    revision: Option<[u8; 32]>,
}
#[derive(Debug)]
pub struct SaveReceipt {
    pub(super) base: SaveGuard,
    pub(super) revision: Option<[u8; 32]>,
}
pub(super) fn fingerprint(bytes: Option<&[u8]>) -> Option<[u8; 32]> {
    bytes.map(|bytes| Sha256::digest(bytes).into())
}
impl ConnectionFileRepository {
    pub fn guard(&self) -> Result<SaveGuard> {
        match *self.revision.borrow() {
            Revision::Loaded(revision) => Ok(SaveGuard {
                path: self.path.clone(),
                revision,
            }),
            Revision::Unloaded => bail!("请先加载连接配置再保存"),
        }
    }
    pub fn load_guarded(&self, guard: &SaveGuard) -> Result<Store> {
        let current = self.read()?;
        anyhow::ensure!(
            guard.path == self.path && fingerprint(current.as_deref()) == guard.revision,
            "连接配置已被其他程序修改，请重新打开应用后再编辑；本次未覆盖外部修改"
        );
        let store = current
            .as_deref()
            .map(Store::decode)
            .transpose()?
            .unwrap_or(Store {
                version: 1,
                ..Default::default()
            });
        *self.revision.borrow_mut() = Revision::Loaded(guard.revision);
        Ok(store)
    }
    pub fn saved_receipt(&self, base: SaveGuard) -> Result<SaveReceipt> {
        let current = self.guard()?;
        anyhow::ensure!(base.path == current.path, "保存目标不一致");
        Ok(SaveReceipt {
            base,
            revision: current.revision,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn fixture() -> Result<(tempfile::TempDir, ConnectionFileRepository, Store, Vec<u8>)> {
        let dir = tempfile::tempdir()?;
        let repo = ConnectionFileRepository::new(dir.path().join("connections.json"));
        let mut store = repo.load()?;
        store.import(r#"[{"id":"a","name":"first","type":"SSH","password":"fixture-secret"}]"#)?;
        repo.save(&store)?;
        let original = std::fs::read(repo.path())?;
        store.connections[0].name = "changed".into();
        Ok((dir, repo, store, original))
    }
    #[test]
    fn cancellation_and_external_change_during_staging_preserve_the_current_file() -> Result<()> {
        let (_dir, repo, store, original) = fixture()?;
        let checks = Cell::new(0);
        let result = repo.save_checked(&store, || {
            checks.set(checks.get() + 1);
            anyhow::ensure!(checks.get() < 4, "cancelled");
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(repo.path())?, original);
        assert_eq!(std::fs::read(repo.backup_path())?, original);
        let external = crate::legacy::encode(&[])?;
        checks.set(0);
        let result = repo.save_checked(&store, || {
            checks.set(checks.get() + 1);
            if checks.get() == 4 {
                std::fs::write(repo.path(), &external)?;
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(repo.path())?, external.as_bytes());
        assert_eq!(std::fs::read(repo.backup_path())?, original);
        Ok(())
    }
    #[test]
    fn background_save_receipt_advances_only_its_matching_repository_revision() -> Result<()> {
        let (_dir, repo, mut store, _) = fixture()?;
        let guard = repo.guard()?;
        let background = ConnectionFileRepository::new(repo.path());
        background.load_guarded(&guard)?;
        background.save(&store)?;
        repo.accept_saved(background.saved_receipt(guard.clone())?)?;
        store.connections[0].name = "next".into();
        repo.save(&store)?;
        assert!(repo.accept_saved(background.saved_receipt(guard)?).is_err());
        assert_eq!(repo.load()?.connections[0].name, "next");
        Ok(())
    }
}
