//! Opt-in real Linux/Docker acceptance, only against a disposable loopback guest.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tomato_core::Connection;
use tomato_services::terminal::{
    Status,
    ssh::{
        RemoteTerminal,
        tools::{ManagementAction, Operation, Output, management},
    },
};

async fn request(terminal: &mut RemoteTerminal, operation: Operation) -> Result<Output> {
    let id = terminal.request(operation)?;
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            while terminal.try_read().is_some() {}
            if let Some(reply) = terminal.try_reply() {
                if reply.id == 0 {
                    continue;
                }
                ensure!(reply.id == id, "Unexpected owned Linux reply");
                return reply.result.map_err(anyhow::Error::msg);
            }
            if let Status::Failed(error) = terminal.status() {
                anyhow::bail!(error);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?
}
async fn inspect(terminal: &mut RemoteTerminal, id: &str) -> Result<Value> {
    let Output::Inspection { value, .. } = request(
        terminal,
        Operation::Manage(ManagementAction::DockerInspect(id.into())),
    )
    .await?
    else {
        anyhow::bail!("Missing actual Docker inspect response");
    };
    Ok(value)
}
#[tokio::test]
#[ignore = "Requires an owned disposable Linux guest, real Docker daemon and fixture SSH key"]
async fn actual_docker_resource_restart_failure_and_state_readback_over_native_ssh() -> Result<()> {
    ensure!(
        std::env::var("TOMATO_TEST_OWNED_LINUX").as_deref() == Ok("owned-loopback-linux"),
        "Owned Linux fixture authorization missing"
    );
    let port = std::env::var("TOMATO_TEST_LINUX_SSH_PORT")?.parse::<u16>()?;
    ensure!(port > 0, "Fixture SSH port missing");
    let key =
        PathBuf::from(std::env::var_os("TOMATO_TEST_LINUX_KEY").context("Fixture key missing")?);
    ensure!(
        key.is_file() && key.file_name().is_some_and(|n| n == "fixture-ed25519"),
        "Owned fixture key required"
    );
    let mut connection = Connection::new("SSH");
    connection.set("host", "127.0.0.1");
    connection.set("port", port);
    connection.set("username", "tomato");
    connection.set("usePassword", false);
    connection.set("useKey", true);
    connection.set("privateKeyPaths", serde_json::json!([key]));
    let profile = tempfile::tempdir()?;
    let mut terminal = RemoteTerminal::start_mode(
        &tokio::runtime::Handle::current(),
        connection,
        profile.path().join("known_hosts"),
        false,
    )?;
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            match terminal.status() {
                Status::Ready => return Ok::<_, anyhow::Error>(()),
                Status::Failed(error) => anyhow::bail!(error),
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    })
    .await??;
    let id = "tomato-resource-fixture1690";
    let before = inspect(&mut terminal, id).await?;
    ensure!(
        before["HostConfig"]["Memory"] == 64_u64 << 20,
        "Unexpected owned container initial memory"
    );
    let action = management::docker::resource_update(
        id,
        "96m",
        "0.5",
        "1024",
        management::docker::Limits::read(&before),
    )?
    .context("Missing update")?;
    request(&mut terminal, Operation::Manage(action)).await?;
    let current = inspect(&mut terminal, id).await?;
    ensure!(
        current["HostConfig"]["Memory"] == 96_u64 << 20
            && current["HostConfig"]["MemorySwap"] == 160_u64 << 20,
        "Actual Docker memory/swap readback mismatch"
    );
    ensure!(
        current["HostConfig"]["NanoCpus"] == 500_000_000_u64
            && current["HostConfig"]["CpuShares"] == 1024,
        "Actual Docker CPU readback mismatch"
    );
    request(
        &mut terminal,
        Operation::Manage(ManagementAction::DockerUpdate {
            id: id.into(),
            restart: Some("on-failure:3".into()),
            memory: None,
            memory_swap: None,
            cpus: None,
            shares: None,
        }),
    )
    .await?;
    let current = inspect(&mut terminal, id).await?;
    ensure!(
        current["HostConfig"]["RestartPolicy"]["Name"] == "on-failure"
            && current["HostConfig"]["RestartPolicy"]["MaximumRetryCount"] == 3,
        "Actual restart-policy readback mismatch"
    );
    let action = management::docker::resource_update(
        id,
        "1",
        "",
        "",
        management::docker::Limits::read(&current),
    )?
    .unwrap();
    ensure!(
        request(&mut terminal, Operation::Manage(action))
            .await
            .is_err(),
        "Real Docker daemon must reject subminimum memory"
    );
    let after = inspect(&mut terminal, id).await?;
    ensure!(
        after["HostConfig"]["Memory"] == current["HostConfig"]["Memory"]
            && after["HostConfig"]["MemorySwap"] == current["HostConfig"]["MemorySwap"],
        "Failed daemon update changed resource state"
    );
    for action in ["stop", "start"] {
        request(
            &mut terminal,
            Operation::Manage(ManagementAction::Docker {
                action: action.into(),
                ids: vec![id.into()],
            }),
        )
        .await?;
        let value = inspect(&mut terminal, id).await?;
        ensure!(
            value["State"]["Running"] == (action == "start"),
            "Actual Docker start/stop readback mismatch"
        );
    }
    drop(terminal);
    Ok(())
}
