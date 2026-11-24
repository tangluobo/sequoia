use anyhow::{Context, Result, ensure};
use std::{net::SocketAddr, path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream},
    net::{TcpListener, TcpStream},
};
use tomato_core::server::{ServerAccount, ServerConfig, ServerType};

struct Client(BufStream<TcpStream>);

impl Client {
    async fn login(address: SocketAddr) -> Result<Self> {
        let mut client = Self(BufStream::new(TcpStream::connect(address).await?));
        client.reply(220).await?;
        client.command("USER fixture", 331).await?;
        client.command("PASS fixture-only", 230).await?;
        client.command("TYPE I", 200).await?;
        Ok(client)
    }

    async fn reply(&mut self, code: u16) -> Result<String> {
        let mut line = String::new();
        self.0.read_line(&mut line).await?;
        ensure!(
            line.starts_with(&format!("{code} ")),
            "expected FTP reply {code}, got {line:?}"
        );
        Ok(line)
    }

    async fn command(&mut self, command: &str, code: u16) -> Result<String> {
        self.0.write_all(command.as_bytes()).await?;
        self.0.write_all(b"\r\n").await?;
        self.0.flush().await?;
        self.reply(code).await
    }

    async fn active(&mut self) -> Result<TcpStream> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let [hi, lo] = listener.local_addr()?.port().to_be_bytes();
        self.command(&format!("PORT 127,0,0,1,{hi},{lo}"), 200)
            .await?;
        Ok(listener.accept().await?.0)
    }

    async fn passive_address(&mut self, ip: std::net::IpAddr) -> Result<SocketAddr> {
        let reply = self.command("EPSV", 229).await?;
        let port = reply
            .split_once("|||")
            .and_then(|(_, tail)| tail.split_once('|'))
            .context("malformed EPSV reply")?
            .0
            .parse()?;
        Ok(SocketAddr::new(ip, port))
    }
}

fn config(root: &Path) -> ServerConfig {
    ServerConfig {
        kind: ServerType::Ftp,
        port: 0,
        bind_address: "127.0.0.1".into(),
        root_directory: root.to_string_lossy().into_owned(),
        accounts: vec![ServerAccount {
            username: "fixture".into(),
            password: "fixture-only".into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[tokio::test]
async fn active_ftp_and_legacy_directory_commands_transfer_through_the_server_dispatcher()
-> Result<()> {
    tokio::time::timeout(Duration::from_secs(25), async {
        let root = tempfile::tempdir()?;
        let server = tomato_services::servers::start(config(root.path())).await?;
        let mut ftp = Client::login(server.address).await?;
        // Reject an invalid target before any data socket can be dialed.
        ftp.command("PORT 127,0,0,1,0,21", 504).await?;
        ftp.command("PORT 127,0,0,2,195,80", 504).await?;
        ftp.command("PORT 127,0,bad,0,1,195,80", 501).await?;
        ftp.command("XMKD /目录/下一级", 257).await?;
        ftp.command("XMKD /目录/下一级", 257).await?;
        let mut data = ftp.active().await?;
        ftp.command("XCWD /目录/下一级", 250).await?;
        assert!(ftp.command("XPWD", 257).await?.contains("/目录/下一级"));
        ftp.command("STOR 新建/更多/文件.bin", 150).await?;
        let bytes = "主动模式传输🍅\0\n".repeat(16_000).into_bytes();
        data.write_all(&bytes).await?;
        data.shutdown().await?;
        ftp.reply(226).await?;
        let file = root.path().join("目录/下一级/新建/更多/文件.bin");
        assert_eq!(tokio::fs::read(&file).await?, bytes);
        assert!(
            ftp.command("SIZE 新建/更多/文件.bin", 213)
                .await?
                .contains(&bytes.len().to_string())
        );
        ftp.command("MDTM 新建/更多/文件.bin", 213).await?;

        let mut data = ftp.active().await?;
        ftp.command("REST 128", 350).await?;
        ftp.command("RETR 新建/更多/文件.bin", 150).await?;
        let mut received = Vec::new();
        data.read_to_end(&mut received).await?;
        ftp.reply(226).await?;
        assert_eq!(received, bytes[128..]);

        let mut data = ftp.active().await?;
        ftp.command("NLST", 150).await?;
        let mut listing = String::new();
        data.read_to_string(&mut listing).await?;
        ftp.reply(226).await?;
        assert!(listing.contains("新建"), "{listing:?}");
        ftp.command("RNFR 新建/更多/文件.bin", 350).await?;
        ftp.command("RNTO 新建/更多/改名.bin", 250).await?;
        ftp.command("DELE 新建/更多/改名.bin", 250).await?;
        ftp.command("XRMD 新建/更多", 250).await?;
        ftp.command("RMD 新建", 250).await?;
        assert!(!root.path().join("目录/下一级/新建").exists());
        ftp.command("XMKD /../逃逸", 550).await?;
        ftp.command("QUIT", 221).await?;
        Ok(())
    })
    .await?
}

#[tokio::test]
async fn ftp_abort_and_stop_close_open_data_sockets_and_invalidate_old_roots() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(25), async {
        let root = tempfile::tempdir()?;
        let changed = tempfile::tempdir()?;
        tokio::fs::write(root.path().join("old.txt"), b"old").await?;
        tokio::fs::write(changed.path().join("new.txt"), b"new").await?;
        let server = tomato_services::servers::start(config(root.path())).await?;
        let address = server.address;
        let mut ftp = Client::login(address).await?;
        let mut data = ftp.active().await?;
        ftp.command("STOR interrupted.bin", 150).await?;
        data.write_all(b"before abort").await?;
        // No data EOF from the client. ABOR has to interrupt the transfer itself.
        ftp.command("ABOR", 226).await?;
        let mut byte = [0; 1];
        assert!(matches!(data.read(&mut byte).await, Ok(0) | Err(_)));
        ftp.command("NOOP", 200).await?;
        ftp.command("SIZE /old.txt", 213).await?;

        server.update_config(config(changed.path()))?;
        let mut next = loop {
            let mut next = Client::login(address).await?;
            next.0.write_all(b"SIZE /new.txt\r\n").await?;
            next.0.flush().await?;
            let mut line = String::new();
            next.0.read_line(&mut line).await?;
            if line.starts_with("213 ") {
                break next;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        ftp.command("SIZE /old.txt", 550).await?;
        let passive = next.passive_address(address.ip()).await?;
        let mut stalled = Client::login(address).await?;
        let mut active = stalled.active().await?;
        stalled.command("STOR /abandoned.bin", 150).await?;
        active.write_all(b"waiting for stop").await?;
        drop(server);

        assert!(matches!(active.read(&mut byte).await, Ok(0) | Err(_)));
        assert!(matches!(next.0.read(&mut byte).await, Ok(0) | Err(_)));
        assert!(matches!(stalled.0.read(&mut byte).await, Ok(0) | Err(_)));
        // Poll via bind rather than connect, which would satisfy a lingering PASV accept.
        for endpoint in [address, passive] {
            tokio::time::timeout(Duration::from_secs(3), async {
                while TcpListener::bind(endpoint).await.is_err() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .context("FTP listener survived shutdown")?;
        }
        Ok(())
    })
    .await?
}

#[tokio::test]
async fn anonymous_ftp_logs_in_on_user_is_read_only_and_cannot_inherit_a_named_home() -> Result<()>
{
    tokio::time::timeout(Duration::from_secs(20), async {
        let root = tempfile::tempdir()?;
        let home = tempfile::tempdir()?;
        tokio::fs::write(root.path().join("public.txt"), b"public").await?;
        tokio::fs::write(home.path().join("private.txt"), b"private").await?;
        let mut config = config(root.path());
        config.anonymous_access = true;
        config.accounts.push(ServerAccount {
            username: "anonymous".into(),
            password: "named-account-fixture".into(),
            home_directory: home.path().to_string_lossy().into_owned(),
            ..Default::default()
        });
        let server = tomato_services::servers::start(config.clone()).await?;
        let address = server.address;
        let mut guest = Client(BufStream::new(TcpStream::connect(address).await?));
        guest.reply(220).await?;
        guest.command("USER AnOnYmOuS", 230).await?;
        guest.command("SIZE /private.txt", 550).await?;
        let endpoint = guest.passive_address(address.ip()).await?;
        let mut data = TcpStream::connect(endpoint).await?;
        guest.command("RETR /public.txt", 150).await?;
        let mut bytes = Vec::new();
        data.read_to_end(&mut bytes).await?;
        guest.reply(226).await?;
        assert_eq!(bytes, b"public");
        guest.command("MKD /denied", 550).await?;
        guest.command("DELE /public.txt", 550).await?;

        config.anonymous_access = false;
        server.update_config(config)?;
        let mut member = loop {
            let mut client = Client(BufStream::new(TcpStream::connect(address).await?));
            client.reply(220).await?;
            client.0.write_all(b"USER anonymous\r\n").await?;
            client.0.flush().await?;
            let mut line = String::new();
            client.0.read_line(&mut line).await?;
            if line.starts_with("331 ") {
                break client;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        };
        member.command("PASS named-account-fixture", 230).await?;
        member.command("SIZE /private.txt", 213).await?;
        guest.command("SIZE /public.txt", 550).await?;
        assert_eq!(
            tokio::fs::read(root.path().join("public.txt")).await?,
            b"public"
        );
        Ok(())
    })
    .await?
}
