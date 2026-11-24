//! Contains shared code for the PASV and EPSV commands.

use crate::{
    auth::UserDetail,
    server::{
        ControlChanErrorKind, ControlChanMsg,
        chancomms::{DataChanCmd, PortAllocationError, SwitchboardMessage, SwitchboardSender},
        controlchan::{Reply, ReplyCode, error::ControlChanError, handler::CommandContext},
        datachan,
        session::SharedSession,
    },
    storage::{Metadata, StorageBackend},
};
use async_trait::async_trait;
use std::{fmt::Debug, io, net::SocketAddr, ops::RangeInclusive, time::Duration};
use tokio::net::TcpSocket;
use tokio::sync::{
    mpsc::{Receiver, Sender, channel},
    oneshot,
};
use tokio_util::sync::CancellationToken;

const BIND_RETRIES: u8 = 10;

#[async_trait]
pub(crate) trait LegacyReplyProducer<Storage, User>: Send + Sync + Debug
where
    Storage: StorageBackend<User> + 'static,
    User: UserDetail + 'static,
{
    async fn build_reply(&self, args: &CommandContext<Storage, User>, port: u16) -> Result<Reply, ControlChanError>;
}

#[tracing_attributes::instrument]
pub(crate) fn try_port_range(local_addr: SocketAddr, passive_ports: RangeInclusive<u16>) -> io::Result<TcpSocket> {
    let ip = local_addr.ip();
    let rng_length = passive_ports.end() - passive_ports.start() + 1;

    let mut socket: io::Result<TcpSocket> = Err(io::Error::new(io::ErrorKind::InvalidInput, "Bind retries cannot be 0"));

    for _ in 1..BIND_RETRIES {
        let random_u32 = {
            let mut data = [0; 4];
            getrandom::fill(&mut data).expect("Error generating random port");
            u32::from_ne_bytes(data)
        };

        let port = random_u32 % rng_length as u32 + *passive_ports.start() as u32;
        let s = match ip {
            std::net::IpAddr::V4(_) => TcpSocket::new_v4()?,
            std::net::IpAddr::V6(_) => TcpSocket::new_v6()?,
        };
        // Winsock SO_REUSEADDR can let a second session take the same passive port.
        #[cfg(not(windows))]
        s.set_reuseaddr(true)?;
        if s.bind(std::net::SocketAddr::new(ip, port as u16)).is_ok() {
            socket = Ok(s);
            break;
        }
    }

    socket
}

// modifies the session by adding channels that are used to communicate with the data connection
// processing loop.
#[tracing_attributes::instrument]
pub(crate) async fn setup_inter_loop_comms<S, U>(session: SharedSession<S, U>, control_loop_tx: Sender<ControlChanMsg>) -> CancellationToken
where
    U: UserDetail + 'static,
    S: StorageBackend<U> + 'static,
    S::Metadata: Metadata,
{
    let (cmd_tx, cmd_rx): (Sender<DataChanCmd>, Receiver<DataChanCmd>) = channel(1);

    let mut session = session.lock().await;
    session.cancel_data_channel();
    let channel = session.data_lifetime.child_token();
    session.data_channel = Some(channel.clone());
    session.data_cmd_tx = Some(cmd_tx);
    session.data_cmd_rx = Some(cmd_rx);
    session.control_msg_tx = Some(control_loop_tx);
    channel
}

// For legacy mode we choose a data port here and start listening on it while letting the control
// channel know (via method return) what the address is that the client should connect to.
#[tracing_attributes::instrument]
pub(crate) async fn handle_legacy_mode<S, U, T>(cmd: &T, args: CommandContext<S, U>) -> Result<Reply, ControlChanError>
where
    U: UserDetail + 'static,
    S: StorageBackend<U> + 'static,
    S::Metadata: Metadata,
    T: LegacyReplyProducer<S, U>,
{
    let listener = match args.session.lock().await.binder {
        Some(ref mut binder) => binder.bind(args.local_addr.ip(), args.passive_ports.clone()).await,
        _ => try_port_range(args.local_addr, args.passive_ports.clone()),
    };
    let listener = match listener {
        Err(_) => return Ok(Reply::new(ReplyCode::CantOpenDataConnection, "No data connection established")),
        Ok(l) => l,
    };
    let listener = listener.listen(1024)?;

    let port = listener.local_addr()?.port();

    let reply = cmd.build_reply(&args, port).await?;
    if reply.is_positive() {
        let channel = setup_inter_loop_comms(args.session.clone(), args.tx_control_chan.clone()).await;
        let peer = args.session.lock().await.source.ip().to_canonical();
        // A different peer must not be able to consume this listener or its command channel.
        let accept_peer = async move {
            loop {
                let (socket, address) = listener.accept().await?;
                if address.ip().to_canonical() == peer {
                    break Ok::<_, io::Error>(socket);
                }
            }
        };
        tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = channel.cancelled() => return,
                result = tokio::time::timeout(Duration::from_secs(15), accept_peer) => result,
            };
            match result {
                Ok(Ok(socket)) => datachan::spawn_processing(args.logger, args.session, socket, Some(channel)).await,
                Ok(Err(e)) => slog::error!(args.logger, "Error waiting for data connection: {}", e),
                Err(_) => slog::warn!(args.logger, "Client did not connect to data port in time"),
            }
        });
    }

    Ok(reply)
}

// For delegated mode, we prepare the session and let the listener loop know (via channel) that it
// should choose a data port and check for connections on it.
#[tracing_attributes::instrument]
pub(crate) async fn handle_delegated_mode<S, U>(args: CommandContext<S, U>, tx: SwitchboardSender<S, U>) -> Result<Reply, ControlChanError>
where
    U: UserDetail + 'static,
    S: StorageBackend<U> + 'static,
    S::Metadata: Metadata,
{
    setup_inter_loop_comms(args.session.clone(), args.tx_control_chan).await;

    let (oneshot_tx, oneshot_rx) = oneshot::channel::<Result<Reply, PortAllocationError>>();

    tx.send(SwitchboardMessage::AssignDataPortCommand(args.session.clone(), oneshot_tx))
        .await
        .map_err(|_| ControlChanErrorKind::InternalServerError)?;

    match oneshot_rx.await {
        Ok(Ok(reply)) => Ok(reply),
        Ok(Err(_)) => Ok(Reply::new(ReplyCode::CantOpenDataConnection, "Could not allocate passive port")),
        Err(_) => Ok(Reply::new(ReplyCode::CantOpenDataConnection, "Internal error: Channel closed")),
    }
}
