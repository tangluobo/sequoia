use anyhow::{Result, ensure};
pub mod docker;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum Section {
    Terminal,
    Docker,
    Services,
    Ports,
    Storage,
    Files,
    Monitor,
}
impl Section {
    pub const TREE: [Self; 6] = [
        Self::Terminal,
        Self::Docker,
        Self::Services,
        Self::Ports,
        Self::Storage,
        Self::Files,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "终端",
            Self::Docker => "容器",
            Self::Services => "服务",
            Self::Ports => "端口",
            Self::Storage => "存储",
            Self::Files => "文件",
            Self::Monitor => "监控",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::Terminal => "images/connect/shell.png",
            Self::Docker => "images/connect/docker.png",
            Self::Services => "images/connect/tree-icons/service.svg",
            Self::Ports => "ssh:port",
            Self::Storage => "images/connect/tree-icons/storage.svg",
            Self::Files => "images/connect/folder.png",
            Self::Monitor => "images/connect/monitor.png",
        }
    }
}
#[derive(Clone, Debug)]
pub enum ManagementAction {
    Docker {
        action: String,
        ids: Vec<String>,
    },
    DockerLogs(String),
    DockerInspect(String),
    DockerUpdate {
        id: String,
        restart: Option<String>,
        memory: Option<u64>,
        memory_swap: Option<i64>,
        cpus: Option<f64>,
        shares: Option<u32>,
    },
    StopProcesses(Vec<u32>),
    AllowPort {
        port: u16,
        udp: bool,
    },
    Service {
        action: String,
        name: String,
    },
    InitializePv(String),
    VolumeGroup {
        pv: String,
        vg: String,
    },
    ExtendLv {
        device: String,
        amount: String,
    },
    Mount {
        device: String,
        directory: String,
        filesystem: Option<String>,
        persist: bool,
    },
}
#[derive(Clone, Debug)]
pub struct Table {
    pub section: Section,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub detail: String,
}
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub fn container_terminal(id: &str, logs: bool) -> Result<String> {
    ensure!(identifier(id), "无效容器标识");
    let args = if logs {
        format!("logs -f --tail 100 -- {}", quote(id))
    } else {
        format!(
            "exec -it -- {} sh -c {}",
            quote(id),
            quote("command -v bash >/dev/null 2>&1 && exec bash || exec sh")
        )
    };
    Ok(format!(
        "if sudo -n docker version >/dev/null 2>&1; then sudo -n docker {args}; else docker {args}; fi"
    ))
}
pub fn query(section: Section) -> String {
    match section {
        Section::Docker => "sudo -n docker ps -a --format '{{.ID}}|{{.Image}}|{{.Names}}|{{.Status}}|{{.Ports}}|{{.CreatedAt}}' 2>/dev/null || docker ps -a --format '{{.ID}}|{{.Image}}|{{.Names}}|{{.Status}}|{{.Ports}}|{{.CreatedAt}}'".into(),
        Section::Ports => r#"ports=$(sudo -n ss -lnpH 2>/dev/null || ss -lnpH 2>/dev/null || true); if [ -z "$ports" ]; then ports=$(sudo -n netstat -tulnp 2>/dev/null || netstat -tulnp 2>/dev/null || true); fi; printf '%s\n' "$ports"; echo __TOMATO_FIREWALL__; sudo -n firewall-cmd --list-all 2>/dev/null || firewall-cmd --list-all 2>/dev/null || sudo -n iptables -S INPUT 2>/dev/null || iptables -S INPUT 2>/dev/null || true; for service in $(firewall-cmd --list-services 2>/dev/null); do echo __TOMATO_SERVICE__; firewall-cmd --info-service "$service" 2>/dev/null || true; done"#.into(),
        Section::Services => "LC_ALL=C systemctl list-units --type=service --all --no-legend --no-pager --plain".into(),
        Section::Storage => "LC_ALL=C lsblk -P -b -o PATH,TYPE,SIZE,PTTYPE,FSTYPE,MOUNTPOINT,PKNAME; echo __TOMATO_PVS__; sudo -n pvs --noheadings --separator '|' --units b --nosuffix -o pv_name,vg_name,pv_size,pv_free 2>&1; echo __TOMATO_VGS__; sudo -n vgs --noheadings --separator '|' --units b --nosuffix -o vg_name,vg_size,vg_free 2>&1; echo __TOMATO_LVS__; sudo -n lvs --noheadings --separator '|' --units b --nosuffix -o lv_path,vg_name,lv_name,lv_size 2>&1; true".into(),
        Section::Monitor => "head -n 1 /proc/stat; echo __SEP__; cat /proc/meminfo; echo __SEP__; cat /proc/net/dev; echo __SEP__; cat /proc/diskstats; echo __SEP__; for device in /sys/block/*; do [ -e \"$device/device\" ] && printf '%s\\n' \"${device##*/}\"; done; true".into(),
        _ => String::new(),
    }
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:@/-".contains(&b))
        && !value.starts_with('-')
}
fn device(value: &str) -> bool {
    value.starts_with("/dev/") && identifier(value) && !value.split('/').any(|p| p == "..")
}
pub fn command(action: &ManagementAction) -> Result<String> {
    let command = match action {
        ManagementAction::DockerUpdate {
            id,
            restart,
            memory,
            memory_swap,
            cpus,
            shares,
        } => {
            ensure!(identifier(id), "无效容器标识");
            let mut args = Vec::new();
            if let Some(policy) = restart {
                let (name, count) = policy.split_once(':').unwrap_or((policy, "0"));
                ensure!(
                    ["no", "always", "unless-stopped", "on-failure"].contains(&name)
                        && count.parse::<u32>().is_ok(),
                    "无效重启策略"
                );
                args.push(format!("--restart={}", quote(policy)));
            }
            if let Some(memory) = memory {
                ensure!(*memory <= i64::MAX as u64, "无效内存限制");
                args.push(format!("--memory={memory}"));
            }
            if let Some(swap) = memory_swap {
                ensure!(
                    memory
                        .is_some_and(|memory| *swap == -1 || *swap >= 0 && *swap as u64 >= memory),
                    "无效内存与交换空间限制"
                );
                args.push(format!("--memory-swap={swap}"));
            }
            if let Some(cpus) = cpus {
                ensure!(cpus.is_finite() && *cpus >= 0., "无效 CPU 数量");
                args.push(format!("--cpus={cpus}"));
            }
            if let Some(shares) = shares {
                args.push(format!("--cpu-shares={shares}"));
            }
            ensure!(!args.is_empty(), "没有可应用的容器参数");
            format!("docker update {} -- {}", args.join(" "), quote(id))
        }
        ManagementAction::Docker { action, ids } => {
            ensure!(
                ["start", "stop", "restart", "rm", "rm -f"].contains(&action.as_str()),
                "无效 Docker 操作"
            );
            ensure!(
                !ids.is_empty() && ids.len() <= 1000 && ids.iter().all(|id| identifier(id)),
                "无效容器标识"
            );
            format!(
                "docker {action} -- {}",
                ids.iter().map(|v| quote(v)).collect::<Vec<_>>().join(" ")
            )
        }
        ManagementAction::DockerLogs(id) | ManagementAction::DockerInspect(id) => {
            ensure!(identifier(id), "无效容器标识");
            format!(
                "docker {} -- {}",
                if matches!(action, ManagementAction::DockerLogs(_)) {
                    "logs --tail 200"
                } else {
                    "inspect"
                },
                quote(id)
            )
        }
        ManagementAction::StopProcesses(pids) => {
            ensure!(
                !pids.is_empty() && pids.iter().all(|p| *p > 1),
                "无效进程 PID"
            );
            format!(
                "kill -9 -- {}",
                pids.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        ManagementAction::AllowPort { port, udp } => {
            ensure!(*port > 0, "无效端口");
            let proto = if *udp { "udp" } else { "tcp" };
            format!(
                "if command -v firewall-cmd >/dev/null 2>&1 && systemctl is-active --quiet firewalld; then sudo -n firewall-cmd --permanent --add-port={port}/{proto} && sudo -n firewall-cmd --reload; else sudo -n iptables -C INPUT -p {proto} --dport {port} -j ACCEPT 2>/dev/null || sudo -n iptables -I INPUT -p {proto} --dport {port} -j ACCEPT; fi"
            )
        }
        ManagementAction::Service { action, name } => {
            ensure!(
                [
                    "start", "stop", "restart", "enable", "disable", "status", "logs"
                ]
                .contains(&action.as_str())
                    && identifier(name),
                "无效服务操作"
            );
            if action == "logs" {
                format!("journalctl --no-pager -n 200 -u {}", quote(name))
            } else if action == "status" {
                format!("systemctl --no-pager status -- {}", quote(name))
            } else {
                format!("sudo -n systemctl {action} -- {}", quote(name))
            }
        }
        ManagementAction::InitializePv(path) => {
            ensure!(device(path), "无效块设备路径");
            let script = format!(
                "set -eu; d={}; test -b \"$d\"; test -z \"$(lsblk -nr -o MOUNTPOINT \"$d\" | grep -v '^$' || true)\"; wipefs -a \"$d\"; parted -s \"$d\" mklabel gpt mkpart primary 1MiB 100%; partprobe \"$d\"; sleep 2; case \"$d\" in *[0-9]) p=\"${{d}}p1\";; *) p=\"${{d}}1\";; esac; pvcreate -ff -y \"$p\"",
                quote(path)
            );
            format!("sudo -n sh -c {}", quote(&script))
        }
        ManagementAction::VolumeGroup { pv, vg } => {
            ensure!(
                device(pv) && identifier(vg) && !vg.contains('/'),
                "无效 PV/VG"
            );
            format!(
                "sudo -n sh -c {}",
                quote(&format!(
                    "set -eu; pv={}; vg={}; test -b \"$pv\"; pvs \"$pv\" >/dev/null 2>&1 || pvcreate \"$pv\"; if vgs \"$vg\" >/dev/null 2>&1; then vgextend \"$vg\" \"$pv\"; else vgcreate \"$vg\" \"$pv\"; fi",
                    quote(pv),
                    quote(vg)
                ))
            )
        }
        ManagementAction::ExtendLv {
            device: path,
            amount,
        } => {
            ensure!(
                device(path) && valid_amount(amount),
                "容量应为 +10G 或 +100%FREE"
            );
            format!(
                "sudo -n lvextend -r {} {} {}",
                if amount.ends_with("%FREE") {
                    "-l"
                } else {
                    "-L"
                },
                quote(amount),
                quote(path)
            )
        }
        ManagementAction::Mount {
            device: path,
            directory,
            filesystem,
            persist,
        } => {
            ensure!(
                device(path)
                    && directory.starts_with('/')
                    && !directory.contains(['\n', '\r', '\0'])
                    && !directory.split('/').any(|v| v == ".."),
                "无效设备或挂载点"
            );
            ensure!(
                filesystem
                    .as_ref()
                    .is_none_or(|s| s == "ext4" || s == "xfs"),
                "文件系统应为 ext4/xfs"
            );
            let mkfs = filesystem
                .as_ref()
                .map(|s| {
                    format!(
                        "mkfs.{s} {} \"$dev\"; ",
                        if s == "ext4" { "-F" } else { "-f" }
                    )
                })
                .unwrap_or_default();
            let fstab = if *persist {
                let escaped = directory
                    .replace('\\', "\\134")
                    .replace(' ', "\\040")
                    .replace('\t', "\\011");
                format!(
                    "fstab_dir={}; uuid=$(blkid -s UUID -o value \"$dev\"); test -n \"$uuid\"; grep -q \"UUID=$uuid \" /etc/fstab || printf 'UUID=%s %s auto defaults,nofail 0 2\\n' \"$uuid\" \"$fstab_dir\" >> /etc/fstab; ",
                    quote(&escaped)
                )
            } else {
                String::new()
            };
            format!(
                "sudo -n sh -c {}",
                quote(&format!(
                    "set -eu; dev={}; dir={}; {mkfs}mkdir -p \"$dir\"; mount \"$dev\" \"$dir\"; {fstab}",
                    quote(path),
                    quote(directory)
                ))
            )
        }
    };
    Ok(
        if matches!(
            action,
            ManagementAction::Docker { .. }
                | ManagementAction::DockerLogs(_)
                | ManagementAction::DockerInspect(_)
                | ManagementAction::DockerUpdate { .. }
        ) {
            format!(
                "if sudo -n docker version >/dev/null 2>&1; then sudo -n {command}; else {command}; fi"
            )
        } else {
            command
        },
    )
}
fn valid_amount(value: &str) -> bool {
    value.strip_prefix('+').is_some_and(|v| {
        let n = v.bytes().take_while(u8::is_ascii_digit).count();
        n > 0 && ["K", "M", "G", "T", "KB", "MB", "GB", "TB", "%FREE"].contains(&&v[n..])
    })
}
pub fn parse(section: Section, text: &str) -> Table {
    let (columns, rows, detail) = match section {
        Section::Docker => (
            vec!["容器ID", "镜像", "名称", "状态", "端口", "创建时间"],
            text.lines()
                .filter_map(|l| {
                    let p: Vec<_> = l.split('|').map(str::to_string).collect();
                    (p.len() == 6).then_some(p)
                })
                .collect(),
            String::new(),
        ),
        Section::Ports => {
            let (raw, firewall) = text.split_once("__TOMATO_FIREWALL__").unwrap_or((text, ""));
            (
                vec!["协议", "本地地址", "端口", "PID", "进程"],
                raw.lines().filter_map(parse_port).collect(),
                firewall.trim().into(),
            )
        }
        Section::Services => (
            vec!["服务", "加载", "活动", "状态", "描述"],
            text.lines()
                .filter_map(|l| {
                    let p: Vec<_> = l.split_whitespace().collect();
                    (p.len() >= 4).then(|| {
                        vec![
                            p[0].into(),
                            p[1].into(),
                            p[2].into(),
                            p[3].into(),
                            p[4..].join(" "),
                        ]
                    })
                })
                .collect(),
            String::new(),
        ),
        Section::Storage => {
            let (raw, detail) = text.split_once("__TOMATO_PVS__").unwrap_or((text, ""));
            let rows = raw
                .lines()
                .filter_map(|l| {
                    let fields = lsblk_fields(l);
                    let path = fields.get("PATH")?;
                    let get = |k| fields.get(k).cloned().unwrap_or_default();
                    let state = if !get("MOUNTPOINT").is_empty() {
                        "已挂载"
                    } else if !get("FSTYPE").is_empty() {
                        "已格式化"
                    } else if get("TYPE") == "disk" && get("PTTYPE").is_empty() {
                        "未分区"
                    } else {
                        "未格式化"
                    };
                    Some(vec![
                        path.clone(),
                        get("TYPE"),
                        get("SIZE"),
                        get("PTTYPE"),
                        get("FSTYPE"),
                        get("MOUNTPOINT"),
                        state.into(),
                    ])
                })
                .collect();
            (
                vec![
                    "设备",
                    "类型",
                    "容量",
                    "分区表",
                    "文件系统",
                    "挂载点",
                    "状态",
                ],
                rows,
                detail.trim().into(),
            )
        }
        _ => (vec![], vec![], text.into()),
    };
    Table {
        section,
        columns: columns.into_iter().map(str::to_string).collect(),
        rows,
        detail,
    }
}
fn parse_port(line: &str) -> Option<Vec<String>> {
    let p: Vec<_> = line.split_whitespace().collect();
    if p.len() < 5 || !(p[0].starts_with("tcp") || p[0].starts_with("udp")) {
        return None;
    }
    let endpoint = if p[1].parse::<u64>().is_ok() { 3 } else { 4 };
    let (addr, port) = p.get(endpoint)?.rsplit_once(':')?;
    let process = if let Some(s) = line.split("pid=").nth(1) {
        let pid = s
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let name = line.split('"').nth(1).unwrap_or("");
        (pid, name.to_string())
    } else {
        p.iter()
            .find_map(|v| {
                v.split_once('/')
                    .filter(|(id, _)| id.parse::<u32>().is_ok())
            })
            .map(|(a, b)| (a.into(), b.into()))
            .unwrap_or_default()
    };
    Some(vec![
        p[0].into(),
        addr.into(),
        port.into(),
        process.0,
        process.1,
    ])
}
fn lsblk_fields(line: &str) -> std::collections::BTreeMap<String, String> {
    let mut fields = std::collections::BTreeMap::new();
    let mut rest = line;
    while let Some((key, after)) = rest.split_once("=\"") {
        let key = key.split_whitespace().last().unwrap_or("");
        let mut value = String::new();
        let mut escaped = false;
        let mut end = after.len();
        for (i, c) in after.char_indices() {
            if escaped {
                value.push(c);
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                end = i + 1;
                break;
            } else {
                value.push(c);
            }
        }
        fields.insert(key.to_string(), value);
        rest = &after[end..];
    }
    fields
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Whitelist {
    Direct,
    Service,
}
pub fn whitelist(detail: &str, protocol: &str, port: u16) -> Option<Whitelist> {
    let mut service = false;
    for line in detail.lines() {
        if line.contains("__TOMATO_SERVICE__") {
            service = true;
            continue;
        }
        let tokens: Vec<_> = line.split_whitespace().collect();
        let port_matches = |value: &str| {
            value.split(',').any(|part|{let (low,high)=part.split_once(['-',':']).unwrap_or((part,part));matches!((low.parse::<u16>(),high.parse::<u16>()),(Ok(low),Ok(high))if low<=port&&port<=high)})
        };
        if tokens
            .iter()
            .filter_map(|t| t.split_once('/'))
            .any(|(p, proto)| proto == protocol && port_matches(p))
        {
            return Some(if service {
                Whitelist::Service
            } else {
                Whitelist::Direct
            });
        }
        if line.contains("ACCEPT")
            && tokens.windows(2).any(|p| p[0] == "-p" && p[1] == protocol)
            && tokens
                .windows(2)
                .any(|p| matches!(p[0], "--dport" | "--dports") && port_matches(p[1]))
        {
            return Some(Whitelist::Direct);
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_firewall_protocol_ranges_and_service_rules() {
        assert_eq!(
            whitelist("ports: 20-22/tcp 53/udp", "tcp", 22),
            Some(Whitelist::Direct)
        );
        assert_eq!(whitelist("ports: 53/udp", "tcp", 53), None);
        assert_eq!(
            whitelist("__TOMATO_SERVICE__\nports: 443/tcp", "tcp", 443),
            Some(Whitelist::Service)
        );
        assert_eq!(
            whitelist("-A INPUT -p tcp --dports 80,443 -j ACCEPT", "tcp", 443),
            Some(Whitelist::Direct)
        );
    }
    #[test]
    fn management_inputs_cannot_inject_shell_commands() {
        assert!(
            command(&ManagementAction::Docker {
                action: "start".into(),
                ids: vec!["x;whoami".into()]
            })
            .is_err()
        );
        assert!(
            command(&ManagementAction::InitializePv(
                "/dev/../../etc/passwd".into()
            ))
            .is_err()
        );
        assert!(
            command(&ManagementAction::DockerUpdate {
                id: "abc".into(),
                restart: None,
                memory: Some(u64::MAX),
                memory_swap: None,
                cpus: None,
                shares: None
            })
            .is_err()
        );
        assert!(
            command(&ManagementAction::DockerUpdate {
                id: "abc".into(),
                restart: None,
                memory: None,
                cpus: Some(f64::NAN),
                memory_swap: None,
                shares: None
            })
            .is_err()
        );
    }
    #[test]
    fn persistent_mounts_encode_fstab_whitespace_without_changing_the_mount_target() {
        let action = ManagementAction::Mount {
            device: "/dev/data/lv".into(),
            directory: "/data/a b\\c\tpart".into(),
            filesystem: None,
            persist: true,
        };
        let script = command(&action).unwrap();
        assert!(script.contains(r"/data/a\040b\134c\011part"));
        assert!(script.contains("\"$fstab_dir\" >> /etc/fstab"));
        assert!(script.contains("mount \"$dev\" \"$dir\""));
        assert!(!script.contains("mkfs."));
        let ManagementAction::Mount {
            device,
            directory,
            filesystem,
            ..
        } = action
        else {
            unreachable!()
        };
        let temporary = command(&ManagementAction::Mount {
            device,
            directory,
            filesystem,
            persist: false,
        })
        .unwrap();
        assert!(!temporary.contains("/etc/fstab"));
    }
}
