use anyhow::{Context, Result, ensure};
use tomato_core::Connection;
use tomato_services::mongo::{self, Action, Output, Sessions, Target};
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Fixture {
    _child: Option<Child>,
    _root: tempfile::TempDir,
    connection: Connection,
}
impl Fixture {
    async fn start() -> Result<Self> {
        if let Ok(uri) = std::env::var("TOMATO_TEST_MONGO_URI") {
            let uri = url::Url::parse(&uri)?;
            let host = uri.host_str().context("fixture host required")?;
            ensure!(
                matches!(host, "127.0.0.1" | "localhost" | "::1"),
                "Only isolated loopback MongoDB is allowed"
            );
            let mut connection = Connection::new("MONGODB");
            connection.set("host", host);
            connection.set("port", uri.port().unwrap_or(27017));
            connection.set("database", "admin");
            connection.set(
                "username",
                percent_encoding::percent_decode_str(uri.username())
                    .decode_utf8()?
                    .into_owned(),
            );
            connection.set(
                "password",
                percent_encoding::percent_decode_str(uri.password().unwrap_or_default())
                    .decode_utf8()?
                    .into_owned(),
            );
            return Ok(Self {
                _child: None,
                _root: tempfile::tempdir()?,
                connection,
            });
        }
        let executable =
            std::env::var_os("TOMATO_TEST_MONGOD").expect("TOMATO_TEST_MONGOD required");
        let root = tempfile::tempdir()?;
        let db = root.path().join("db");
        std::fs::create_dir(&db)?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let mut cmd = std::process::Command::new(executable);
        cmd.current_dir(root.path())
            .args([
                "--bind_ip",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--auth",
                "--wiredTigerCacheSizeGB",
                "0.25",
                "--setParameter",
                "diagnosticDataCollectionEnabled=false",
                "--dbpath",
            ])
            .arg(&db)
            .arg("--logpath")
            .arg(root.path().join("mongod.log"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let mut child = Child(cmd.spawn()?);
        let mut ready = false;
        for _ in 0..200 {
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                ready = true;
                break;
            }
            if let Some(status) = child.0.try_wait()? {
                anyhow::bail!(
                    "isolated MongoDB exited {status}: {}",
                    std::fs::read_to_string(root.path().join("mongod.log")).unwrap_or_default()
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        ensure!(ready, "MongoDB loopback startup timeout");
        let mut connection = Connection::new("MONGODB");
        connection.set("host", "127.0.0.1");
        connection.set("port", port);
        connection.set("database", "admin");
        let mut bootstrap = Sessions::default();
        bootstrap.execute(&connection,Action::Command {database:"admin".into(),json:r#"{"createUser":"fixture","pwd":"fixture-only","roles":[{"role":"root","db":"admin"}]}"#.into()}).await?;
        bootstrap.close(&connection.id).await;
        connection.set("username", "fixture");
        connection.set("password", "fixture-only");
        Ok(Self {
            _child: Some(child),
            _root: root,
            connection,
        })
    }
}
fn target(connection: &Connection, name: &str) -> Target {
    Target {
        database: format!("tomato_fixture_{}", connection.id.replace('-', "")),
        collection: name.into(),
    }
}
async fn find(s: &mut Sessions, c: &Connection, t: &Target) -> Result<Vec<String>> {
    match s
        .execute(
            c,
            Action::Find {
                target: t.clone(),
                filter: "{}".into(),
                skip: 0,
                limit: 100,
            },
        )
        .await?
    {
        Output::Documents { documents, .. } => Ok(documents),
        _ => unreachable!(),
    }
}
#[test]
fn extended_json_preserves_id_and_replacement_rejects_identity_changes() -> Result<()> {
    let original = r#"{"_id":{"$oid":"507f1f77bcf86cd799439011"},"n":{"$numberLong":"9007199254740993"},"date":{"$date":"2024-01-01T00:00:00Z"}}"#;
    let parsed = mongo::parse(original)?;
    let pretty = mongo::pretty(parsed.clone())?;
    assert_eq!(mongo::parse(&pretty)?, parsed);
    let (id, replaced) = mongo::replacement(original, r#"{"value":"中文"}"#)?;
    assert_eq!(replaced.get("_id"), Some(&id));
    assert!(
        mongo::replacement(original, r#"{"_id":{"$oid":"507f1f77bcf86cd799439012"}}"#).is_err()
    );
    assert!(mongo::replacement("{}", "{}").is_err());
    assert!(mongo::parse("[]").is_err());
    let (_, null) = mongo::replacement(original, r#"{"_id":null}"#)?;
    assert_eq!(null.get("_id"), Some(&id));
    Ok(())
}
#[tokio::test]
#[ignore = "Set TOMATO_TEST_MONGOD; starts its own authenticated loopback MongoDB, never uses saved connections"]
async fn real_mongo_documents_indexes_validator_uri_and_close() -> Result<()> {
    let fixture = Fixture::start().await?;
    let c = &fixture.connection;
    let mut sessions = Sessions::default();
    let t = target(c, "文档");
    sessions
        .execute(c, Action::CreateCollection { target: t.clone() })
        .await?;
    let Output::Databases(names) = sessions.execute(c, Action::Databases).await? else {
        unreachable!()
    };
    assert!(names.contains(&t.database));
    let json = r#"{"_id":{"$oid":"507f1f77bcf86cd799439011"},"中文":"value with spaces","age":12,"n":{"$numberLong":"9007199254740993"}}"#;
    let Output::Document { json: inserted, .. } = sessions
        .execute(
            c,
            Action::Insert {
                target: t.clone(),
                json: json.into(),
            },
        )
        .await?
    else {
        unreachable!()
    };
    let documents = find(&mut sessions, c, &t).await?;
    assert_eq!(documents.len(), 1);
    assert_eq!(mongo::parse(&documents[0])?, mongo::parse(&inserted)?);
    assert!(
        sessions
            .execute(
                c,
                Action::Insert {
                    target: t.clone(),
                    json: json.into()
                }
            )
            .await
            .is_err()
    );
    assert!(
        sessions
            .execute(
                c,
                Action::Replace {
                    target: t.clone(),
                    original: inserted.clone(),
                    json: r#"{"_id":3}"#.into()
                }
            )
            .await
            .is_err()
    );
    let Output::Document { json: saved, .. } = sessions
        .execute(
            c,
            Action::Replace {
                target: t.clone(),
                original: inserted,
                json: r#"{"中文":"updated","age":12}"#.into(),
            },
        )
        .await?
    else {
        unreachable!()
    };
    assert_eq!(mongo::parse(&saved)?.get_str("中文")?, "updated");
    let Output::Documents { documents, .. } = sessions
        .execute(
            c,
            Action::Find {
                target: t.clone(),
                filter: r#"{"age":{"$gte":10}}"#.into(),
                skip: 0,
                limit: 1,
            },
        )
        .await?
    else {
        unreachable!()
    };
    assert_eq!(documents.len(), 1);
    let Output::Documents { documents, .. } = sessions
        .execute(
            c,
            Action::Find {
                target: t.clone(),
                filter: "{}".into(),
                skip: 1,
                limit: 1,
            },
        )
        .await?
    else {
        unreachable!()
    };
    assert!(documents.is_empty());
    sessions
        .execute(
            c,
            Action::CreateIndex {
                target: t.clone(),
                keys: r#"{"age":1}"#.into(),
                name: "age_unique".into(),
                unique: true,
            },
        )
        .await?;
    let Output::Indexes { indexes, .. } = sessions
        .execute(c, Action::Indexes { target: t.clone() })
        .await?
    else {
        unreachable!()
    };
    assert!(
        indexes
            .iter()
            .any(|j| mongo::parse(j).unwrap().get_str("name").ok() == Some("age_unique"))
    );
    let Output::Text(raw) = sessions
        .execute(
            c,
            Action::Command {
                database: t.database.clone(),
                json: serde_json::json!({"listIndexes":t.collection,"cursor":{"batchSize":50}})
                    .to_string(),
            },
        )
        .await?
    else {
        unreachable!()
    };
    let raw = mongo::parse(&raw)?;
    let server = raw
        .get_document("cursor")?
        .get_array("firstBatch")?
        .iter()
        .filter_map(bson::Bson::as_document)
        .find(|d| d.get_str("name").ok() == Some("age_unique"))
        .unwrap();
    let received = indexes
        .iter()
        .map(|s| mongo::parse(s).unwrap())
        .find(|d| d.get_str("name").ok() == Some("age_unique"))
        .unwrap();
    assert_eq!(
        received.keys().collect::<Vec<_>>(),
        server.keys().collect::<Vec<_>>(),
        "index editor must retain the original wire document order and fields"
    );
    assert_eq!(&received, server);
    assert!(
        sessions
            .execute(
                c,
                Action::Insert {
                    target: t.clone(),
                    json: r#"{"age":12}"#.into()
                }
            )
            .await
            .is_err()
    );
    assert!(
        sessions
            .execute(
                c,
                Action::DropIndex {
                    target: t.clone(),
                    name: "_id_".into()
                }
            )
            .await
            .is_err()
    );
    sessions
        .execute(
            c,
            Action::DropIndex {
                target: t.clone(),
                name: "age_unique".into(),
            },
        )
        .await?;
    let validator = r#"{"$jsonSchema":{"bsonType":"object","required":["age"],"properties":{"age":{"bsonType":"int"}}}}"#;
    sessions
        .execute(
            c,
            Action::UpdateValidator {
                target: t.clone(),
                json: validator.into(),
            },
        )
        .await?;
    let Output::Validator { json: actual, .. } = sessions
        .execute(c, Action::Validator { target: t.clone() })
        .await?
    else {
        unreachable!()
    };
    assert_eq!(mongo::parse(&actual)?, mongo::parse(validator)?);
    assert!(
        sessions
            .execute(
                c,
                Action::Insert {
                    target: t.clone(),
                    json: r#"{"age":"invalid"}"#.into()
                }
            )
            .await
            .is_err()
    );
    let Output::Text(stats) = sessions
        .execute(
            c,
            Action::Command {
                database: t.database.clone(),
                json: r#"{"dbStats":1}"#.into(),
            },
        )
        .await?
    else {
        unreachable!()
    };
    assert_eq!(mongo::parse(&stats)?.get_str("db")?, t.database);
    sessions
        .execute(
            c,
            Action::RenameCollection {
                target: t.clone(),
                name: "renamed".into(),
            },
        )
        .await?;
    let renamed = target(c, "renamed");
    let Output::Collections { names, .. } = sessions
        .execute(
            c,
            Action::Collections {
                database: t.database.clone(),
            },
        )
        .await?
    else {
        unreachable!()
    };
    assert!(names.contains(&"renamed".into()));
    assert!(!names.contains(&t.collection));
    sessions
        .execute(
            c,
            Action::Delete {
                target: renamed.clone(),
                json: saved.clone(),
            },
        )
        .await?;
    assert!(find(&mut sessions, c, &renamed).await?.is_empty());
    let Output::Changed { kind, .. } = sessions
        .execute(
            c,
            Action::Delete {
                target: renamed.clone(),
                json: saved,
            },
        )
        .await?
    else {
        unreachable!()
    };
    assert_eq!(kind, "documents_missing");
    sessions
        .execute(c, Action::DropCollection { target: renamed })
        .await?;
    let mut uri = Connection::new("MONGODB");
    uri.set(
        "host",
        format!(
            "mongodb://fixture:fixture-only@127.0.0.1:{}/?authSource=admin",
            c.number("port")
        ),
    );
    sessions.execute(&uri, Action::Databases).await?;
    sessions.close(&uri.id).await;
    sessions.close(&c.id).await;
    let mut wrong = c.clone();
    wrong.set("password", "wrong-fixture-password");
    assert!(sessions.execute(&wrong, Action::Databases).await.is_err());
    sessions.execute(c, Action::Databases).await?;
    sessions.close(&c.id).await;
    Ok(())
}

#[tokio::test]
#[ignore = "Set TOMATO_TEST_MONGOD; verifies bounded page retention using two complete large documents"]
async fn real_mongo_page_retention_keeps_complete_documents_and_marks_truncation() -> Result<()> {
    let fixture = Fixture::start().await?;
    let c = &fixture.connection;
    let mut sessions = Sessions::default();
    let t = target(c, "bounded");
    for index in 0..2 {
        let json = serde_json::json!({"index":index,"payload":"x".repeat(4600*1024)}).to_string();
        sessions
            .execute(
                c,
                Action::Insert {
                    target: t.clone(),
                    json,
                },
            )
            .await?;
    }
    let Output::Documents {
        documents,
        truncated,
        ..
    } = sessions
        .execute(
            c,
            Action::Find {
                target: t.clone(),
                filter: "{}".into(),
                skip: 0,
                limit: 100,
            },
        )
        .await?
    else {
        unreachable!()
    };
    assert!(truncated);
    assert_eq!(documents.len(), 1);
    assert_eq!(
        mongo::parse(&documents[0])?.get_str("payload")?.len(),
        4600 * 1024
    );
    assert!(documents.iter().map(String::len).sum::<usize>() <= 8 * 1024 * 1024);
    sessions.close(&c.id).await;
    Ok(())
}
