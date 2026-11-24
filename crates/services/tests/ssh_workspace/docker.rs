//! A stateful SSH command peer checks Docker constraints independently of the UI model.
use super::*;
use tomato_services::terminal::ssh::tools::{ManagementAction, management};
pub(super) struct Peer {
    pub running: bool,
    pub memory: u64,
    pub swap: i64,
    pub policy: String,
    pub retries: u32,
    pub commands: Vec<String>,
    pub shell_input: Vec<u8>,
    pub fail_next: bool,
}
impl Default for Peer {
    fn default() -> Self {
        Self {
            running: true,
            memory: 512 << 20,
            swap: 1024 << 20,
            policy: "on-failure".into(),
            retries: 2,
            commands: Vec::new(),
            shell_input: Vec::new(),
            fail_next: false,
        }
    }
}
impl Peer {
    fn inspect(&self) -> serde_json::Value {
        serde_json::json!([{"Name":"/web","Id":"fixture-web","Config":{"Image":"alpine:3","Path":"sh","Args":["-c","echo 中文"],"Env":["ENV=fixture","LANG=C.UTF-8"],"Hostname":"web"},"Created":"2026-11-02T00:00:00Z","State":{"Status":"running"},"HostConfig":{"Memory":self.memory,"MemorySwap":self.swap,"NanoCpus":0,"CpuShares":0,"RestartPolicy":{"Name":self.policy,"MaximumRetryCount":self.retries},"PortBindings":{"80/tcp":[{"HostIp":"0.0.0.0","HostPort":"8080"}]},"NetworkMode":"bridge"},"Mounts":[{"Source":"/data","Destination":"/app","RW":true,"Type":"bind"}],"NetworkSettings":{"Networks":{"bridge":{"IPAddress":"172.17.0.2"}}}}])
    }
    fn execute(&mut self, command: &str) -> Result<String> {
        ensure!(self.commands.len() < 64, "Fixture command bound exceeded");
        self.commands.push(command.into());
        if command.contains("docker ps ") {
            return Ok(format!(
                "fixture-web|alpine:3|web|{}|0.0.0.0:8080->80/tcp|2026-11-02\n",
                if self.running {
                    "Up 2 hours"
                } else {
                    "Exited (0)"
                }
            ));
        }
        for action in ["stop", "start", "restart"] {
            if command.contains(&format!("docker {action} ")) {
                self.running = action != "stop";
                return Ok("web\n".into());
            }
        }
        if command.contains("docker inspect ") {
            return Ok(self.inspect().to_string());
        }
        if command.contains("docker update ") {
            if std::mem::take(&mut self.fail_next) {
                anyhow::bail!("Docker daemon rejected fixture update");
            }
            let command = command.split("; else").next().unwrap_or(command);
            let option = |name: &str| {
                command
                    .split_ascii_whitespace()
                    .find_map(|word| word.strip_prefix(name))
            };
            let memory = option("--memory=")
                .map(str::parse::<u64>)
                .transpose()?
                .unwrap_or(self.memory);
            let swap = option("--memory-swap=")
                .map(str::parse::<i64>)
                .transpose()?
                .unwrap_or(self.swap);
            ensure!(
                swap == -1 || swap >= 0 && swap as u64 >= memory,
                "Minimum memoryswap limit should be larger than memory limit"
            );
            self.memory = memory;
            self.swap = swap;
            if let Some(policy) = option("--restart=") {
                let policy = policy.trim_matches('\'');
                let (name, retries) = policy.split_once(':').unwrap_or((policy, "0"));
                self.policy = name.into();
                self.retries = retries.parse()?;
            }
            return Ok("web\n".into());
        }
        Ok("fixture command completed\n".into())
    }
}
pub(super) fn serve(
    peer: &Arc<std::sync::Mutex<Peer>>,
    id: ChannelId,
    command: &str,
    session: &mut server::Session,
) -> Result<()> {
    match peer.lock().unwrap().execute(command) {
        Ok(text) => {
            session.data(id, text.into_bytes())?;
            session.exit_status_request(id, 0)?;
        }
        Err(error) => {
            session.extended_data(id, 1, error.to_string().into_bytes())?;
            session.exit_status_request(id, 1)?;
        }
    }
    session.eof(id)?;
    session.close(id)?;
    Ok(())
}
#[tokio::test]
async fn container_inspection_resource_update_failure_and_refresh_use_real_ssh() -> Result<()> {
    let peer = Arc::new(std::sync::Mutex::new(Peer::default()));
    let fixture = Fixture::start(Mode::Docker(peer.clone())).await?;
    let mut terminal = fixture.terminal("SSH").await?;
    let Output::Inspection { value, .. } = request(
        &mut terminal,
        Operation::Manage(ManagementAction::DockerInspect("web".into())),
    )
    .await?
    else {
        anyhow::bail!("Missing initial inspect")
    };
    let action = management::docker::resource_update(
        "web",
        "2g",
        "",
        "",
        management::docker::Limits::read(&value),
    )?
    .context("Missing update")?;
    let Output::Done(_) = request(&mut terminal, Operation::Manage(action)).await? else {
        anyhow::bail!("Missing update response")
    };
    let Output::Inspection { value, .. } = request(
        &mut terminal,
        Operation::Manage(ManagementAction::DockerInspect("web".into())),
    )
    .await?
    else {
        anyhow::bail!("Missing refreshed inspect")
    };
    assert_eq!(value["HostConfig"]["Memory"], 2_u64 << 30);
    assert_eq!(
        value["HostConfig"]["MemorySwap"],
        (2_u64 << 30) + (512 << 20)
    );
    peer.lock().unwrap().fail_next = true;
    let action = management::docker::resource_update(
        "web",
        "3g",
        "",
        "",
        management::docker::Limits::read(&value),
    )?
    .unwrap();
    assert!(
        request(&mut terminal, Operation::Manage(action))
            .await
            .unwrap_err()
            .to_string()
            .contains("Docker daemon rejected")
    );
    assert_eq!(peer.lock().unwrap().memory, 2_u64 << 30);
    drop(terminal);
    Ok(())
}
