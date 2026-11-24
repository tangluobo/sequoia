use super::*;
use anyhow::{Result, ensure};
use reqwest::Method;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Default)]
pub struct View {
    pub accounts: Vec<Account>,
    pub push: bool,
    pub user: Option<User>,
    pub online: bool,
    pub projects: Vec<Project>,
    pub selected: String,
    pub items: Vec<Cached>,
    pub document: Option<Arc<Object>>,
    pub remote: Option<Arc<Object>>,
    pub detail: Value,
    pub message: String,
    pub busy: bool,
    pub saved: std::collections::VecDeque<(String, String)>,
    pub accepted: std::collections::VecDeque<String>,
    pub failed: Option<(String, String)>,
    pub bindings: Vec<(String, Object)>,
    pub local_origins: std::collections::HashSet<String>,
    pub local_accepted: std::collections::VecDeque<(String, String)>,
    pub baselines: std::collections::HashMap<String, String>,
    pub personal: Option<Arc<Object>>,
}
pub enum ProjectAction {
    Create(String),
    Rename(Project, String),
    Delete(String),
    Leave(String),
    Invite {
        project: String,
        email: String,
        role: Role,
    },
    Member {
        project: String,
        member: String,
        role: Option<Role>,
    },
    Invitation {
        id: String,
        accept: bool,
    },
    CancelInvitation {
        project: String,
        id: String,
    },
}
pub enum Detail {
    Members(String),
    Invitations,
    Activity(String),
    History(String, String),
    Account,
    Version {
        project: String,
        id: String,
        revision: i64,
    },
}
pub enum Command {
    Unload,
    BindMany(Vec<(String, String)>),
    Delete(Vec<String>),
    Push(bool),
    ClearCache,
    Connect {
        base: String,
        email: String,
        password: String,
        code: String,
    },
    Offline {
        base: String,
        email: String,
    },
    Register {
        base: String,
        email: String,
        name: String,
        password: String,
    },
    Disconnect,
    Select(String),
    Load(String),
    Save {
        project: String,
        id: String,
        mutation: Mutation,
    },
    Bind {
        id: String,
        local: String,
    },
    LocalConnection {
        id: String,
        name: String,
        body: Value,
    },
    Resolve {
        id: String,
        keep_local: bool,
    },
    Refresh,
    Project(ProjectAction),
    Detail(Detail),
    Transfer {
        project: String,
        objects: Vec<String>,
        target: String,
        copy: bool,
    },
    Export {
        id: String,
        path: PathBuf,
    },
    Import {
        project: String,
        path: PathBuf,
        kind: Kind,
        replace: Option<(String, i64)>,
    },
}
pub struct Client {
    sender: mpsc::Sender<Command>,
    receiver: watch::Receiver<Arc<View>>,
    task: tokio::task::JoinHandle<()>,
}
impl Client {
    pub fn start(
        runtime: &tokio::runtime::Handle,
        root: PathBuf,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (sender, commands) = mpsc::channel(32);
        let (tx, receiver) = watch::channel(Arc::new(View::default()));
        let task = runtime.spawn(
            Worker {
                root,
                api: None,
                repository: None,
                view: View::default(),
                tx,
                repaint: Arc::new(repaint),
                detail_path: None,
            }
            .run(commands),
        );
        Self {
            sender,
            receiver,
            task,
        }
    }
    pub fn send(&self, command: Command) -> Result<()> {
        self.sender
            .try_send(command)
            .map_err(|_| anyhow::anyhow!("云端操作队列繁忙，请稍后重试"))
    }
    pub fn dispatcher(&self) -> mpsc::Sender<Command> {
        self.sender.clone()
    }
    pub fn changed(&mut self) -> Option<Arc<View>> {
        if self.receiver.has_changed().unwrap_or(false) {
            Some(self.receiver.borrow_and_update().clone())
        } else {
            None
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct Worker {
    detail_path: Option<String>,
    root: PathBuf,
    api: Option<Api>,
    repository: Option<Repository>,
    view: View,
    tx: watch::Sender<Arc<View>>,
    repaint: Arc<dyn Fn() + Send + Sync>,
}
impl Worker {
    fn publish(&self) {
        self.tx.send_replace(Arc::new(self.view.clone()));
        (self.repaint)();
    }
    fn repo(&self) -> Result<&Repository> {
        self.repository
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("请先登录或打开离线副本"))
    }
    fn api(&self) -> Result<&Api> {
        self.api
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("当前为离线状态，请登录云服务"))
    }
    async fn open_cache(&mut self, base: &str, email: &str) -> Result<()> {
        let canonical = Api::new(base)?;
        let hash = crate::queries::stamp(
            format!("{}\n{}", canonical.base(), email.trim().to_lowercase()).as_bytes(),
        );
        // Hexadecimal content fingerprints prevent server/email path traversal.
        let hash = hash
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>();
        self.repository = Some(Repository::open(&self.root.join(format!("{hash}.sqlite"))).await?);
        self.view.projects = self.repo()?.projects().await?;
        self.view.items.clear();
        self.view.document = None;
        self.view.remote = None;
        self.view.selected.clear();
        Ok(())
    }
    async fn run(mut self, mut commands: mpsc::Receiver<Command>) {
        self.view.push = true;
        match super::accounts::read(&self.root).await {
            Ok(accounts) => self.view.accounts = accounts,
            Err(e) => self.view.message = e.to_string(),
        };
        self.publish();
        let mut retry = tokio::time::Instant::now();
        loop {
            enum Wake {
                Command(Option<Command>),
                Sync,
                Notification(Result<Events>),
            }
            let wake = if let (Some(api), Some(repo)) = (&self.api, &self.repository) {
                let cursor = repo.cursor().await.unwrap_or(0);
                tokio::select! { biased;
                    command=commands.recv()=>Wake::Command(command),
                    _=tokio::time::sleep_until(retry)=>Wake::Sync,
                    events=api.events(cursor), if self.view.online&&self.view.push=>Wake::Notification(events),
                }
            } else {
                Wake::Command(commands.recv().await)
            };
            let command = match wake {
                Wake::Command(Some(command)) => command,
                Wake::Command(None) => break,
                Wake::Sync => {
                    match self.synchronize().await {
                        Ok(()) => {
                            self.view.online = true;
                            self.view.message = "同步完成".into();
                            retry = tokio::time::Instant::now()
                                + if self.repo().is_ok()
                                    && !self
                                        .repo()
                                        .unwrap()
                                        .pending()
                                        .await
                                        .unwrap_or_default()
                                        .is_empty()
                                {
                                    Duration::from_millis(10)
                                } else {
                                    Duration::from_secs(3600)
                                };
                        }
                        Err(error) => {
                            self.cloud_error(error);
                            retry = tokio::time::Instant::now() + Duration::from_secs(10);
                        }
                    }
                    if let Err(error) = self.refresh_view().await {
                        self.view.message = error.to_string();
                    }
                    self.publish();
                    continue;
                }
                Wake::Notification(Ok(events)) => {
                    if events.events.is_empty() {
                        let _ = self.repo().unwrap().set_cursor(events.cursor).await;
                    } else {
                        retry = tokio::time::Instant::now();
                    }
                    continue;
                }
                Wake::Notification(Err(error)) => {
                    self.cloud_error(error);
                    self.publish();
                    retry = tokio::time::Instant::now() + Duration::from_secs(2);
                    continue;
                }
            };
            self.view.busy = true;
            self.publish();
            let saving = match &command {
                Command::Save { id, .. } => Some(id.clone()),
                _ => None,
            };
            let saved = matches!(
                command,
                Command::Save { .. }
                    | Command::Delete(_)
                    | Command::LocalConnection { .. }
                    | Command::Resolve { .. }
                    | Command::Import { .. }
            );
            if let Err(error) = self.execute(command).await {
                self.view.message = format!("{error:#}");
                if let Some(id) = saving {
                    self.view.failed = Some((id, format!("{error:#}")));
                }
            }
            self.view.busy = false;
            if let Err(error) = self.refresh_view().await {
                self.view.message = format!("{error:#}");
            }
            self.publish();
            if saved {
                retry = tokio::time::Instant::now();
            }
        }
    }
    fn cloud_error(&mut self, error: anyhow::Error) {
        if error
            .downcast_ref::<HttpError>()
            .is_some_and(|e| e.status == 401)
        {
            self.api = None;
            self.view.user = None;
        }
        self.view.online = false;
        self.view.message = format!("{error:#}；本地修改已保留");
    }
    async fn synchronize(&mut self) -> Result<()> {
        let api = self.api()?.clone();
        let projects = api.projects().await?;
        self.repo()?.set_projects(&projects).await?;
        self.view.projects = projects;
        self.repo()?.retry_blocked().await?;
        self.reconcile_bindings().await?;
        // Drain one document at a time; never collect large pending bodies into RAM.
        for _ in 0..32 {
            let pending = self.repo()?.pending().await?;
            let Some((id, pid, mutation)) = pending.into_iter().next() else {
                break;
            };
            let response = if pid == "@personal" {
                api.request(Method::PUT, "/profile", Some(&mutation)).await
            } else {
                api.put(&pid, &id, &mutation).await
            };
            match response {
                Ok(object) => self.repo()?.receive(object).await?,
                Err(error) => {
                    if error
                        .downcast_ref::<HttpError>()
                        .is_some_and(|e| e.status == 409)
                    {
                        let object = if pid == "@personal" {
                            api.get("/profile").await?
                        } else {
                            api.object(&pid, &id).await?
                        };
                        self.repo()?.receive(object).await?;
                    } else if error
                        .downcast_ref::<HttpError>()
                        .is_some_and(|e| matches!(e.status, 400 | 403 | 404 | 413))
                    {
                        self.repo()?.block(&id, &error.to_string()).await?;
                    } else {
                        return Err(error);
                    }
                }
            }
        }
        let personal: Object = api.get("/profile").await?;
        self.repo()?.receive(personal).await?;
        for p in &self.view.projects {
            let mut after = String::new();
            loop {
                let page: Vec<Object> = api
                    .get(&format!(
                        "/projects/{}/objects?include_deleted=1&limit=200&after={after}",
                        p.id
                    ))
                    .await?;
                let done = page.len() < 200;
                for o in &page {
                    if self.repo()?.revision(&o.id).await? != Some(o.revision) {
                        self.repo()?
                            .receive(api.object(&p.id, &o.id).await?)
                            .await?;
                    }
                }
                if done {
                    break;
                }
                after = page.last().map(|o| o.id.clone()).unwrap_or_default();
            }
        }
        let mut cursor = self.repo()?.cursor().await?;
        loop {
            // Drain historical notifications without the 25-second idle wait.
            let events: Events = api.get(&format!("/events?cursor={cursor}&wait=0")).await?;
            for event in &events.events {
                if !event.object_id.is_empty()
                    && self.view.projects.iter().any(|p| p.id == event.project_id)
                {
                    let object = api.object(&event.project_id, &event.object_id).await;
                    match object {
                        Ok(o) => self.repo()?.receive(o).await?,
                        Err(e)
                            if e.downcast_ref::<HttpError>()
                                .is_some_and(|e| e.status == 404) => {}
                        Err(e) => return Err(e),
                    }
                }
            }
            cursor = events.cursor;
            self.repo()?.set_cursor(cursor).await?;
            if events.events.len() < 100 {
                break;
            }
        }
        Ok(())
    }
    async fn refresh_view(&mut self) -> Result<()> {
        if let Some(repo) = &self.repository {
            self.view.personal = repo.object("@personal").await.ok().map(Arc::new);
            self.view.items = repo.list(&self.view.selected).await?;
            if let Some(o) = &self.view.document {
                self.view.document = Some(Arc::new(repo.object(&o.id).await?));
                self.view.remote = repo
                    .remote(&self.view.document.as_ref().unwrap().id)
                    .await?
                    .map(Arc::new);
            }
            self.view.bindings.clear();
            self.view.local_origins.clear();
            self.view.baselines.clear();
            for (id, local, origin) in repo.bindings().await? {
                self.view
                    .baselines
                    .insert(id.clone(), repo.baseline(&id).await?);
                if origin {
                    self.view.local_origins.insert(id.clone());
                }
                let o = repo.object(&id).await?;
                if matches!(o.kind, Kind::Connection | Kind::Group) {
                    self.view.bindings.push((local, o));
                }
            }
        }
        Ok(())
    }
    async fn reconcile_bindings(&mut self) -> Result<()> {
        let bindings = self.repo()?.bindings().await?;
        if bindings.is_empty() {
            return Ok(());
        }
        let path = self
            .root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("云缓存目录缺少父目录"))?
            .join("connections.json");
        let store = tokio::task::spawn_blocking(move || tomato_core::Store::load(&path)).await??;
        for (id, local, _) in bindings {
            let Some(connection) = store.connections.iter().find(|c| c.id == local) else {
                continue;
            };
            let object = self.repo()?.object(&id).await?;
            let body = if object.kind == Kind::Group {
                object.body.clone()
            } else {
                connection_body_in_store(connection, &store)
            };
            let stamp = super::connection_stamp(&connection.name, &body);
            let baseline = self.repo()?.baseline(&id).await?;
            let cached = super::connection_stamp(&object.name, &object.body);
            if stamp != baseline && stamp != cached {
                if self
                    .view
                    .projects
                    .iter()
                    .any(|p| p.id == object.project_id && p.role.write())
                {
                    let mut mutation = Mutation::from_object(&object);
                    mutation.name = connection.name.clone();
                    mutation.body = body;
                    self.repo()?
                        .enqueue_local(&object.project_id, &id, mutation)
                        .await?;
                    self.repo()?.set_baseline(&id, &stamp).await?;
                    self.view.local_accepted.push_back((id, stamp));
                    while self.view.local_accepted.len() > 64 {
                        self.view.local_accepted.pop_front();
                    }
                } else {
                    self.repo()?
                        .block(&id, "项目只读或权限已撤销；本地连接修改保留在原配置中")
                        .await?;
                }
            } else if stamp == cached && stamp != baseline {
                self.repo()?.set_baseline(&id, &stamp).await?;
            }
        }
        Ok(())
    }
    async fn execute(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Unload => {
                self.view.document = None;
                self.view.remote = None;
                self.view.detail = Value::Null;
            }
            Command::BindMany(bindings) => {
                ensure!(bindings.len() <= 64, "分组超过 64 层");
                for (id, local) in bindings {
                    self.repo()?.bind(&id, &local).await?;
                }
                self.view.message = "连接与分组已接入本地连接树".into();
            }
            Command::Delete(ids) => {
                ensure!(ids.len() <= 200, "单次最多删除 200 个对象");
                for id in &ids {
                    let object = self.repo()?.object(id).await?;
                    let children = self.repo()?.list(&object.project_id).await?;
                    ensure!(
                        !children.iter().any(|c| c.object.parent_id == *id
                            && !c.object.deleted
                            && !ids.contains(&c.object.id)),
                        "请同时选中全部子对象，或先删除子对象"
                    );
                }
                for id in ids {
                    let object = self.repo()?.object(&id).await?;
                    ensure!(
                        self.view
                            .projects
                            .iter()
                            .any(|p| p.id == object.project_id && p.role.write()),
                        "项目只读或权限已撤销"
                    );
                    let mut mutation = Mutation::from_object(&object);
                    mutation.deleted = true;
                    self.repo()?
                        .enqueue(&object.project_id, &id, mutation)
                        .await?;
                }
                self.view.message = "删除操作已保存到本地待上传队列".into();
            }
            Command::Push(enabled) => {
                self.view.push = enabled;
                self.remember().await?;
            }
            Command::ClearCache => {
                ensure!(self.api.is_none(), "请先退出云端账号再清理离线副本");
                self.repo()?.clear().await?;
                self.view.projects.clear();
                self.view.document = None;
                self.view.remote = None;
                self.view.personal = None;
                self.view.message = "已清理本账号的云端离线副本，本地连接配置保留".into();
            }
            Command::Connect {
                base,
                email,
                password,
                code,
            } => {
                let api = Api::new(&base)?;
                let login = api.login(email, password, code).await?;
                self.open_cache(&base, &login.user.email).await?;
                self.view.push = self
                    .view
                    .accounts
                    .iter()
                    .find(|a| a.base == api.base() && a.email == login.user.email)
                    .is_none_or(|a| a.push);
                self.view.user = Some(login.user);
                self.api = Some(api.authenticated(login.token));
                self.remember().await?;
                self.synchronize().await?;
                self.view.online = true;
                self.view.message = "登录成功，同步完成".into();
            }
            Command::Offline { base, email } => {
                self.api = None;
                self.view.user = None;
                self.open_cache(&base, &email).await?;
                self.view.online = false;
                self.view.message = "离线副本；保存的修改将在登录后同步".into();
            }
            Command::Register {
                base,
                email,
                name,
                password,
            } => {
                let _: Value = Api::new(&base)?
                    .request(
                        Method::POST,
                        "/register",
                        Some(&json!({"email":email,"name":name,"password":password})),
                    )
                    .await?;
                self.view.message = "注册成功，请登录".into();
            }
            Command::Disconnect => {
                if let Some(api) = self.api.take() {
                    let _: Result<Value> =
                        api.request(Method::POST, "/logout", Some(&json!({}))).await;
                }
                self.view.online = false;
                self.view.user = None;
                self.view.message = "已退出，离线修改仍保存在本机".into();
            }
            Command::Select(project) => {
                self.detail_path = None;
                self.view.selected = project;
                self.view.document = None;
                self.view.remote = None;
                self.view.detail = Value::Null;
            }
            Command::Load(id) => {
                self.view.document = Some(Arc::new(self.repo()?.object(&id).await?));
                self.view.remote = self.repo()?.remote(&id).await?.map(Arc::new);
            }
            Command::Save {
                project,
                id,
                mutation,
            } => {
                ensure!(
                    project == "@personal"
                        || self
                            .view
                            .projects
                            .iter()
                            .any(|p| p.id == project && p.role.write()),
                    "当前项目只读或已没有访问权限"
                );
                let accepted = mutation.mutation_id.clone();
                let stamp = crate::queries::stamp(
                    mutation
                        .body
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .as_bytes(),
                );
                self.repo()?.enqueue(&project, &id, mutation).await?;
                self.view.failed = None;
                self.view.accepted.push_back(accepted);
                while self.view.accepted.len() > 64 {
                    self.view.accepted.pop_front();
                }
                self.view.saved.push_back((id, stamp));
                while self.view.saved.len() > 64 {
                    self.view.saved.pop_front();
                }
                self.view.message = "已保存到本地待上传队列".into();
            }
            Command::Bind { id, local } => {
                self.repo()?.bind(&id, &local).await?;
                self.view.message = "已接入本地连接树，密码保留在本机".into();
            }
            Command::LocalConnection { id, name, body } => {
                let stamp = super::connection_stamp(&name, &body);
                let object = self.repo()?.object(&id).await?;
                if object.body != body || object.name != name {
                    let mut mutation = Mutation::from_object(&object);
                    mutation.body = body;
                    mutation.name = name;
                    self.repo()?
                        .enqueue_local(&object.project_id, &id, mutation)
                        .await?;
                }
                self.view.local_accepted.push_back((id, stamp));
                while self.view.local_accepted.len() > 64 {
                    self.view.local_accepted.pop_front();
                }
            }
            Command::Resolve { id, keep_local } => {
                self.repo()?.resolve(&id, keep_local).await?;
                self.view.message = "冲突已处理".into();
            }
            Command::Refresh => self.synchronize().await?,
            Command::Project(action) => {
                let (method, path, body) = match action {
                    ProjectAction::Create(name) => {
                        (Method::POST, "/projects".into(), json!({"name":name}))
                    }
                    ProjectAction::Rename(p, name) => (
                        Method::PATCH,
                        format!("/projects/{}", p.id),
                        json!({"name":name,"revision":p.revision}),
                    ),
                    ProjectAction::Delete(id) => {
                        (Method::DELETE, format!("/projects/{id}"), Value::Null)
                    }
                    ProjectAction::Leave(id) => (
                        Method::DELETE,
                        format!(
                            "/projects/{id}/members/{}",
                            self.view
                                .user
                                .as_ref()
                                .ok_or_else(|| anyhow::anyhow!("请先登录"))?
                                .id
                        ),
                        Value::Null,
                    ),
                    ProjectAction::Invite {
                        project,
                        email,
                        role,
                    } => (
                        Method::POST,
                        format!("/projects/{project}/invitations"),
                        json!({"email":email,"role":role}),
                    ),
                    ProjectAction::Member {
                        project,
                        member,
                        role,
                    } => {
                        if let Some(role) = role {
                            (
                                Method::PUT,
                                format!("/projects/{project}/members/{member}"),
                                json!({"role":role}),
                            )
                        } else {
                            (
                                Method::DELETE,
                                format!("/projects/{project}/members/{member}"),
                                Value::Null,
                            )
                        }
                    }
                    ProjectAction::Invitation { id, accept } => (
                        Method::POST,
                        format!("/invitations/{id}"),
                        json!({"accept":accept}),
                    ),
                    ProjectAction::CancelInvitation { project, id } => (
                        Method::DELETE,
                        format!("/projects/{project}/invitations/{id}"),
                        Value::Null,
                    ),
                };
                let _: Value = self.api()?.request(method, &path, Some(&body)).await?;
                self.synchronize().await?;
                self.view.message = "云端操作完成".into();
                if let Some(path) = self.detail_path.clone() {
                    match self.api()?.get(&path).await {
                        Ok(value) => self.view.detail = value,
                        Err(error) => {
                            self.view.detail = Value::Null;
                            self.view.message = format!("操作已完成，详情刷新失败：{error:#}");
                        }
                    }
                }
            }
            Command::Detail(detail) => {
                let path = match detail {
                    Detail::Members(p) => format!("/projects/{p}/members"),
                    Detail::Invitations => "/invitations".into(),
                    Detail::Activity(p) => format!("/projects/{p}/activity?limit=200"),
                    Detail::History(p, id) => format!("/projects/{p}/objects/{id}/history"),
                    Detail::Account => "/account".into(),
                    Detail::Version {
                        project,
                        id,
                        revision,
                    } => format!("/projects/{project}/objects/{id}?revision={revision}"),
                };
                self.view.detail = self.api()?.get(&path).await?;
                self.detail_path = Some(path);
            }
            Command::Transfer {
                project,
                objects,
                target,
                copy,
            } => {
                let _: Value = self
                    .api()?
                    .request(
                        Method::POST,
                        &format!("/projects/{project}/transfer"),
                        Some(&json!({"objects":objects,"target":target,"copy":copy})),
                    )
                    .await?;
                self.synchronize().await?;
            }
            Command::Export { id, path } => {
                let object = self.repo()?.object(&id).await?;
                let body = object.body;
                if let Some(encoded) = body["base64"].as_str() {
                    use base64::Engine;
                    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
                    tokio::fs::write(path, bytes).await?;
                } else {
                    tokio::fs::write(path, serde_json::to_vec_pretty(&body)?).await?;
                }
                self.view.message = "对象已下载".into();
            }
            Command::Import {
                project,
                path,
                kind,
                replace,
            } => {
                use base64::Engine;
                let metadata = tokio::fs::metadata(&path).await?;
                ensure!(
                    metadata.len() <= 5 * 1024 * 1024,
                    "工作文件不能超过 5 MiB（Base64 后最大 8 MiB）"
                );
                use tokio::io::AsyncReadExt;
                let file = tokio::fs::File::open(&path).await?;
                let mut bytes = Vec::with_capacity(metadata.len() as usize);
                file.take(5 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .await?;
                ensure!(bytes.len() <= 5 * 1024 * 1024, "读取期间工作文件超过 5 MiB");
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| anyhow::anyhow!("文件名称无效"))?
                    .to_owned();
                let body = if matches!(kind, Kind::Query | Kind::Snippet | Kind::Pipeline) {
                    json!({"filename":name,"text":String::from_utf8(bytes)?})
                } else {
                    json!({"filename":name,"base64":base64::engine::general_purpose::STANDARD.encode(bytes)})
                };
                if let Some((id, revision)) = replace {
                    let object = self.repo()?.object(&id).await?;
                    ensure!(
                        object.project_id == project && object.kind == kind,
                        "更新的工作文件不属于当前对象"
                    );
                    let mut mutation = Mutation::from_object(&object);
                    mutation.base_revision = revision;
                    mutation.body = body;
                    self.repo()?.enqueue(&project, &id, mutation).await?;
                } else {
                    self.repo()?
                        .enqueue(
                            &project,
                            &uuid::Uuid::new_v4().to_string(),
                            Mutation {
                                parent_id: String::new(),
                                kind,
                                name,
                                body,
                                base_revision: 0,
                                mutation_id: uuid::Uuid::new_v4().to_string(),
                                deleted: false,
                            },
                        )
                        .await?;
                }
            }
        }
        Ok(())
    }
    async fn remember(&mut self) -> Result<()> {
        if let (Some(api), Some(user)) = (&self.api, &self.view.user) {
            let account = Account {
                base: api.base().into(),
                email: user.email.clone(),
                push: self.view.push,
            };
            if let Some(old) = self
                .view
                .accounts
                .iter_mut()
                .find(|a| a.base == account.base && a.email == account.email)
            {
                *old = account;
            } else {
                self.view.accounts.push(account);
            }
            super::accounts::write(&self.root, &self.view.accounts).await?;
        }
        Ok(())
    }
}
