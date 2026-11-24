use super::*;
use tomato_core::{Store, repository::WorkspaceRepository};

struct Fixture {
    _dir: tempfile::TempDir,
    source: PathBuf,
    repository: ConnectionFileRepository,
    original: Vec<u8>,
}
fn fixture() -> Result<Fixture> {
    let dir = tempfile::tempdir()?;
    let repository = ConnectionFileRepository::new(dir.path().join("connections.json"));
    let mut store = repository.load()?;
    store.import(r#"[{"id":"group","name":"group"},{"id":"jump","name":"ssh","type":"SSH","password":"saved-fixture-secret"}]"#)?;
    repository.save(&store)?;
    let source = dir.path().join("import.json");
    std::fs::write(
        &source,
        tomato_core::legacy::encode(&tomato_core::legacy::decode(
            r#"[{"id":"g","name":"group"},{"id":"s","name":"ssh","type":"SSH","password":"import-fixture-secret"},{"id":"db","name":"db","type":"MYSQL","parentId":"g","sshTunnelHostId":"s","future":{"keep":42}}]"#,
        )?)?,
    )?;
    let original = std::fs::read(repository.path())?;
    Ok(Fixture {
        _dir: dir,
        source,
        repository,
        original,
    })
}
fn preview(f: &Fixture) -> Result<Arc<ImportPlan>> {
    let output = execute(
        Request::Preview {
            source: f.source.clone(),
            baseline: ImportBaseline::new(&f.repository.load()?)?,
        },
        Control::default(),
    )?;
    let text = format!("{output:?} {}", serde_json::to_string(&output)?);
    assert!(!text.contains("fixture-secret"));
    match output {
        Output::Preview {
            count,
            duplicates,
            plan,
            ..
        } => {
            assert_eq!((count, duplicates), (3, 2));
            Ok(plan)
        }
        _ => unreachable!(),
    }
}
fn request(f: &Fixture, plan: Arc<ImportPlan>) -> Result<Request> {
    Ok(Request::Commit {
        source: f.source.clone(),
        source_signature: plan.source_signature(),
        guard: f.repository.guard()?,
        selected: BTreeSet::from([2]),
        prepared: Some(plan),
    })
}

#[test]
fn encrypted_import_commits_backup_and_adopts_revision_without_exposing_credentials() -> Result<()>
{
    let f = fixture()?;
    let input = std::fs::read(&f.source)?;
    let plan = preview(&f)?;
    let store = f.repository.load()?;
    let output = execute(request(&f, plan)?, Control::default())?;
    assert!(!format!("{output:?} {}", serde_json::to_string(&output)?).contains("fixture-secret"));
    let Output::Committed {
        count,
        added,
        receipt,
    } = output
    else {
        unreachable!()
    };
    assert_eq!(count, 1);
    assert_eq!(
        added[0].parent_id.as_deref(),
        Some(store.connections[0].id.as_str())
    );
    assert_eq!(added[0].text("sshTunnelHostId"), store.connections[1].id);
    assert_eq!(added[0].options["future"]["keep"], 42);
    f.repository.accept_saved(receipt)?;
    let mut next = store;
    next.connections.extend(added);
    f.repository.save(&next)?;
    assert_eq!(std::fs::read(f.repository.backup_path())?, f.original);
    assert_eq!(std::fs::read(&f.source)?, input);
    assert_eq!(f.repository.load()?.connections.len(), 3);
    assert!(!std::fs::read_to_string(f.repository.path())?.contains("fixture-secret"));
    Ok(())
}

#[test]
fn source_change_configuration_change_and_cancellation_never_overwrite() -> Result<()> {
    for change in 0..3 {
        let f = fixture()?;
        let plan = preview(&f)?;
        let request = request(&f, plan)?;
        let control = Control::default();
        match change {
            0 => {
                std::fs::write(&f.source, "[]")?;
            }
            1 => {
                std::fs::write(f.repository.path(), tomato_core::legacy::encode(&[])?)?;
            }
            _ => control.cancel(),
        }
        let before = std::fs::read(f.repository.path())?;
        assert!(execute(request, control).is_err());
        assert_eq!(std::fs::read(f.repository.path())?, before);
    }
    Ok(())
}

#[test]
fn serialized_cli_commit_reconstructs_plan_and_manual_duplicates_keep_the_selected_password()
-> Result<()> {
    let f = fixture()?;
    let plan = preview(&f)?;
    let mut request = request(&f, plan)?;
    if let Request::Commit { selected, .. } = &mut request {
        *selected = BTreeSet::from([0, 1, 2]);
    }
    let json = serde_json::to_string(&request)?;
    assert!(!json.contains("fixture-secret"));
    let request: Request = serde_json::from_str(&json)?;
    let Output::Committed { added, .. } = execute(request, Control::default())? else {
        unreachable!()
    };
    assert_eq!(added.len(), 3);
    assert_eq!(added[1].name, "ssh (1)");
    assert_eq!(added[1].text("password"), "import-fixture-secret");
    assert_eq!(added[2].text("sshTunnelHostId"), added[1].id);
    assert_eq!(Store::load(f.repository.path())?.connections.len(), 5);
    Ok(())
}
