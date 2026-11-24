use super::{Action, Snapshot, transport};
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{mpsc, oneshot},
    task::JoinSet,
    time::{interval, timeout},
};
use tomato_core::input_share::{Input, MAX_NODES, Node, Routed, validate};
use tomato_native_input::{Capture, Update};
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Wire {
    Focus(String),
    Clear,
    Yield,
    Hello(Node),
    Layout(Vec<Node>),
    Order(Vec<String>),
    Input(Routed),
    Ping,
}
enum Event {
    Native(Routed),
    Connected(String, Node, mpsc::Sender<Wire>),
    Message(String, String, Wire),
    Gone(String, String),
    Action(Action, oneshot::Sender<Result<()>>),
}
pub(super) struct Session {
    state: Arc<Mutex<Snapshot>>,
    tx: mpsc::Sender<Event>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Session {
    pub async fn start(
        port: u16,
        join: Option<(String, String)>,
        avoid: Option<String>,
    ) -> Result<Self> {
        let (width, height) = tomato_native_input::screen();
        let local = Node {
            id: uuid::Uuid::new_v4().to_string(),
            name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Tomato".into()),
            width,
            height,
        };
        validate(std::slice::from_ref(&local))?;
        let hosting = join.is_none();
        let code = join
            .as_ref()
            .map(|(_, code)| code.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let key = transport::secret(&code)?;
        let (listener, discovery) = if hosting {
            ensure!(port != 0, "请输入有效端口");
            let listener = TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).await?;
            let discovery = UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, super::PORT)).await?;
            (Some(listener), Some(discovery))
        } else {
            (None, None)
        };
        let initial = if let Some((address, _)) = join {
            let stream = timeout(Duration::from_secs(3), TcpStream::connect(address)).await??;
            let connection = authenticate(stream, key, local.clone()).await?;
            ensure!(
                avoid.as_deref() != Some(connection.3.id.as_str()),
                "不能加入本机正在运行的服务，请选择其他主机"
            );
            Some(connection)
        } else {
            None
        };
        let state = Arc::new(Mutex::new(Snapshot {
            local_id: local.id.clone(),
            active: local.id.clone(),
            running: true,
            hosting,
            port,
            code: if hosting { code } else { String::new() },
            nodes: vec![local.clone()],
            ..Default::default()
        }));
        let (tx, rx) = mpsc::channel(512);
        let events = tx.clone();
        let capture = Box::new(tomato_native_input::start(
            local.id.clone(),
            Box::new(move |event| events.try_send(Event::Native(event)).is_ok()),
        )?);
        capture.send(Update::Layout(vec![local.clone()]));
        let actor = Actor {
            local,
            state: state.clone(),
            rx,
            tx: tx.clone(),
            capture,
            peers: HashMap::new(),
            tasks: JoinSet::new(),
            hosting,
            key,
            owner: None,
        };
        let task = tokio::spawn(actor.run(listener, discovery, initial));
        Ok(Self { state, tx, task })
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
    pub async fn action(&self, action: Action) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Event::Action(action, tx))
            .await
            .map_err(|_| anyhow!("共享服务已退出，请停止后重新启动"))?;
        timeout(Duration::from_secs(2), rx).await??
    }
}
type Connection = (TcpStream, transport::Cipher, transport::Cipher, Node);
async fn authenticate(mut stream: TcpStream, key: [u8; 16], local: Node) -> Result<Connection> {
    stream.set_nodelay(true)?;
    let (mut send, mut recv) = transport::handshake(&mut stream, &key).await?;
    transport::write(&mut stream, &mut send, &Wire::Hello(local)).await?;
    let Wire::Hello(node) = timeout(
        Duration::from_secs(3),
        transport::read(&mut stream, &mut recv),
    )
    .await??
    else {
        anyhow::bail!("协议版本不兼容");
    };
    validate(std::slice::from_ref(&node))?;
    Ok((stream, send, recv, node))
}
async fn connection(connection: Connection, tx: mpsc::Sender<Event>) {
    let (stream, mut send, mut recv, node) = connection;
    let id = node.id.clone();
    let token = uuid::Uuid::new_v4().to_string();
    let (mut reader, mut writer) = stream.into_split();
    let (out, mut queue) = mpsc::channel(256);
    if tx
        .send(Event::Connected(token.clone(), node, out))
        .await
        .is_err()
    {
        return;
    }
    let mut readers = JoinSet::new();
    let events = tx.clone();
    let source = id.clone();
    let reader_token = token.clone();
    readers.spawn(async move {
        loop {
            let wire = timeout(
                Duration::from_secs(3),
                transport::read(&mut reader, &mut recv),
            )
            .await??;
            events
                .send(Event::Message(reader_token.clone(), source.clone(), wire))
                .await
                .map_err(|_| anyhow!("共享服务已停止"))?;
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });
    let mut heartbeat = interval(Duration::from_millis(500));
    loop {
        let value = tokio::select! {
            _ = readers.join_next() => break,
            value = queue.recv() => if let Some(value) = value { value } else { break; },
            _ = heartbeat.tick() => Wire::Ping,
        };
        if transport::write(&mut writer, &mut send, &value)
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = tx.send(Event::Gone(token, id)).await;
}
trait InputAdapter: Send {
    fn send(&self, update: Update);
    fn failed(&self) -> bool;
}
impl InputAdapter for Capture {
    fn send(&self, update: Update) {
        Capture::send(self, update);
    }
    fn failed(&self) -> bool {
        Capture::failed(self)
    }
}
struct Peer {
    token: String,
    sender: mpsc::Sender<Wire>,
}
struct Actor {
    owner: Option<String>,
    local: Node,
    state: Arc<Mutex<Snapshot>>,
    rx: mpsc::Receiver<Event>,
    tx: mpsc::Sender<Event>,
    capture: Box<dyn InputAdapter>,
    peers: HashMap<String, Peer>,
    tasks: JoinSet<()>,
    hosting: bool,
    key: [u8; 16],
}
impl Actor {
    fn upstream(&mut self, wire: Wire) {
        let failed = self
            .peers
            .iter()
            .next()
            .and_then(|(id, peer)| peer.sender.try_send(wire).err().map(|_| id.clone()));
        if let Some(id) = failed {
            self.evict(&id);
        }
    }
    fn evict(&mut self, id: &str) {
        if self.peers.remove(id).is_none() {
            return;
        }
        self.capture.send(Update::Reset);
        if self.hosting {
            let mut nodes = self.nodes();
            nodes.retain(|n| n.id != id);
            self.layout(nodes);
        } else {
            self.state.lock().unwrap_or_else(|e| e.into_inner()).running = false;
        }
    }
    fn clear(&mut self) {
        self.capture.send(Update::Inject(Input::Release));
        self.broadcast(Wire::Clear);
    }
    fn relinquish(&mut self, requester: &str) {
        self.clear();
        if let Some(previous) = self.owner.take()
            && previous != requester
        {
            if previous == self.local.id {
                self.capture.send(Update::Return);
            } else if let Some(peer) = self.peers.get(&previous) {
                let _ = peer.sender.try_send(Wire::Yield);
            }
        }
    }
    fn input_from(&mut self, source: &str, input: Routed) {
        if let Input::Display { width, height } = input.input {
            let mut nodes = self.nodes();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == source) {
                node.width = width;
                node.height = height;
            }
            if validate(&nodes).is_ok() {
                if source == self.local.id {
                    self.local.width = width;
                    self.local.height = height;
                }
                self.layout(nodes);
            }
            return;
        }
        if input.target.is_empty() && matches!(input.input, Input::Release) {
            self.relinquish(source);
            return;
        }
        if !self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .nodes
            .iter()
            .any(|n| n.id == input.target)
        {
            return;
        }
        // Only an explicit physical-control claim can take ownership. Late motion/key
        // records from the previous source must not steal control back after yielding.
        if !matches!(input.input, Input::Claim) && self.owner.as_deref() != Some(source) {
            return;
        }
        if matches!(input.input, Input::Claim) && self.owner.as_deref() != Some(source) {
            self.clear();
            if let Some(previous) = self.owner.take() {
                if previous == self.local.id {
                    self.capture.send(Update::Return);
                } else if let Some(peer) = self.peers.get(&previous) {
                    let _ = peer.sender.try_send(Wire::Yield);
                }
            }
            self.owner = Some(source.into());
        }
        let changed = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.active != input.target {
                state.active = input.target.clone();
                true
            } else {
                false
            }
        };
        if changed {
            self.broadcast(Wire::Focus(input.target.clone()));
        }
        if !matches!(input.input, Input::Claim) {
            self.route(input);
        }
    }
    fn nodes(&self) -> Vec<Node> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .nodes
            .clone()
    }
    fn layout(&mut self, mut nodes: Vec<Node>) {
        self.clear();
        if self.hosting {
            nodes.retain(|n| n.id == self.local.id || self.peers.contains_key(&n.id));
        }
        self.owner = None;
        self.capture.send(Update::Layout(nodes.clone()));
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.nodes = nodes.clone();
            state.active.clear();
        }
        if self.hosting {
            self.broadcast(Wire::Layout(nodes));
        }
    }
    fn broadcast(&mut self, wire: Wire) {
        let failed: Vec<_> = self
            .peers
            .iter()
            .filter_map(|(id, peer)| peer.sender.try_send(wire.clone()).err().map(|_| id.clone()))
            .collect();
        for id in failed {
            self.evict(&id);
        }
    }
    fn route(&mut self, routed: Routed) {
        let destination = if self.hosting {
            routed.target.clone()
        } else {
            self.peers.keys().next().cloned().unwrap_or_default()
        };
        if routed.target == self.local.id {
            self.capture.send(Update::Inject(routed.input));
        } else if self.hosting {
            if let Some(peer) = self.peers.get(&routed.target)
                && peer.sender.try_send(Wire::Input(routed)).is_err()
            {
                self.evict(&destination);
            }
        } else if let Some(peer) = self.peers.values().next()
            && peer.sender.try_send(Wire::Input(routed)).is_err()
        {
            self.evict(&destination);
        }
    }
    fn order(&mut self, ids: Vec<String>) -> Result<()> {
        let current = self.nodes();
        ensure!(ids.len() == current.len(), "节点列表已变化，请重试");
        let nodes: Vec<_> = ids
            .iter()
            .filter_map(|id| current.iter().find(|n| &n.id == id).cloned())
            .collect();
        validate(&nodes)?;
        ensure!(nodes.len() == current.len(), "节点列表无效");
        if self.hosting {
            self.layout(nodes);
        } else {
            self.peers
                .values()
                .next()
                .ok_or_else(|| anyhow!("服务已断开"))?
                .sender
                .try_send(Wire::Order(ids))?;
        }
        Ok(())
    }
    async fn run(
        mut self,
        listener: Option<TcpListener>,
        discovery: Option<UdpSocket>,
        initial: Option<Connection>,
    ) {
        if let Some(connection) = initial {
            self.tasks
                .spawn(connection_task(connection, self.tx.clone()));
        }
        let permits = Arc::new(tokio::sync::Semaphore::new(MAX_NODES));
        let mut tick = interval(Duration::from_millis(500));
        let mut buffer = [0; 128];
        loop {
            tokio::select! {
                accepted = async { if let Some(l)=&listener { l.accept().await } else { std::future::pending().await } } => {
                    if let Ok((stream,_)) = accepted && let Ok(permit) = permits.clone().try_acquire_owned() {
                        let key=self.key; let local=self.local.clone(); let tx=self.tx.clone();
                        self.tasks.spawn(async move {
                            let _permit=permit;
                            if let Ok(Ok(c))=timeout(Duration::from_secs(6),authenticate(stream,key,local)).await { connection(c,tx).await; }
                        });
                    }
                }
                packet = async { if let Some(s)=&discovery { s.recv_from(&mut buffer).await } else { std::future::pending().await } } => {
                    if let Ok((n,addr))=packet && &buffer[..n] == transport::DISCOVER {
                        let mut reply=transport::DISCOVER.to_vec();
                        reply.extend_from_slice(&self.state.lock().unwrap_or_else(|e|e.into_inner()).port.to_be_bytes());
                        if let Some(s)=&discovery { let _=s.send_to(&reply,addr).await; }
                    }
                }
                Some(event)=self.rx.recv() => match event {
                    Event::Native(input) => {
                        if let Input::Display { width,height }=input.input { self.local.width=width; self.local.height=height; }
                        if self.hosting { let id=self.local.id.clone(); self.input_from(&id,input); } else { self.upstream(Wire::Input(input)); }
                    }
                    Event::Connected(token,node,tx) => {
                        let mut nodes=self.nodes();
                        if self.peers.contains_key(&node.id) || node.id == self.local.id || nodes.len() >= MAX_NODES { continue; }
                        self.peers.insert(node.id.clone(),Peer { token,sender:tx });
                        if self.hosting { nodes.push(node); self.layout(nodes); }
                    }
                    Event::Gone(token,id) => {
                        if !self.peers.get(&id).is_some_and(|p|p.token == token) { continue; }
                        self.peers.remove(&id);
                        self.capture.send(Update::Reset);
                        if !self.hosting { break; }
                        let mut nodes=self.nodes(); nodes.retain(|n|n.id != id); self.layout(nodes);
                        self.clear(); self.owner=None;
                    }
                    Event::Message(token,id,wire) => {
                        if !self.peers.get(&id).is_some_and(|p|p.token == token) { continue; }
                        match wire {
                            Wire::Layout(nodes) if !self.hosting && validate(&nodes).is_ok() && nodes.iter().any(|n|n == &self.local) => self.layout(nodes),
                            Wire::Order(ids) if self.hosting => { let _=self.order(ids); }
                            Wire::Input(routed) => {
                                if self.hosting { self.input_from(&id,routed); }
                                else if routed.target == self.local.id { self.capture.send(Update::Inject(routed.input)); }
                            }
                            Wire::Clear if !self.hosting => self.capture.send(Update::Inject(Input::Release)),
                            Wire::Yield if !self.hosting => self.capture.send(Update::Return),
                            Wire::Focus(id) if !self.hosting && self.nodes().iter().any(|n|n.id==id) => self.state.lock().unwrap_or_else(|e|e.into_inner()).active=id,
                            Wire::Ping => {},
                            _ => {},
                        }
                    }
                    Event::Action(action,reply) => {
                        let result=match action {
                            Action::Order { ids } => self.order(ids),
                            Action::Release => { self.capture.send(Update::Pause); if self.hosting { let id=self.local.id.clone(); self.relinquish(&id); } else { self.route(Routed { target:String::new(),input:Input::Release }); } Ok(()) },
                            Action::Resume => { self.capture.send(Update::Resume); self.state.lock().unwrap_or_else(|e|e.into_inner()).error.clear(); Ok(()) },
                            _ => Ok(()),
                        };
                        let _=reply.send(result);
                    }
                },
                _ = tick.tick() => {
                    if !self.state.lock().unwrap_or_else(|e|e.into_inner()).running { break; }
                    self.capture.send(Update::Healthy);
                    if self.capture.failed() {
                        self.capture.send(Update::Reset);
                        self.state.lock().unwrap_or_else(|e|e.into_inner()).error="系统输入不可用或队列拥塞，共享已停止，请重新启动或加入".into();
                        break;
                    }
                }
                _ = self.tasks.join_next(), if !self.tasks.is_empty() => {},
            }
        }
        self.capture.send(Update::Reset);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.running = false;
        state.nodes = vec![self.local.clone()];
        if state.error.is_empty() {
            state.error = "共享连接已断开；已返回本机".into();
        }
    }
}
async fn connection_task(c: Connection, tx: mpsc::Sender<Event>) {
    connection(c, tx).await;
}
