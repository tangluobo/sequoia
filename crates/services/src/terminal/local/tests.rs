use super::*;
#[test]
#[cfg(windows)]
fn dropping_pty_closes_input_and_output_before_waiting_for_the_console_host() {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut connection = Connection::new("LOCAL_TERMINAL");
            connection.set("terminalType", "cmd");
            // Cover both the initial cursor query and a full bounded output queue.
            LocalTerminal::start(&connection)?.shutdown();
            let terminal = LocalTerminal::start(&connection)?;
            terminal.write("\x1b[1;1R\r\nfor /L %i in (1,1,200000) do @echo terminal-disposal-backpressure-fixture\r\n")?;
            std::thread::sleep(std::time::Duration::from_secs(1));
            // The view is no longer consuming output. Large paste attempts
            // must queue or fail promptly even if the PTY input pipe is full.
            let paste = "x".repeat(1024 * 1024);
            for _ in 0..9 {
                if terminal.write(&paste).is_err() {
                    break;
                }
            }
            terminal.resize(40, 100)?;
            terminal.shutdown();
            Ok(())
        })();
        let _ = send.send(result);
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("PTY input, resize, or shutdown blocked the caller")
        .unwrap();
}
