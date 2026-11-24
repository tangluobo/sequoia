use anyhow::Result;
use tomato_core::backup::{FileAction, FileOutput, FileRequest, Item, Scope};
use tomato_services::commands::{Command, CommandHandler, Engine, Output};
async fn call(
    engine: &mut Engine,
    root: &std::path::Path,
    scope: &Scope,
    action: FileAction,
) -> Result<FileOutput> {
    let Output::BackupFiles(output) = engine
        .execute(Command::BackupFiles {
            root: root.into(),
            request: FileRequest {
                scope: scope.clone(),
                action,
            },
        })
        .await?
    else {
        anyhow::bail!("backup filesystem reply required")
    };
    Ok(output)
}
#[tokio::test]
async fn legacy_backup_files_list_inspect_rename_nested_directory_and_delete_use_the_real_actor()
-> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("data");
    let scope = Scope {
        connection: "旧 备份连接".into(),
        database: "actual_db".into(),
        path: String::new(),
    };
    let mut engine = Engine::default();
    assert!(
        matches!(call(&mut engine,&root,&scope,FileAction::List).await?,FileOutput::Listed {items,..}if items.is_empty())
    );
    assert!(!root.exists());
    call(&mut engine, &root, &scope, FileAction::Directory).await?;
    let config = root.join("connections.json");
    std::fs::write(&config, b"old configuration sentinel")?;
    call(
        &mut engine,
        &root,
        &scope,
        FileAction::CreateDirectory {
            name: "子目录".into(),
        },
    )
    .await?;
    let file = root.join(scope.file("原备份")?);
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/backup/original.nb3"),
        &file,
    )?;
    std::fs::write(
        root.join(scope.directory()?).join("UPPER.NB3"),
        b"not listed",
    )?;
    let FileOutput::Listed { items, .. } =
        call(&mut engine, &root, &scope, FileAction::List).await?
    else {
        unreachable!()
    };
    assert_eq!(items.len(), 2);
    assert!(items[0].directory && items[0].name == "子目录");
    assert!(!items[1].directory && items[1].name == "原备份");
    assert!(
        matches!(call(&mut engine,&root,&scope,FileAction::Info {name:"原备份".into()}).await?,FileOutput::Info {metadata,bytes,..}if metadata.meta_version=="30100"&&metadata.objects.len()==1&&bytes>0)
    );
    call(
        &mut engine,
        &root,
        &scope,
        FileAction::Rename {
            item: Item {
                name: "原备份".into(),
                directory: false,
            },
            name: "新的 名".into(),
        },
    )
    .await?;
    assert!(!file.exists());
    assert!(root.join(scope.file("新的_名")?).exists());
    let child = scope.child("子目录")?;
    std::fs::copy(
        root.join(scope.file("新的_名")?),
        root.join(child.file("嵌套")?),
    )?;
    let FileOutput::Changed { errors, .. } = call(
        &mut engine,
        &root,
        &scope,
        FileAction::Delete {
            items: vec![
                Item {
                    name: "子目录".into(),
                    directory: true,
                },
                Item {
                    name: "新的_名".into(),
                    directory: false,
                },
            ],
        },
    )
    .await?
    else {
        unreachable!()
    };
    assert!(errors.is_empty());
    assert!(!root.join(child.directory()?).exists());
    assert!(!root.join(scope.file("新的_名")?).exists());
    assert_eq!(std::fs::read(&config)?, b"old configuration sentinel");
    Ok(())
}
