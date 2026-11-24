use crate::network::RunningServer;
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use libunftp::{ServerBuilder, options::ActivePassiveMode};
use std::{
    fmt::{self, Debug},
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tokio::io::AsyncRead;
use tomato_core::server::ServerConfig;
use unftp_core::{
    auth::{
        AuthenticationError, Authenticator, Credentials, Principal, UserDetail, UserDetailError,
        UserDetailProvider,
    },
    storage::{Error, ErrorKind, Fileinfo, Metadata, StorageBackend},
};
use unftp_sbe_fs::{Filesystem, Meta};

#[derive(Debug)]
struct Policy {
    config: ServerConfig,
    shares: Vec<(PathBuf, usize)>,
}
impl Policy {
    fn new(config: ServerConfig) -> Self {
        let shares = config
            .shared_directories
            .iter()
            .enumerate()
            .filter_map(|(i, s)| std::fs::canonicalize(&s.path).ok().map(|p| (p, i)))
            .collect();
        Self { config, shares }
    }
}
fn configured_root<'a>(config: &'a ServerConfig, account: Option<&str>) -> Option<&'a str> {
    account
        .and_then(|name| {
            config
                .accounts
                .iter()
                .find(|account| account.enabled && account.username == name)
        })
        .map(|account| account.home_directory.as_str())
        .filter(|path| !path.is_empty())
        .or_else(|| (!config.root_directory.is_empty()).then_some(config.root_directory.as_str()))
        .or_else(|| {
            config
                .shared_directories
                .first()
                .map(|share| share.path.as_str())
        })
}

fn open_root(policy: &Policy, user: &str, configured: Option<&str>) -> Result<PathBuf> {
    let path = configured
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
        })
        .context("未配置根目录")?;
    let root = std::fs::canonicalize(path)?;
    ensure!(root.is_dir(), "共享路径不是目录");
    if let Some((_, index)) = policy.shares.iter().find(|(path, _)| path == &root) {
        let share = &policy.config.shared_directories[*index];
        ensure!(
            share.allowed_users.is_empty() || share.allowed_users.iter().any(|name| name == user),
            "该账号没有共享目录访问权限"
        );
    }
    Ok(root)
}
type SharedPolicy = Arc<std::sync::RwLock<Arc<Policy>>>;
struct Accounts(SharedPolicy);
impl Debug for Accounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TomatoServerAccounts")
    }
}
#[async_trait]
impl Authenticator for Accounts {
    async fn authenticate(
        &self,
        user: &str,
        creds: &Credentials,
    ) -> Result<Principal, AuthenticationError> {
        let policy = self
            .0
            .read()
            .map_err(|_| AuthenticationError::BadPassword)?;
        if policy.config.anonymous_access && user.eq_ignore_ascii_case("anonymous") {
            return Ok(Principal {
                username: "guest".into(),
            });
        }
        if creds
            .password
            .as_deref()
            .is_some_and(|password| policy.config.authenticate(user, password))
        {
            Ok(Principal {
                username: format!("account:{user}"),
            })
        } else {
            Err(AuthenticationError::BadPassword)
        }
    }
}
#[derive(Debug)]
struct User {
    name: String,
    fs: Arc<Filesystem>,
    root: PathBuf,
    configured_root: Option<String>,
    anonymous: bool,
    policy: SharedPolicy,
}
impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}
impl UserDetail for User {}
#[async_trait]
impl UserDetailProvider for Accounts {
    type User = User;
    async fn provide_user_detail(&self, principal: &Principal) -> Result<User, UserDetailError> {
        let policy = self.0.clone();
        let snapshot = policy
            .read()
            .map_err(|_| UserDetailError::new("服务器配置不可用"))?
            .clone();
        let account = match principal.username.as_str() {
            "guest" if snapshot.config.anonymous_access => None,
            identity => Some(
                identity
                    .strip_prefix("account:")
                    .filter(|name| {
                        snapshot
                            .config
                            .accounts
                            .iter()
                            .any(|account| account.enabled && account.username == *name)
                    })
                    .ok_or_else(|| UserDetailError::new("FTP 账号已停用"))?
                    .to_owned(),
            ),
        };
        tokio::task::spawn_blocking(move || {
            let anonymous = account.is_none();
            let configured_root =
                configured_root(&snapshot.config, account.as_deref()).map(str::to_owned);
            let name = account.unwrap_or_else(|| "anonymous".into());
            let root = open_root(&snapshot, &name, configured_root.as_deref())
                .map_err(|_| UserDetailError::new("共享目录不可用"))?;
            let fs =
                Filesystem::new(&root).map_err(|_| UserDetailError::new("无法打开共享目录"))?;
            Ok(User {
                name,
                fs: Arc::new(fs),
                root,
                configured_root,
                anonymous,
                policy,
            })
        })
        .await
        .map_err(|_| UserDetailError::new("读取账号目录失败"))?
    }
}
#[derive(Debug)]
struct Files;
fn accessible(user: &User, write: bool) -> std::result::Result<(), Error> {
    let policy = user
        .policy
        .read()
        .map_err(|_| Error::from(ErrorKind::PermissionDenied))?;
    let allowed = if user.anonymous {
        policy.config.anonymous_access && !write
    } else {
        policy
            .config
            .accounts
            .iter()
            .any(|a| a.enabled && a.username == user.name)
    };
    if !allowed
        || configured_root(
            &policy.config,
            (!user.anonymous).then_some(user.name.as_str()),
        ) != user.configured_root.as_deref()
    {
        return Err(ErrorKind::PermissionDenied.into());
    }
    if let Some((_, index)) = policy.shares.iter().find(|(path, _)| path == &user.root) {
        let share = &policy.config.shared_directories[*index];
        if !share.allowed_users.is_empty() && !share.allowed_users.contains(&user.name)
            || write && share.read_only
        {
            return Err(ErrorKind::PermissionDenied.into());
        }
    }
    Ok(())
}
fn writable(user: &User) -> std::result::Result<(), Error> {
    accessible(user, true)
}

fn normalize(path: &Path) -> std::result::Result<PathBuf, Error> {
    let mut result = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::Normal(name) => result.push(name),
            Component::CurDir | Component::RootDir => {}
            Component::ParentDir if result.pop() => {}
            Component::ParentDir | Component::Prefix(_) => {
                return Err(ErrorKind::PermissionDenied.into());
            }
        }
    }
    Ok(result)
}

async fn make_directories(user: &User, path: &Path) -> std::result::Result<(), Error> {
    if user
        .fs
        .metadata(user, path)
        .await
        .is_ok_and(|meta| meta.is_dir())
    {
        return Ok(());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if component == Component::RootDir {
            continue;
        }
        if let Err(error) = user.fs.mkd(user, &current).await
            && !user
                .fs
                .metadata(user, &current)
                .await
                .is_ok_and(|meta| meta.is_dir())
        {
            return Err(error);
        }
    }
    Ok(())
}
#[async_trait]
impl StorageBackend<User> for Files {
    type Metadata = Meta;
    fn supported_features(&self) -> u32 {
        unftp_core::storage::FEATURE_RESTART | unftp_core::storage::FEATURE_SITEMD5
    }
    async fn metadata<P: AsRef<Path> + Send + Debug>(
        &self,
        u: &User,
        p: P,
    ) -> std::result::Result<Meta, Error> {
        accessible(u, false)?;
        u.fs.metadata(u, p).await
    }
    async fn list<P: AsRef<Path> + Send + Debug>(
        &self,
        u: &User,
        p: P,
    ) -> std::result::Result<Vec<Fileinfo<PathBuf, Meta>>, Error> {
        accessible(u, false)?;
        u.fs.list(u, p).await
    }
    async fn get<P: AsRef<Path> + Send + Debug>(
        &self,
        u: &User,
        p: P,
        offset: u64,
    ) -> std::result::Result<Box<dyn AsyncRead + Send + Sync + Unpin>, Error> {
        accessible(u, false)?;
        u.fs.get(u, p, offset).await
    }
    async fn put<P: AsRef<Path> + Send + Debug, R: AsyncRead + Send + Sync + Unpin + 'static>(
        &self,
        u: &User,
        input: R,
        p: P,
        offset: u64,
    ) -> std::result::Result<u64, Error> {
        writable(u)?;
        let path = normalize(p.as_ref())?;
        let parent = path.parent().ok_or(ErrorKind::PermissionDenied)?;
        make_directories(u, parent).await?;
        u.fs.put(u, input, path, offset).await
    }
    async fn del<P: AsRef<Path> + Send + Debug>(
        &self,
        u: &User,
        p: P,
    ) -> std::result::Result<(), Error> {
        writable(u)?;
        u.fs.del(u, p).await
    }
    async fn mkd<P: AsRef<Path> + Send + Debug>(
        &self,
        u: &User,
        p: P,
    ) -> std::result::Result<(), Error> {
        writable(u)?;
        make_directories(u, &normalize(p.as_ref())?).await
    }
    async fn rename<P: AsRef<Path> + Send + Debug>(
        &self,
        u: &User,
        from: P,
        to: P,
    ) -> std::result::Result<(), Error> {
        writable(u)?;
        u.fs.rename(u, from, to).await
    }
    async fn rmd<P: AsRef<Path> + Send + Debug>(
        &self,
        u: &User,
        p: P,
    ) -> std::result::Result<(), Error> {
        writable(u)?;
        u.fs.rmd(u, p).await
    }
    async fn cwd<P: AsRef<Path> + Send + Debug>(
        &self,
        u: &User,
        p: P,
    ) -> std::result::Result<(), Error> {
        accessible(u, false)?;
        u.fs.cwd(u, p).await
    }
}
pub async fn start(config: ServerConfig) -> Result<RunningServer> {
    config.validate()?;
    let listener =
        tokio::net::TcpListener::bind((config.bind_address.as_str(), config.port)).await?;
    let address = listener.local_addr()?;
    let (send_updates, mut updates) = tokio::sync::watch::channel(config.clone());
    let policy: SharedPolicy = Arc::new(std::sync::RwLock::new(Arc::new(
        tokio::task::spawn_blocking(move || Policy::new(config)).await?,
    )));
    let task = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                socket=listener.accept()=>{
                    let Ok((socket,_))=socket else{break;};
                    if clients.len()>=128 {drop(socket);continue;}
                    let accounts = Arc::new(Accounts(policy.clone()));
                    let server = ServerBuilder::with_user_detail_provider(Box::new(|| Files), accounts.clone())
                        .authenticator(accounts)
                        .greeting("Welcome to Tomato FTP Server")
                        .active_passive_mode(ActivePassiveMode::ActiveAndPassive)
                        .passwordless_anonymous()
                        .passive_ports(49152..=65535)
                        .build();
                    if let Ok(server) = server {
                        clients.spawn(async move { let _ = server.service(socket).await; });
                    }
                }
                _=clients.join_next(),if !clients.is_empty()=>{},
                update=updates.changed()=>{if update.is_err(){break;}let config=updates.borrow_and_update().clone();if let Ok(next)=tokio::task::spawn_blocking(move||Policy::new(config)).await && let Ok(mut current)=policy.write(){*current=Arc::new(next);}}
            }
        }
    });
    Ok(RunningServer::from_task(address, task).with_updates(send_updates))
}
