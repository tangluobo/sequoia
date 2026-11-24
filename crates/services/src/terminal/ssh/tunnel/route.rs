//! A scoped TCP route. Destination identity is kept by the calling protocol.
use super::*;

#[derive(Clone)]
pub struct Route {
    target: Connection,
    jump: Option<(Connection, PathBuf)>,
    active: Arc<tokio::sync::Mutex<Option<Tunnel>>>,
}
impl Route {
    pub fn new(target: &Connection) -> Self {
        Self {
            target: target.clone(),
            jump: None,
            active: Default::default(),
        }
    }
    pub fn via(target: &Connection, jump: &Connection, known_hosts: PathBuf) -> Self {
        Self {
            jump: Some((jump.clone(), known_hosts)),
            ..Self::new(target)
        }
    }
    pub async fn connect(&self, host: &str, port: u16) -> Result<tokio::net::TcpStream> {
        let mut target = self.target.clone();
        target.set("host", host);
        target.set("port", port);
        let tunnel = if let Some((jump, known)) = &self.jump {
            Tunnel::open_via(&target, jump, known.clone()).await?
        } else {
            Tunnel::open(&target).await?
        };
        let socket = tokio::net::TcpStream::connect(tunnel.address).await?;
        socket.set_nodelay(true)?;
        *self.active.lock().await = Some(tunnel);
        Ok(socket)
    }
}
