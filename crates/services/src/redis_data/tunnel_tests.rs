use super::*;
use crate::ssh_fixture as ssh_jump;
use anyhow::Context as _;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};

#[tokio::test]
async fn redis_tls_over_ssh_preserves_sni_auth_database_and_certificate_validation() -> Result<()> {
    crate::init_crypto();
    for (trusted, matching) in [(true, true), (true, false), (false, true)] {
        let jump = ssh_jump::Jump::start().await?;
        let host = "redis.internal.test";
        let cert_name = if matching {
            host
        } else {
            "wrong.internal.test"
        };
        let certificate = rcgen::generate_simple_self_signed(vec![cert_name.into()])?;
        let cert = certificate.cert.der().clone();
        let key =
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der());
        let server_tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert.clone()], key.into())?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_tls));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let commands = Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
        let captured = commands.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let Ok(socket) = acceptor.accept(socket).await else {
                return Ok::<_, anyhow::Error>(());
            };
            assert_eq!(
                socket.get_ref().1.server_name(),
                Some("redis.internal.test")
            );
            let mut socket = BufReader::new(socket);
            loop {
                let mut line = String::new();
                match socket.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(error) => return Err(error.into()),
                }
                let count: usize = line
                    .trim()
                    .strip_prefix('*')
                    .context("expected RESP array")?
                    .parse()?;
                let mut args = Vec::new();
                for _ in 0..count {
                    line.clear();
                    socket.read_line(&mut line).await?;
                    let size: usize = line
                        .trim()
                        .strip_prefix('$')
                        .context("expected RESP bulk string")?
                        .parse()?;
                    let mut bytes = vec![0; size + 2];
                    socket.read_exact(&mut bytes).await?;
                    args.push(String::from_utf8(bytes[..size].to_vec())?);
                }
                let ping = args.first().is_some_and(|s| s.eq_ignore_ascii_case("PING"));
                captured.lock().unwrap().push(args);
                socket
                    .get_mut()
                    .write_all(if ping { b"+PONG\r\n" } else { b"+OK\r\n" })
                    .await?;
                socket.get_mut().flush().await?;
            }
            Ok(())
        });
        let mut connection = Connection::new("REDIS");
        connection.set("host", host);
        connection.set("port", address.port());
        connection.set("redisTls", true);
        connection.set("redisDatabase", 3);
        connection.set("username", "fixture");
        connection.set("password", "fixture-only");
        let before = serde_json::to_value(&connection)?;
        let tunnel = crate::terminal::ssh::tunnel::Tunnel::open_via(
            &connection,
            &jump.connection,
            jump.root.path().join("known_hosts"),
        )
        .await?;
        let tls = if trusted {
            let mut roots = rustls::RootCertStore::empty();
            roots.add(cert)?;
            Some(
                rustls::ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        } else {
            None
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            routed_session_at(&connection, tunnel.address, tls),
        )
        .await?;
        if trusted && matching {
            let mut session = result?;
            let response: String = query(&mut session, &mut redis::cmd("PING")).await?;
            assert_eq!(response, "PONG");
            assert!(
                commands
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|cmd| cmd == &["AUTH", "fixture", "fixture-only"])
            );
            assert!(
                commands
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|cmd| cmd == &["SELECT", "3"])
            );
            drop(session);
        } else {
            assert!(result.is_err(), "invalid certificate was accepted");
        }
        assert_eq!(serde_json::to_value(&connection)?, before);
        assert_eq!(
            jump.targets.lock().unwrap().as_slice(),
            &[(host.into(), u32::from(address.port()))]
        );
        tokio::time::timeout(std::time::Duration::from_secs(3), server).await???;
        drop(tunnel);
    }
    Ok(())
}
