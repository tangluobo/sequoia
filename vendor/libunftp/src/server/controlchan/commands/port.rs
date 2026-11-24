//! The RFC 959 Data Port (`PORT`) command
//
// The argument is a HOST-PORT specification for the data port
// to be used in data connection.  There are defaults for both
// the user and server data ports, and under normal
// circumstances this command and its reply are not needed.  If
// this command is used, the argument is the concatenation of a
// 32-bit internet host address and a 16-bit TCP port address.
// This address information is broken into 8-bit fields and the
// value of each field is transmitted as a decimal number (in
// character string representation).  The fields are separated
// by commas.  A port command would be:
//
// PORT h1,h2,h3,h4,p1,p2
//
// where h1 is the high order 8 bits of the internet host
// address.

use super::passive_common::setup_inter_loop_comms;
use crate::{
    auth::UserDetail,
    server::{
        controlchan::{
            Reply, ReplyCode,
            error::ControlChanError,
            handler::{CommandContext, CommandHandler},
        },
        datachan,
    },
    storage::{Metadata, StorageBackend},
};
use async_trait::async_trait;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddrV4},
    time::Duration,
};
use tokio::net::TcpStream;

#[derive(Debug)]
pub struct Port {
    addr: String,
}

impl Port {
    pub fn new(addr: String) -> Self {
        Port { addr }
    }
}

#[async_trait]
impl<Storage, User> CommandHandler<Storage, User> for Port
where
    User: UserDetail + 'static,
    Storage: StorageBackend<User> + 'static,
    Storage::Metadata: Metadata,
{
    #[tracing_attributes::instrument]
    async fn handle(&self, args: CommandContext<Storage, User>) -> Result<Reply, ControlChanError> {
        let CommandContext {
            logger,
            tx_control_chan: tx,
            session,
            ..
        } = args;

        let mut fields = self.addr.split(',');
        let mut bytes = [0u8; 6];
        for byte in &mut bytes {
            let Some(value) = fields.next().and_then(|field| field.parse().ok()) else {
                return Ok(Reply::new(ReplyCode::ParameterSyntaxError, "Invalid address format"));
            };
            *byte = value;
        }
        if fields.next().is_some() {
            return Ok(Reply::new(ReplyCode::ParameterSyntaxError, "Invalid address format"));
        }
        let port = u16::from_be_bytes([bytes[4], bytes[5]]);
        let ip = Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);
        let control_peer = session.lock().await.source.ip().to_canonical();
        // Validate before connecting. A post-connect peer check still lets PORT probe other services.
        if port < 1024 || control_peer != IpAddr::V4(ip) {
            return Ok(Reply::new(
                ReplyCode::CommandNotImplementedForParameter,
                "Data endpoint must use the control peer and an unprivileged port",
            ));
        }
        let addr = SocketAddrV4::new(ip, port);
        let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr)).await else {
            return Ok(Reply::new(ReplyCode::CantOpenDataConnection, "Could not establish data connection"));
        };
        let channel = setup_inter_loop_comms(session.clone(), tx).await;
        datachan::spawn_processing(logger, session, stream, Some(channel)).await;

        Ok(Reply::new(ReplyCode::CommandOkay, "PORT command successful"))
    }
}
