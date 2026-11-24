#[path = "support/cloud_fixture.rs"]
mod fixture;
#[path = "cloud/renewal.rs"]
mod renewal;
use anyhow::Result;
use tomato_services::{
    backup::Control,
    cloud::{self, Action, Record},
};
#[tokio::test]
async fn both_cloud_providers_create_edit_list_delete_records_and_enumerate_resources() -> Result<()>
{
    let fixture = fixture::Fixture::start().await?;
    for kind in ["ALIYUN", "TENCENT_CLOUD"] {
        let c = fixture.connection(kind);
        cloud::execute(&c, Action::Verify).await?;
        let instances = cloud::execute(
            &c,
            Action::Instances {
                region: String::new(),
            },
        )
        .await?;
        assert_eq!(instances.rows[0]["instanceId"], "owned-instance");
        let domains = cloud::execute(&c, Action::Domains).await?;
        assert_eq!(domains.rows[0]["domainName"], "owned.test");
        let client = cloud::api::Client::new(&c)?;
        let mut r = Record {
            rr: "home".into(),
            kind: "A".into(),
            value: "192.0.2.10".into(),
            ttl: 600,
            line: "default".into(),
            ..Default::default()
        };
        r.record_id = client.save_record("owned.test", &r).await?;
        r.value = "192.0.2.20".into();
        client.save_record("owned.test", &r).await?;
        let listed = client
            .execute(Action::Records {
                domain: "owned.test".into(),
            })
            .await?;
        assert_eq!(listed.rows.len(), 1);
        assert_eq!(listed.rows[0]["value"], "192.0.2.20");
        client.delete_record("owned.test", &r.record_id).await?;
        if kind == "TENCENT_CLOUD" {
            r.record_id.clear();
            r.kind = "MX".into();
            r.value = "mx.owned.test.".into();
            r.priority = 10;
            let id = client.save_record("owned.test", &r).await?;
            client.delete_record("owned.test", &id).await?;
        }
        assert!(
            client
                .execute(Action::Records {
                    domain: "owned.test".into()
                })
                .await?
                .rows
                .is_empty()
        );
    }
    Ok(())
}
#[test]
fn legacy_ddns_preserves_unknown_fields_and_backs_up_before_updates() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("ddns.json");
    let old=br#"[{"connectionId":"old","domainName":"owned.test","recordId":"1001","rr":"home","type":"A","ttl":"600","line":"default","future":{"keep":true}}]"#;
    std::fs::write(&path, old)?;
    let mut c = tomato_core::Connection::new("ALIYUN");
    c.id = "old".into();
    let r = Record {
        record_id: "1001".into(),
        rr: "home".into(),
        kind: "A".into(),
        ttl: 600,
        ..Default::default()
    };
    cloud::ddns::toggle(&path, &c, "owned.test", &r, true)?;
    assert_eq!(cloud::ddns::load(&path)?[0].extra["future"]["keep"], true);
    assert_eq!(
        std::fs::read(path.with_extension("json.before-rust.bak"))?,
        old
    );
    let mut other = c.clone();
    other.id = "other".into();
    cloud::ddns::toggle(&path, &other, "owned.test", &r, true)?;
    cloud::ddns::toggle(&path, &c, "owned.test", &r, false)?;
    assert_eq!(cloud::ddns::load(&path)?[0].connection_id, "other");
    Ok(())
}
#[tokio::test]
async fn certificate_invalid_storage_is_rejected_before_creating_an_acme_order() -> Result<()> {
    let fixture = fixture::Fixture::start().await?;
    let connection = fixture.connection("ALIYUN");
    for invalid_prefix in ["../escape", "valid/../escape", "bad\\path"] {
        let mut request = fixture.request();
        request.prefix = invalid_prefix.into();
        assert!(
            cloud::certificate::issue(&connection, request, &Control::default())
                .await
                .is_err()
        );
    }
    let mut request = fixture.request();
    request.storage.set("password", "");
    assert!(
        cloud::certificate::issue(&connection, request, &Control::default())
            .await
            .is_err()
    );
    let state = fixture.state.inner.lock().unwrap();
    assert!(state.records.is_empty());
    assert!(state.writes.is_empty());
    assert!(state.calls.is_empty());
    Ok(())
}
#[tokio::test]
async fn actual_acme_dns01_jws_csr_issuance_s3_upload_and_all_server_formats() -> Result<()> {
    let fixture = fixture::Fixture::start().await?;
    {
        let mut state = fixture.state.inner.lock().unwrap();
        state.cache_txt_answers = true;
        state.first_txt_query_nxdomain = true;
    }
    let c = fixture.connection("TENCENT_CLOUD");
    let mut request = fixture.request();
    request.dns_zone = " OWNED.TEST. ".into();
    request
        .storage
        .set("endpoint", fixture.endpoint.trim_start_matches("http://"));
    request.storage.set("pathStyleAccess", false);
    let control = Control::default();
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        cloud::certificate::issue(&c, request.clone(), &control),
    )
    .await??;
    assert!(output.contains("cert-bucket"));
    let s = fixture.state.inner.lock().unwrap();
    assert!(
        s.records.is_empty(),
        "created challenge TXT must be removed"
    );
    assert!(s.calls.iter().any(|s| s == "/acme/finalize"));
    assert_eq!(s.auth_ready, 2);
    assert_eq!(s.writes.len(), 12);
    let p = "/cert-bucket/certificates/owned.test/";
    let pfx = &s.writes[&format!("{p}tomcat/owned.test.pfx")];
    let ks = p12_keystore::KeyStore::from_pkcs12(
        pfx,
        &request.keystore_password,
        p12_keystore::Pkcs12ImportPolicy::Strict,
    )?;
    assert!(ks.private_key_chain().is_some());
    let mut jks = jks::KeyStore::new();
    jks.load(
        &s.writes[&format!("{p}jks/owned.test.jks")][..],
        request.keystore_password.as_bytes(),
    )?;
    assert!(
        !jks.get_private_key_entry("certificate", request.keystore_password.as_bytes())?
            .private_key
            .is_empty()
    );
    let key = std::str::from_utf8(&s.writes[&format!("{p}pem/private.key")])?;
    let chain = std::str::from_utf8(&s.writes[&format!("{p}pem/fullchain.pem")])?;
    let account = std::str::from_utf8(&s.writes[&format!("{p}pem/account.key")])?;
    let mut zip_request = request;
    zip_request.zip_package = true;
    let archive = cloud::certificate::package(&zip_request, chain, key, account)?;
    let archive = archive.values().next().unwrap();
    let zip = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
    assert_eq!(zip.len(), 12);
    Ok(())
}
#[tokio::test]
async fn certificate_honors_ca_retry_after_beyond_thirty_seconds() -> Result<()> {
    let fixture = fixture::Fixture::start().await?;
    fixture.state.inner.lock().unwrap().order_retry_after = Some(32);
    let mut request = fixture.request();
    request.zip_package = true;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        cloud::certificate::issue(
            &fixture.connection("TENCENT_CLOUD"),
            request,
            &Control::default(),
        ),
    )
    .await??;
    assert!(output.contains("证书已保存"));
    let state = fixture.state.inner.lock().unwrap();
    assert!(state.records.is_empty());
    assert_eq!(state.writes.len(), 1);
    Ok(())
}

#[tokio::test]
async fn certificate_exposes_ca_dns_rejection_and_cleans_only_records_it_created() -> Result<()> {
    let fixture = fixture::Fixture::start().await?;
    let connection = fixture.connection("TENCENT_CLOUD");
    let existing = cloud::api::Client::new(&connection)?
        .save_record(
            "owned.test",
            &Record {
                rr: "_acme-challenge".into(),
                kind: "TXT".into(),
                value: "another-order".into(),
                ttl: 600,
                ..Default::default()
            },
        )
        .await?;
    fixture.state.inner.lock().unwrap().deny_authorization = true;
    let error = cloud::certificate::issue(&connection, fixture.request(), &Control::default())
        .await
        .expect_err("an invalid ACME order must fail");
    let message = format!("{error:#}");
    assert!(message.contains("证书机构拒绝了 DNS-01 验证"), "{message}");
    assert!(
        message.contains("NXDOMAIN looking up TXT for _acme-challenge.owned.test"),
        "{message}"
    );
    let state = fixture.state.inner.lock().unwrap();
    assert_eq!(state.records.len(), 1);
    assert_eq!(state.records[&existing].value, "another-order");
    assert!(state.writes.is_empty());
    Ok(())
}

#[tokio::test]
async fn certificate_cancel_cleans_owned_txt_and_never_uploads() -> Result<()> {
    let fixture = fixture::Fixture::start().await?;
    fixture.state.inner.lock().unwrap().cancel_before_ready = true;
    let c = fixture.connection("ALIYUN");
    let r = fixture.request();
    let control = Control::default();
    let other = control.clone();
    let state = fixture.state.clone();
    let cancel = tokio::spawn(async move {
        for _ in 0..100 {
            if state
                .inner
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| c.starts_with("/acme/challenge/"))
            {
                other.cancel();
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        other.cancel();
    });
    assert!(cloud::certificate::issue(&c, r, &control).await.is_err());
    cancel.await?;
    assert!(control.cancelled());
    assert!(
        fixture
            .state
            .inner
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c.starts_with("/acme/challenge/"))
    );
    let s = fixture.state.inner.lock().unwrap();
    assert!(s.records.is_empty());
    assert!(s.writes.is_empty());
    Ok(())
}

#[tokio::test]
async fn dropping_the_certificate_caller_still_cleans_dns_records() -> Result<()> {
    use std::time::Duration;
    let fixture = fixture::Fixture::start().await?;
    fixture.state.inner.lock().unwrap().cancel_before_ready = true;
    let c = fixture.connection("TENCENT_CLOUD");
    let r = fixture.request();
    let pending =
        tokio::spawn(async move { cloud::certificate::issue(&c, r, &Control::default()).await });
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if fixture
                .state
                .inner
                .lock()
                .unwrap()
                .calls
                .contains(&"/acme/challenge/1".into())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    assert_eq!(fixture.state.inner.lock().unwrap().records.len(), 2);
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if fixture.state.inner.lock().unwrap().records.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    assert!(fixture.state.inner.lock().unwrap().writes.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "hosts the owned cloud/ACME fixture for native UI checks"]
async fn host_cloud_fixture() -> Result<()> {
    let directory = std::path::PathBuf::from(std::env::var("TOMATO_CLOUD_FIXTURE_DIR")?);
    anyhow::ensure!(
        directory.is_absolute()
            && directory
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with("tomato-cloud-fixture-")),
        "Requires owned fixture directory"
    );
    std::fs::create_dir_all(&directory)?;
    let fixture = fixture::Fixture::start().await?;
    {
        let mut state = fixture.state.inner.lock().unwrap();
        state.reject_bucket_list = true;
        state.bucket_list_delay = std::time::Duration::from_secs(5);
    }
    std::fs::write(
        directory.join("ready.json"),
        serde_json::to_vec(
            &serde_json::json!({"endpoint":fixture.endpoint,"dns":fixture.dns.to_string()}),
        )?,
    )?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
    while !directory.join("stop").exists() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let state = fixture.state.inner.lock().unwrap();
    std::fs::write(
        directory.join("audit.json"),
        serde_json::to_vec(
            &serde_json::json!({"calls":state.calls,"records":state.records,"uploaded":state.writes.keys().collect::<Vec<_>>()}),
        )?,
    )?;
    Ok(())
}

#[test]
fn tc3_signature_matches_current_original_java_helpers() -> Result<()> {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/cloud-signatures.json"))?;
    let t = |key: &str| v[key].as_str().unwrap();
    assert_eq!(
        cloud::api::tencent_signature(
            t("id"),
            t("secret"),
            t("service"),
            t("host"),
            t("body"),
            v["timestamp"].as_i64().unwrap(),
            t("date")
        ),
        t("authorization")
    );
    assert_eq!(
        cloud::api::escape("a +/中文~"),
        "a%20%2B%2F%E4%B8%AD%E6%96%87~"
    );
    Ok(())
}

#[tokio::test]
async fn ddns_worker_detects_ip_retries_failed_records_and_stops_after_disable() -> Result<()> {
    let fixture = fixture::Fixture::start().await?;
    let c = fixture.connection("TENCENT_CLOUD");
    let client = cloud::api::Client::new(&c)?;
    let mut record = Record {
        rr: "ddns".into(),
        kind: "A".into(),
        value: "192.0.2.1".into(),
        ttl: 600,
        ..Default::default()
    };
    record.record_id = client.save_record("owned.test", &record).await?;
    let root = tempfile::tempdir()?;
    let config = root.path().join("connections.json");
    let path = root.path().join("ddns.json");
    let store = tomato_core::Store {
        connections: vec![c.clone()],
        ..Default::default()
    };
    store.save(&config)?;
    cloud::ddns::toggle(&path, &c, "owned.test", &record, true)?;
    fixture.state.inner.lock().unwrap().public_ip = "<html>69.63.203.33</html>".into();
    let runner = cloud::ddns::Runner::with_sources(
        path.clone(),
        config,
        &tokio::runtime::Handle::current(),
        std::time::Duration::from_millis(40),
        vec![
            format!("{}/ip?source=a", fixture.endpoint),
            format!("{}/ip?source=b", fixture.endpoint),
        ],
    );
    async fn until(
        fixture: &fixture::Fixture,
        predicate: impl Fn(&fixture::Data) -> bool,
    ) -> Result<()> {
        let start = tokio::time::Instant::now();
        loop {
            if predicate(&fixture.state.inner.lock().unwrap()) {
                return Ok(());
            }
            anyhow::ensure!(
                start.elapsed() < std::time::Duration::from_secs(5),
                "DDNS fixture timeout"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    // Observe completion of the invalid-IP check instead of assuming the worker
    // and both fixture requests finish within 150 ms on a busy build machine.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while runner.take_notice().is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(
        !fixture
            .state
            .inner
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c == "ModifyRecord")
    );
    fixture.state.inner.lock().unwrap().public_ip = "8.8.4.31".into();
    until(&fixture, |s| {
        s.records.values().any(|r| r.value == "8.8.4.31")
    })
    .await?;
    {
        let mut s = fixture.state.inner.lock().unwrap();
        s.reject_save = true;
        s.public_ip = "8.8.4.32".into();
    }
    until(&fixture, |s| {
        s.calls
            .iter()
            .filter(|c| c.as_str() == "ModifyRecord")
            .count()
            >= 2
    })
    .await?;
    fixture.state.inner.lock().unwrap().reject_save = false;
    until(&fixture, |s| {
        s.records.values().any(|r| r.value == "8.8.4.32")
    })
    .await?;
    cloud::ddns::toggle(&path, &c, "owned.test", &record, false)?;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let calls = fixture.state.inner.lock().unwrap().calls.len();
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    assert_eq!(fixture.state.inner.lock().unwrap().calls.len(), calls);
    drop(runner);
    Ok(())
}
#[tokio::test]
async fn acme_external_account_binding_and_zip_export() -> Result<()> {
    use base64::Engine;
    let fixture = fixture::Fixture::start().await?;
    fixture.state.inner.lock().unwrap().required_eab = true;
    let c = fixture.connection("ALIYUN");
    let mut request = fixture.request();
    request.certificate_authority = "ZeroSSL（90 天，需 EAB）".into();
    request.eab_kid = "fixture-eab".into();
    request.eab_hmac = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7u8; 32]);
    request.zip_package = true;
    cloud::certificate::issue(&c, request, &Control::default()).await?;
    let state = fixture.state.inner.lock().unwrap();
    assert!(state.records.is_empty());
    assert_eq!(state.writes.len(), 1);
    let archive = state.writes.values().next().unwrap();
    assert_eq!(
        zip::ZipArchive::new(std::io::Cursor::new(archive))?.len(),
        12
    );
    Ok(())
}
