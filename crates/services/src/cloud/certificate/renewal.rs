//! Profile-local certificate inventory and one hourly renewal task.
use super::*;
use chrono::Utc;
use std::{
    fs::{File, OpenOptions},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tomato_core::Store;

const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
const RENEW_BEFORE: i64 = 3 * 24 * 60 * 60;
const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub auto_renew: bool,
    pub deployment: Option<Deployment>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    pub ssh_id: String,
    pub directory: String,
    pub reload_command: String,
}

pub fn valid_directory(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('/')
        && !value.trim_matches('/').is_empty()
        && value.len() <= 1024
        && !value.chars().any(char::is_control)
        && !value.split('/').any(|part| matches!(part, "." | ".."))
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Failure {
    Issue,
    Renewal,
    Deployment,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    dns_id: String,
    request: Request,
    options: Options,
    directory_url: String,
    account: Option<Value>,
    expires_at: Option<i64>,
    pending: Option<Material>,
    failure: Option<Failure>,
}

impl Entry {
    fn due(&self, now: i64) -> bool {
        self.options.auto_renew && self.expires_at.is_some_and(|end| end <= now + RENEW_BEFORE)
    }

    fn status(&self, now: i64) -> String {
        let mut status = match self.expires_at {
            Some(end) if end <= now => "已过期",
            Some(end) if end <= now + RENEW_BEFORE => "即将到期",
            Some(_) => "已签发",
            None if self.failure == Some(Failure::Issue) => "申请失败",
            None => "未申请",
        }
        .to_owned();
        if let Some(note) = match self.failure {
            Some(Failure::Renewal) => Some("续期失败"),
            Some(Failure::Deployment) => Some("部署失败"),
            _ if self.pending.is_some() => Some("待部署"),
            _ if self.expires_at.is_some() && self.options.deployment.is_some() => Some("已部署"),
            _ => None,
        } {
            status.push_str(" · ");
            status.push_str(note);
        }
        status
    }
}

struct Repository(PathBuf);
impl Repository {
    fn new(profile: &Path) -> Self {
        Self(profile.with_file_name("certificate-renewals.json"))
    }

    fn lock(&self) -> Result<Option<File>> {
        let parent = self
            .0
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.0.with_extension("json.rust-lock"))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn load(&self) -> Result<Vec<Entry>> {
        let file = match File::open(&self.0) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = String::new();
        file.take(MAX_BYTES as u64 + 1).read_to_string(&mut bytes)?;
        ensure!(bytes.len() <= MAX_BYTES, "证书续期配置过大");
        // Avoid including stored values in parse errors exposed to the UI.
        tomato_core::legacy::decode_encrypted_json(&bytes)
            .map_err(|_| anyhow::anyhow!("无法读取加密的证书续期配置"))
    }

    async fn save(self, entries: Vec<Entry>) -> Result<Vec<Entry>> {
        tokio::task::spawn_blocking(move || {
            let text = tomato_core::legacy::encode_json(&entries)?;
            ensure!(text.len() <= MAX_BYTES, "证书续期配置过大");
            tomato_core::config::atomic_write(&self.0, text.as_bytes())?;
            Ok(entries)
        })
        .await?
    }
}

pub async fn issue(
    profile: PathBuf,
    connection: Connection,
    mut request: Request,
    mut options: Options,
    control: &Control,
) -> Result<String> {
    run_owned(control, |control| async move {
        request.domain = normalize_domain(&request.domain)?;
        if let Some(deployment) = &mut options.deployment {
            ensure!(
                valid_directory(&deployment.directory),
                "请填写有效的远程绝对目录"
            );
            ensure!(
                deployment.reload_command.len() <= 4096
                    && !deployment.reload_command.contains('\0'),
                "部署后命令无效"
            );
            deployment.directory = format!(
                "/{}",
                deployment
                    .directory
                    .trim()
                    .split('/')
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join("/")
            );
        }
        let (repo, _lock, mut entries, ssh) = tokio::task::spawn_blocking({
            let profile = profile.clone();
            let deployment = options.deployment.clone();
            move || -> Result<_> {
                let repo = Repository::new(&profile);
                let lock = repo.lock()?.context("另一项证书任务正在运行，请稍后重试")?;
                let entries = repo.load()?;
                let ssh = deployment
                    .as_ref()
                    .map(|d| {
                        let store = Store::load(&profile)?;
                        ssh_connection(&store, d).cloned()
                    })
                    .transpose()?;
                Ok((repo, lock, entries, ssh))
            }
        })
        .await??;
        let index = entries
            .iter()
            .position(|e| e.dns_id == connection.id && e.request.domain == request.domain);
        if let Some(deployment) = &options.deployment {
            ensure!(
                !entries.iter().enumerate().any(|(i, e)| Some(i) != index
                    && e.options
                        .deployment
                        .as_ref()
                        .is_some_and(|d| d.ssh_id == deployment.ssh_id
                            && d.directory == deployment.directory)),
                "该 SSH 目录已用于另一张证书，请指定独立目录"
            );
        }
        let url = directory_url(&connection, &request).to_owned();
        let mut account = index.and_then(|i| {
            let entry = &entries[i];
            (entry.directory_url == url && entry.request.email.trim() == request.email.trim())
                .then(|| entry.account.clone())
                .flatten()
        });
        let result = issue_owned(
            &connection,
            request.clone(),
            ssh.is_some(),
            &mut account,
            &control,
        )
        .await;
        let mut stored_request = request;
        stored_request.storage.options.clear(); // Renew using the current saved connection and credentials.
        stored_request.eab_kid.clear();
        stored_request.eab_hmac.clear();
        let mut entry = Entry {
            dns_id: connection.id,
            request: stored_request,
            options,
            directory_url: url,
            account,
            expires_at: None,
            pending: None,
            failure: None,
        };
        match result {
            Ok(issued) => {
                let message = accept(&mut entry, issued);
                let index = replace(&mut entries, index, entry);
                deploy_and_save(&profile, entries, index, ssh, &control, message).await
            }
            Err(error) => {
                if !control.cancelled() && index.is_none_or(|i| entries[i].expires_at.is_none()) {
                    entry.failure = Some(Failure::Issue);
                    replace(&mut entries, index, entry);
                    repo.save(entries).await?;
                }
                Err(error)
            }
        }
    })
    .await
}

fn replace(entries: &mut Vec<Entry>, index: Option<usize>, entry: Entry) -> usize {
    if let Some(index) = index {
        entries[index] = entry;
        index
    } else {
        let index = entries.len();
        entries.push(entry);
        index
    }
}

fn accept(entry: &mut Entry, issued: Issued) -> String {
    entry.expires_at = Some(issued.expires_at);
    entry.pending = issued.material;
    entry.failure = None;
    match issued.cleanup_warning {
        Some(error) => format!("{}，{error}", issued.message),
        None => issued.message,
    }
}

fn ssh_connection<'a>(store: &'a Store, target: &Deployment) -> Result<&'a Connection> {
    store
        .connections
        .iter()
        .find(|c| c.id == target.ssh_id && matches!(c.kind(), "SSH" | "SFTP"))
        .context("部署所用的 SSH 连接不存在")
}

async fn deploy_and_save(
    profile: &Path,
    mut entries: Vec<Entry>,
    index: usize,
    ssh: Option<Connection>,
    control: &Control,
    mut message: String,
) -> Result<String> {
    // Commit the issuance before attempting SSH. A restart can retry exactly this certificate.
    let repo = Repository::new(profile);
    if entries[index].pending.is_some() {
        // Persist on the blocking pool without copying the certificate material into the UI.
        entries = repo.save(entries).await?;
        let entry = &mut entries[index];
        if let (Some(deployment), Some(material)) = (&entry.options.deployment, &entry.pending) {
            let published = async {
                let ssh = ssh.context("部署所用的 SSH 连接不存在")?;
                control.check()?;
                control.log("正在通过 SSH 发布证书…");
                crate::terminal::ssh::publish::certificate(
                    &ssh,
                    profile.with_file_name("known_hosts"),
                    &deployment.directory,
                    &deployment.reload_command,
                    material.chain.as_bytes(),
                    material.key.as_bytes(),
                )
                .await
            }
            .await;
            match published {
                Ok(()) => {
                    entry.pending = None;
                    entry.failure = None;
                    message.push_str(&format!("，已部署到 {}/current", deployment.directory));
                }
                Err(error) => {
                    entry.failure = Some(Failure::Deployment);
                    message.push_str(&format!("，部署待重试：{error:#}"));
                }
            }
        }
    }
    Repository::new(profile).save(entries).await?;
    Ok(message)
}

async fn check(profile: &Path, report: &(impl Fn(String) + Sync)) -> Result<()> {
    let profile = profile.to_owned();
    // An empty profile needs no lock file, connection read, or network client.
    let work = tokio::task::spawn_blocking({
        let profile = profile.clone();
        move || -> Result<_> {
            let repo = Repository::new(&profile);
            let now = Utc::now().timestamp();
            let due = repo
                .load()?
                .into_iter()
                .filter(|e| e.due(now) || e.pending.is_some())
                .map(|e| (e.dns_id, e.request.domain))
                .collect::<Vec<_>>();
            Ok(due)
        }
    })
    .await??;
    for (dns_id, domain) in work {
        let control = Control::default();
        let result = run_owned(&control, |control| {
            check_one(profile.clone(), dns_id, domain.clone(), control)
        })
        .await;
        match result {
            Ok(Some(message)) => report(format!("{domain}：{message}")),
            Ok(None) => {}
            Err(error) => report(format!("{domain}：证书检查失败，{error}")),
        }
    }
    Ok(())
}

async fn check_one(
    profile: PathBuf,
    dns_id: String,
    domain: String,
    control: Control,
) -> Result<Option<String>> {
    let work = tokio::task::spawn_blocking({
        let profile = profile.clone();
        move || -> Result<_> {
            let repo = Repository::new(&profile);
            let Some(lock) = repo.lock()? else {
                return Ok(None);
            };
            let entries = repo.load()?;
            let Some(index) = entries
                .iter()
                .position(|e| e.dns_id == dns_id && e.request.domain == domain)
            else {
                return Ok(None);
            };
            let due = entries[index].due(Utc::now().timestamp());
            if !due && entries[index].pending.is_none() {
                return Ok(None);
            }
            Ok(Some((lock, entries, index, due, Store::load(&profile)?)))
        }
    })
    .await??;
    let Some((_lock, mut entries, index, due, store)) = work else {
        return Ok(None);
    };
    let entry = &mut entries[index];
    let ssh = entry
        .options
        .deployment
        .as_ref()
        .and_then(|target| ssh_connection(&store, target).ok())
        .cloned();
    let mut message = "证书部署重试".to_owned();
    if due {
        let renewed = renew(entry, &store, &control).await;
        match renewed {
            Ok(issued) => message = accept(entry, issued),
            Err(error) if !control.cancelled() => {
                entry.failure = Some(Failure::Renewal);
                Repository::new(&profile).save(entries).await?;
                return Ok(Some(format!("证书续期失败，将在下次检查时重试：{error}")));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Some(
        deploy_and_save(&profile, entries, index, ssh, &control, message).await?,
    ))
}

async fn renew(entry: &mut Entry, store: &Store, control: &Control) -> Result<Issued> {
    let dns = store
        .connections
        .iter()
        .find(|c| c.id == entry.dns_id)
        .context("找不到证书所用的 DNS 连接")?;
    let mut request = entry.request.clone();
    if !request.bucket.trim().is_empty() {
        request.storage = store
            .connections
            .iter()
            .find(|c| c.id == request.storage.id)
            .context("找不到证书所用的 S3/OSS 连接")?
            .clone();
    }
    ensure!(
        directory_url(dns, &request) == entry.directory_url,
        "证书机构地址已更改，请重新申请"
    );
    issue_owned(
        dns,
        request,
        entry.options.deployment.is_some(),
        &mut entry.account,
        control,
    )
    .await
}

pub struct Runner {
    task: tokio::task::JoinHandle<()>,
    notice: Arc<Mutex<Option<String>>>,
}

impl Runner {
    pub fn start(
        profile: PathBuf,
        runtime: &tokio::runtime::Handle,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self::with_interval(profile, runtime, CHECK_INTERVAL, wake)
    }

    pub fn with_interval(
        profile: PathBuf,
        runtime: &tokio::runtime::Handle,
        interval: Duration,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let notice = Arc::new(Mutex::new(None));
        let notify = notice.clone();
        let task = runtime.spawn(async move {
            let report = |message| {
                if let Ok(mut notice) = notify.lock() {
                    *notice = Some(message);
                }
                wake();
            };
            let mut ticks = tokio::time::interval(interval);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticks.tick().await; // Tokio's first tick is immediate.
                if let Err(error) = check(&profile, &report).await {
                    report(format!("证书检查失败：{error}"));
                }
            }
        });
        Self { task, notice }
    }

    pub fn take_notice(&self) -> Option<String> {
        self.notice.lock().ok()?.take()
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) fn annotate(
    profile: &Path,
    connection_id: &str,
    zone: Option<&str>,
    rows: &mut [Value],
) {
    let loaded = Repository::new(profile).load();
    let now = Utc::now().timestamp();
    let inventory: BTreeMap<_, _> = loaded
        .as_ref()
        .ok()
        .into_iter()
        .flatten()
        .filter(|e| e.dns_id == connection_id)
        .map(|e| (e.request.domain.as_str(), e))
        .collect();
    for row in rows {
        let name = match zone {
            Some(zone) => crate::cloud::certificate_domain(&text(row, "rr"), zone),
            None => text(row, "domainName"),
        };
        let domain = normalize_domain(&name).ok();
        let entry = domain.as_deref().and_then(|domain| inventory.get(domain));
        let status = if loaded.is_err() {
            "读取失败".to_owned()
        } else {
            entry.map_or_else(|| "未申请".into(), |e| e.status(now))
        };
        let expires = entry
            .and_then(|e| e.expires_at)
            .and_then(|end| chrono::DateTime::from_timestamp(end, 0))
            .map_or_else(String::new, |end| {
                end.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M %:z")
                    .to_string()
            });
        row["certificateStatus"] = json!(status);
        row["certificateExpiry"] = json!(expires);
    }
}
