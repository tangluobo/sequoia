//! Independent RESP fault peer: verifies per-key errors, continuation, and no replay.
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tomato_core::Connection;
use tomato_services::redis_data::{self, Action, Output};

#[tokio::test]
async fn batch_delete_reports_partial_failures_and_preserves_completed_work_on_disconnect()
-> Result<()> {
    for disconnect in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let mut stream = BufReader::new(socket);
            let mut deleted = Vec::new();
            loop {
                let mut header = String::new();
                if stream.read_line(&mut header).await? == 0 {
                    break;
                }
                let count: usize = header.trim().strip_prefix('*').unwrap().parse()?;
                let mut args = Vec::new();
                for _ in 0..count {
                    let mut length = String::new();
                    stream.read_line(&mut length).await?;
                    let bytes: usize = length.trim().strip_prefix('$').unwrap().parse()?;
                    let mut value = vec![0; bytes + 2];
                    stream.read_exact(&mut value).await?;
                    value.truncate(bytes);
                    args.push(String::from_utf8(value)?);
                }
                if args[0].eq_ignore_ascii_case("DEL") {
                    assert_eq!(
                        args.len(),
                        2,
                        "Java deletes each selected key independently"
                    );
                    deleted.push(args[1].clone());
                    if args[1] == "denied" {
                        if disconnect {
                            break;
                        }
                        let reply = format!(
                            "-NOPERM fixture denies this key {}\r\n",
                            "x".repeat(2 * 1024 * 1024)
                        );
                        stream.get_mut().write_all(reply.as_bytes()).await?;
                    } else {
                        stream.get_mut().write_all(b":1\r\n").await?;
                    }
                } else {
                    assert_eq!(args[0].to_uppercase(), "CLIENT");
                    stream.get_mut().write_all(b"+OK\r\n").await?;
                }
            }
            Ok::<_, anyhow::Error>(deleted)
        });
        let mut connection = Connection::new("REDIS");
        connection.set("host", "127.0.0.1");
        connection.set("port", port);
        let output = redis_data::execute(
            &connection,
            Action::Delete {
                keys: vec!["first".into(), "denied".into(), "last".into()],
            },
        )
        .await?;
        let Output::DeleteReport {
            succeeded,
            failed,
            failures,
        } = output
        else {
            panic!("Partial work must not be lost in a generic error")
        };
        if disconnect {
            assert_eq!((succeeded, failed), (1, 2));
            assert_eq!(failures[1].0, "last");
            assert!(failures[1].1.contains("未发送"));
            assert_eq!(peer.await??, ["first", "denied"]);
        } else {
            assert_eq!((succeeded, failed), (2, 1));
            assert_eq!(failures[0].0, "denied");
            assert!(failures[0].1.contains("NOPERM"));
            assert!(
                failures[0].1.len() <= 1024 && failures[0].1.capacity() <= 2048,
                "Truncation must also release oversized server-error storage"
            );
            assert_eq!(peer.await??, ["first", "denied", "last"]);
        }
    }
    Ok(())
}
