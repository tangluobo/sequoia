use anyhow::{Result, ensure};
use std::io::{BufRead, Read, Write};
use tomato_services::{
    commands::{Command, CommandHandler, Engine, Output},
    input_share::Action,
};
pub async fn run(engine: &mut Engine) -> Result<()> {
    loop {
        let line = tokio::task::spawn_blocking(|| -> Result<String> {
            let mut line = String::new();
            std::io::stdin().lock().take(16385).read_line(&mut line)?;
            ensure!(line.len() <= 16384, "Action JSON 超过 16 KiB");
            Ok(line)
        })
        .await??;
        if line.is_empty() {
            engine
                .execute(Command::InputShare {
                    action: Action::Stop,
                })
                .await?;
            break;
        }
        let action = serde_json::from_str::<Action>(line.trim());
        let stop = matches!(&action, Ok(Action::Stop));
        let result = async {
            let action = action?;
            let output = engine.execute(Command::InputShare { action }).await?;
            if let Output::InputShare(snapshot) = output {
                Ok::<_, anyhow::Error>(snapshot)
            } else {
                anyhow::bail!("意外的服务结果")
            }
        }
        .await;
        let value = match result {
            Ok(snapshot) => serde_json::to_value(snapshot)?,
            Err(error) => serde_json::json!({"error":error.to_string()}),
        };
        println!("{}", serde_json::to_string(&value)?);
        std::io::stdout().flush()?;
        if stop {
            break;
        }
    }
    Ok(())
}
