//! A stateful remote-command peer; no disk or system command is ever executed.
use super::*;
use tomato_services::terminal::ssh::tools::ManagementAction;
#[derive(Default)]
pub(super) struct Peer {
    pub initialized: bool,
    pub joined: bool,
    pub extended: bool,
    pub mounted: bool,
    pub reject: bool,
    pub attempts: usize,
    pub queries: usize,
    pub commands: Vec<String>,
}
impl Peer {
    fn inventory(&self) -> String {
        let mut text = format!(
            "PATH=\"/dev/sdb\" TYPE=\"disk\" SIZE=\"2147483648\" PTTYPE=\"{}\" FSTYPE=\"\" MOUNTPOINT=\"\" PKNAME=\"\"\nPATH=\"/dev/sdc1\" TYPE=\"part\" SIZE=\"4294967296\" PTTYPE=\"\" FSTYPE=\"LVM2_member\" MOUNTPOINT=\"\" PKNAME=\"sdc\"\nPATH=\"/dev/data/lv_data\" TYPE=\"lvm\" SIZE=\"{}\" PTTYPE=\"\" FSTYPE=\"ext4\" MOUNTPOINT=\"{}\" PKNAME=\"sdc1\"\n",
            if self.initialized { "gpt" } else { "" },
            if self.extended {
                2147483648_u64
            } else {
                1073741824
            },
            if self.mounted { "/fixture-data" } else { "" }
        );
        if self.initialized {
            text.push_str("PATH=\"/dev/sdb1\" TYPE=\"part\" SIZE=\"2146435072\" PTTYPE=\"\" FSTYPE=\"LVM2_member\" MOUNTPOINT=\"\" PKNAME=\"sdb\"\n");
        }
        text.push_str("__TOMATO_PVS__\n/dev/sdc1|data|4294967296|2147483648\n");
        if self.initialized {
            text.push_str(&format!(
                "/dev/sdb1|{}|2146435072|2146435072\n",
                if self.joined { "vg_data" } else { "" }
            ));
        }
        text.push_str("__TOMATO_VGS__\ndata|4294967296|2147483648\n");
        if self.joined {
            text.push_str("vg_data|2146435072|2146435072\n");
        }
        text.push_str(&format!(
            "__TOMATO_LVS__\n/dev/data/lv_data|data|lv_data|{}\n",
            if self.extended {
                2147483648_u64
            } else {
                1073741824
            }
        ));
        text
    }
    fn execute(&mut self, command: &str) -> Result<String> {
        ensure!(self.commands.len() < 64, "Fixture command bound");
        self.commands.push(command.into());
        if command.contains("lsblk -P") {
            self.queries += 1;
            return Ok(self.inventory());
        }
        if !(command.contains("wipefs -a")
            || command.contains("vgextend")
            || command.contains("lvextend -r")
            || command.contains("mkdir -p"))
        {
            return Ok("fixture command completed\n".into());
        }
        self.attempts += 1;
        ensure!(!self.reject, "storage fixture rejected mutation");
        if command.contains("wipefs -a") {
            ensure!(
                command.contains("/dev/sdb") && !self.initialized,
                "Unexpected disk initialization"
            );
            self.initialized = true;
        } else if command.contains("vgextend") {
            ensure!(
                self.initialized && command.contains("/dev/sdb1") && command.contains("vg_data"),
                "Unexpected volume-group update"
            );
            self.joined = true;
        } else if command.contains("lvextend -r") {
            ensure!(
                command.contains("/dev/data/lv_data")
                    && command.contains("-L")
                    && command.contains("+1G"),
                "Unexpected LV extension"
            );
            self.extended = true;
        } else {
            ensure!(
                command.contains("/dev/data/lv_data")
                    && command.contains("/fixture-data")
                    && !command.contains("mkfs.")
                    && !command.contains("/etc/fstab"),
                "Mount options were not preserved"
            );
            self.mounted = true;
        }
        Ok("storage fixture mutation completed\n".into())
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
async fn storage_mutations_readback_and_failure_use_real_encrypted_ssh() -> Result<()> {
    let peer = Arc::new(std::sync::Mutex::new(Peer::default()));
    let fixture = Fixture::start(Mode::Storage(peer.clone())).await?;
    let mut terminal = fixture.terminal("SSH").await?;
    let actions = [
        ManagementAction::InitializePv("/dev/sdb".into()),
        ManagementAction::VolumeGroup {
            pv: "/dev/sdb1".into(),
            vg: "vg_data".into(),
        },
        ManagementAction::ExtendLv {
            device: "/dev/data/lv_data".into(),
            amount: "+1G".into(),
        },
        ManagementAction::Mount {
            device: "/dev/data/lv_data".into(),
            directory: "/fixture-data".into(),
            filesystem: None,
            persist: false,
        },
    ];
    for action in actions {
        assert!(matches!(
            request(&mut terminal, Operation::Manage(action)).await?,
            Output::Done(_)
        ));
        assert!(matches!(
            request(&mut terminal, Operation::Query(Section::Storage)).await?,
            Output::Table(_)
        ));
    }
    {
        let p = peer.lock().unwrap();
        assert!(p.initialized && p.joined && p.extended && p.mounted);
        assert_eq!(p.queries, 4);
    }
    peer.lock().unwrap().reject = true;
    assert!(
        request(
            &mut terminal,
            Operation::Manage(ManagementAction::ExtendLv {
                device: "/dev/data/lv_data".into(),
                amount: "+1G".into()
            })
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("storage fixture rejected mutation")
    );
    assert!(peer.lock().unwrap().extended);
    Ok(())
}
