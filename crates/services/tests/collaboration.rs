use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use tomato_services::collaboration::{self as cloud, Api, Kind, Mutation, Object, Repository};

fn object(revision: i64, text: &str) -> Object {
    Object {
        id: "object".into(),
        project_id: "project".into(),
        parent_id: String::new(),
        kind: Kind::Query,
        name: "查询🙂".into(),
        body: json!({"text":text}),
        bytes: text.len(),
        revision,
        deleted: false,
        modified: 0,
        modified_by: "user".into(),
        mutation_id: format!("mutation{revision}"),
    }
}
#[tokio::test]
async fn durable_outbox_preserves_conflicting_documents_and_stale_editors() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cache.sqlite");
    {
        let repo = Repository::open(&path).await?;
        repo.receive(object(1, "base")).await?;
        let mut m = Mutation::from_object(&object(1, "local"));
        m.mutation_id = "local".into();
        repo.enqueue("project", "object", m).await?;
        repo.receive(object(2, "remote")).await?;
        assert_eq!(repo.object("object").await?.body["text"], "local");
        assert_eq!(repo.remote("object").await?.unwrap().body["text"], "remote");
        assert!(repo.pending().await?.is_empty());
    }
    let repo = Repository::open(&path).await?;
    assert!(repo.list("project").await?[0].conflict);
    repo.resolve("object", true).await?;
    assert_eq!(repo.pending().await?[0].2.base_revision, 2);
    assert_eq!(repo.pending().await?[0].2.body["text"], "local");
    repo.receive(object(3, "third")).await?;
    repo.resolve("object", false).await?;
    assert_eq!(repo.object("object").await?.body["text"], "third");
    assert!(repo.pending().await?.is_empty());
    let stale = Mutation::from_object(&object(1, "stale-editor"));
    repo.enqueue("project", "object", stale).await?;
    assert!(repo.list("project").await?[0].conflict);
    assert_eq!(repo.remote("object").await?.unwrap().revision, 3);
    assert_eq!(repo.object("object").await?.body["text"], "stale-editor");
    Ok(())
}
#[tokio::test]
async fn unchanged_stale_save_uses_current_revision_but_preserves_metadata_conflicts() -> Result<()>
{
    let dir = tempfile::tempdir()?;
    let repo = Repository::open(&dir.path().join("cache.sqlite")).await?;
    let current = object(3, "same SQL");
    repo.receive(current.clone()).await?;
    let mut repeated = Mutation::from_object(&object(1, "same SQL"));
    repeated.mutation_id = "repeat-save".into();
    repo.enqueue("project", "object", repeated.clone()).await?;
    assert!(!repo.list("project").await?[0].conflict);
    assert_eq!(repo.pending().await?[0].2.base_revision, 3);
    let mut acknowledged = object(4, "same SQL");
    acknowledged.mutation_id = repeated.mutation_id;
    repo.receive(acknowledged.clone()).await?;
    assert!(!repo.list("project").await?[0].dirty);

    for changed in ["name", "parent", "deleted", "kind", "body"] {
        let mut stale = Mutation::from_object(&current);
        match changed {
            "name" => stale.name = "older name".into(),
            "parent" => stale.parent_id = "older parent".into(),
            "deleted" => stale.deleted = true,
            "kind" => stale.kind = Kind::Snippet,
            "body" => stale.body = json!({"text":"older SQL"}),
            _ => unreachable!(),
        }
        repo.enqueue("project", "object", stale).await?;
        assert!(repo.list("project").await?[0].conflict, "{changed}");
        assert!(repo.pending().await?.is_empty(), "{changed}");
        assert_eq!(repo.remote("object").await?.unwrap().revision, 4);
        repo.resolve("object", false).await?;
        assert_eq!(repo.object("object").await?.body, acknowledged.body);
    }
    Ok(())
}
#[tokio::test]
async fn acknowledge_is_idempotent_and_does_not_erase_newer_local_edits() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::open(&dir.path().join("cache.sqlite")).await?;
    let old = object(1, "base");
    repo.receive(old.clone()).await?;
    let first = Mutation::from_object(&object(1, "one"));
    repo.enqueue("project", "object", first.clone()).await?;
    let mut accepted = object(2, "one");
    accepted.mutation_id = first.mutation_id.clone();
    repo.receive(accepted).await?;
    assert!(!repo.list("project").await?[0].dirty);
    let second = Mutation::from_object(&object(2, "two"));
    repo.enqueue("project", "object", second).await?;
    repo.receive(old).await?;
    assert_eq!(repo.object("object").await?.body["text"], "two");
    assert!(repo.list("project").await?[0].conflict);
    Ok(())
}
#[tokio::test]
async fn local_connection_bindings_keep_tracking_offline_edits_across_cache_restart() -> Result<()>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cache.sqlite");
    {
        let repo = Repository::open(&path).await?;
        let mut connection = object(1, "");
        connection.kind = Kind::Connection;
        connection.body = json!({"type":"MYSQL","host":"one"});
        repo.receive(connection.clone()).await?;
        repo.bind("object", "legacy-id").await?;
        let baseline = repo.baseline("object").await?;
        assert_eq!(
            baseline,
            cloud::connection_stamp(&connection.name, &connection.body)
        );
        let mut mutation = Mutation::from_object(&connection);
        mutation.body["host"] = "two".into();
        repo.enqueue_local("project", "object", mutation).await?;
        assert!(repo.bindings().await?[0].2);
        let mut mutation = Mutation::from_object(&repo.object("object").await?);
        mutation.body["host"] = "three".into();
        repo.enqueue_local("project", "object", mutation).await?;
        assert_eq!(repo.pending().await?[0].2.body["host"], "three");
    }
    let repo = Repository::open(&path).await?;
    assert_eq!(repo.bindings().await?[0].1, "legacy-id");
    assert!(repo.bindings().await?[0].2);
    assert_eq!(repo.object("object").await?.body["host"], "three");
    Ok(())
}
#[test]
fn cloud_endpoints_enforce_tls_and_reject_embedded_credentials() {
    assert!(Api::new("http://127.0.0.1:8443").is_ok());
    assert!(Api::new("http://[::1]:8443").is_ok());
    assert!(Api::new("https://cloud.example.test").is_ok());
    for value in [
        "http://example.test",
        "https://u:p@example.test",
        "https://example.test?token=x",
        "file:///x",
    ] {
        assert!(Api::new(value).is_err(), "{value}");
    }
}

#[tokio::test]
async fn blocked_uploads_do_not_block_other_files_and_deletions_follow_children() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let repo = Repository::open(&dir.path().join("cache.sqlite")).await?;
    let mut parent = object(1, "parent");
    parent.id = "parent".into();
    parent.kind = Kind::Group;
    let mut child = object(1, "child");
    child.id = "child".into();
    child.parent_id = "parent".into();
    repo.receive(parent.clone()).await?;
    repo.receive(child.clone()).await?;
    let mut delete = Mutation::from_object(&parent);
    delete.deleted = true;
    repo.enqueue("project", "parent", delete).await?;
    let mut delete = Mutation::from_object(&child);
    delete.deleted = true;
    repo.enqueue("project", "child", delete.clone()).await?;
    assert_eq!(repo.pending().await?[0].0, "child");
    let mut accepted = child;
    accepted.revision = 2;
    accepted.deleted = true;
    accepted.mutation_id = delete.mutation_id;
    repo.receive(accepted).await?;
    assert_eq!(repo.pending().await?[0].0, "parent");
    repo.block("parent", "quota").await?;
    assert!(repo.pending().await?.is_empty());
    assert_eq!(
        repo.list("project")
            .await?
            .iter()
            .find(|c| c.object.id == "parent")
            .unwrap()
            .error
            .as_deref(),
        Some("quota")
    );
    assert!(repo.clear().await.is_err());
    repo.retry_blocked().await?;
    assert_eq!(repo.pending().await?[0].0, "parent");
    Ok(())
}

struct Server {
    child: std::process::Child,
    _directory: tempfile::TempDir,
    base: String,
}
impl Server {
    async fn start() -> Result<Self> {
        let exe = std::env::var_os("TOMATO_CLOUD_FIXTURE_EXE")
            .context("set TOMATO_CLOUD_FIXTURE_EXE to the locally built Go server")?;
        let directory = tempfile::tempdir()?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        drop(listener);
        let child = std::process::Command::new(exe)
            .env("TOMATO_CLOUD_SMTP_ADDRESS", "")
            .args([
                "--listen",
                &address.to_string(),
                "--local-fixture-registration",
            ])
            .arg("--data")
            .arg(directory.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let mut server = Self {
            child,
            _directory: directory,
            base: format!("http://{address}"),
        };
        for _ in 0..100 {
            if reqwest::get(format!("{}/healthz", server.base))
                .await
                .is_ok()
            {
                return Ok(server);
            }
            ensure!(
                server.child.try_wait()?.is_none(),
                "Go fixture terminated before ready"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        anyhow::bail!("Go fixture did not become ready")
    }
    async fn account(&self, email: &str) -> Result<Api> {
        let api = Api::new(&self.base)?;
        let _: Value = api
            .request(
                reqwest::Method::POST,
                "/register",
                Some(&json!({"email":email,"name":"fixture","password":"fixture-password-123"})),
            )
            .await?;
        let login = api
            .login(email.into(), "fixture-password-123".into(), String::new())
            .await?;
        Ok(api.authenticated(login.token))
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[tokio::test]
#[ignore = "requires locally built Go cloud fixture; never uses saved servers"]
async fn real_go_server_two_accounts_and_rust_outbox_conflict_recovery() -> Result<()> {
    let server = Server::start().await?;
    let owner = server.account("owner@example.test").await?;
    let editor = server.account("editor@example.test").await?;
    let p: cloud::Project = owner
        .request(
            reqwest::Method::POST,
            "/projects",
            Some(&json!({"name":"共享项目"})),
        )
        .await?;
    let _: Value = owner
        .request(
            reqwest::Method::POST,
            &format!("/projects/{}/invitations", p.id),
            Some(&json!({"email":"editor@example.test","role":"editor"})),
        )
        .await?;
    let invitations: Vec<Value> = editor.get("/invitations").await?;
    let _: Value = editor
        .request(
            reqwest::Method::POST,
            &format!("/invitations/{}", invitations[0]["id"].as_str().unwrap()),
            Some(&json!({"accept":true})),
        )
        .await?;
    assert_eq!(editor.projects().await?.len(), 1);
    let mut original = Mutation::from_object(&object(0, "select 1"));
    let base = owner.put(&p.id, "query", &original).await?;
    let directory = tempfile::tempdir()?;
    let repo = Repository::open(&directory.path().join("cache.sqlite")).await?;
    repo.receive(base.clone()).await?;
    original.base_revision = 1;
    original.body = json!({"text":"select 'offline🙂'"});
    original.mutation_id = cloud::new_id();
    repo.enqueue(&p.id, "query", original.clone()).await?;
    let mut remote = Mutation::from_object(&base);
    remote.body = json!({"text":"select 'teammate'"});
    editor.put(&p.id, "query", &remote).await?;
    let error = owner.put(&p.id, "query", &original).await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<cloud::HttpError>().unwrap().status,
        409
    );
    repo.receive(owner.object(&p.id, "query").await?).await?;
    assert!(repo.list(&p.id).await?[0].conflict);
    repo.resolve("query", true).await?;
    let pending = repo.pending().await?.remove(0);
    let saved = owner.put(&p.id, "query", &pending.2).await?;
    assert_eq!(saved.revision, 3);
    repo.receive(saved).await?;
    assert!(repo.pending().await?.is_empty());
    assert_eq!(
        editor.object(&p.id, "query").await?.body["text"],
        "select 'offline🙂'"
    );
    let history: Vec<Value> = editor
        .get(&format!("/projects/{}/objects/query/history", p.id))
        .await?;
    assert_eq!(history.len(), 3);
    Ok(())
}

async fn wait_client(
    client: &mut cloud::Client,
    predicate: impl Fn(&cloud::View) -> bool,
) -> Result<std::sync::Arc<cloud::View>> {
    for _ in 0..500 {
        if let Some(view) = client.changed()
            && predicate(&view)
        {
            return Ok(view);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    anyhow::bail!("cloud actor did not reach the expected state")
}
#[tokio::test]
#[ignore = "requires owned Go fixture; verifies legacy edits made while the client was stopped"]
async fn real_go_server_recovers_local_legacy_connection_edits_before_pulling_cloud_state()
-> Result<()> {
    let server = Server::start().await?;
    let api = server.account("owner@example.test").await?;
    let project: cloud::Project = api
        .request(
            reqwest::Method::POST,
            "/projects",
            Some(&json!({"name":"offline"})),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    let mut c = tomato_core::Connection::new("MYSQL");
    c.id = "legacy-id".into();
    c.set("host", "one");
    c.set("password", "must-stay-local");
    c.set("futureJava", json!({"nested":[1,2]}));
    let mut store = tomato_core::Store {
        connections: vec![c.clone()],
        ..Default::default()
    };
    store.save(&path)?;
    let mutation = Mutation {
        parent_id: String::new(),
        kind: Kind::Connection,
        name: c.name.clone(),
        body: cloud::connection_body(&c),
        base_revision: 0,
        mutation_id: cloud::new_id(),
        deleted: false,
    };
    api.put(&project.id, "connection", &mutation).await?;
    let cache = directory.path().join("collaboration");
    let mut client = cloud::Client::start(&tokio::runtime::Handle::current(), cache.clone(), || ());
    client.send(cloud::Command::Connect {
        base: server.base.clone(),
        email: "owner@example.test".into(),
        password: "fixture-password-123".into(),
        code: String::new(),
    })?;
    wait_client(&mut client, |v| v.online).await?;
    client.send(cloud::Command::Bind {
        id: "connection".into(),
        local: c.id.clone(),
    })?;
    wait_client(&mut client, |v| !v.bindings.is_empty()).await?;
    drop(client);
    store.connections[0].set("host", "offline-two");
    store.save(&path)?;
    let mut client = cloud::Client::start(&tokio::runtime::Handle::current(), cache, || ());
    client.send(cloud::Command::Connect {
        base: server.base.clone(),
        email: "owner@example.test".into(),
        password: "fixture-password-123".into(),
        code: String::new(),
    })?;
    wait_client(&mut client, |v| v.online).await?;
    let saved = api.object(&project.id, "connection").await?;
    assert_eq!(saved.body["host"], "offline-two");
    assert!(saved.body.get("password").is_none());
    let local = tomato_core::Store::load(&path)?;
    assert_eq!(local.connections[0].text("password"), "must-stay-local");
    assert_eq!(
        local.connections[0].options["futureJava"],
        json!({"nested":[1,2]})
    );
    drop(client);
    Ok(())
}
