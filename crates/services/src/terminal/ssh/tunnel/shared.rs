//! One authenticated SSH transport, shared only by its scoped node forwards.
use super::*;
#[derive(Clone)]
pub(crate) struct Forwarder(Arc<connect::Connected>);
impl Forwarder {
    pub(crate) async fn open(jump: &Connection, known_hosts: PathBuf) -> Result<Self> {
        ensure!(
            matches!(jump.kind(), "SSH" | "SFTP"),
            "跳板连接必须为 SSH/SFTP 类型"
        );
        let (answers, mut input) = mpsc::channel(1);
        drop(answers);
        let events = Events {
            state: Arc::new(Mutex::new(Status::Connecting)),
            notifier: Arc::new(Mutex::new(None)),
        };
        let connected = connect::authenticated(jump, known_hosts, &mut input, &events)
            .await
            .context("SSH 跳板认证失败；需要交互认证时请先在 SSH 连接中完成认证并保存所需凭据")?;
        Ok(Self(Arc::new(connected)))
    }
    pub(crate) fn is_active(&self) -> bool {
        !self.0.session.is_closed() && self.0._parents.iter().all(|p| !p.is_closed())
    }
    pub(crate) async fn forward(&self, target: &Connection) -> Result<Tunnel> {
        let host = target.text("host").to_owned();
        let port = u16::try_from(target.number("port"))?;
        ensure!(!host.is_empty() && port > 0, "隧道目标地址无效");
        ensure!(self.is_active(), "SSH 跳板连接已关闭");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let connection = self.clone();
        let task = tokio::spawn(async move {
            let mut jobs = tokio::task::JoinSet::new();
            let mut health = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = health.tick() => { if !connection.is_active() {break;} },
                    result = listener.accept(), if jobs.len()<16 => {
                        let Ok((mut local,_))=result else {break;};let _=local.set_nodelay(true);
                        let connection=connection.clone();let host=host.clone();
                        jobs.spawn(async move {
                            let channel=tokio::time::timeout(Duration::from_secs(15),connection.0.session.channel_open_direct_tcpip(&host,u32::from(port),"127.0.0.1",0)).await;
                            if let Ok(Ok(channel))=channel {let mut remote=channel.into_stream();let _=tokio::io::copy_bidirectional(&mut local,&mut remote).await;}
                        });
                    },
                    _=jobs.join_next(),if !jobs.is_empty()=>{},
                }
            }
        });
        Ok(Tunnel {
            address,
            task,
            _connection: self.clone(),
        })
    }
}
