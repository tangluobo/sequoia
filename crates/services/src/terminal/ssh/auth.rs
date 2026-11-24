//! Credential strategies and interactive authentication, separate from shell I/O.
use super::{Client, Events, Input, Prompt, timed};
use anyhow::{Result, ensure};
use russh::{
    client,
    keys::{self, PrivateKeyWithHashAlg},
};
use std::sync::Arc;
use tokio::sync::mpsc;
use tomato_core::Connection;

pub(super) async fn authenticate(
    session: &mut client::Handle<Client>,
    connection: &Connection,
    events: &Events,
    input: &mut mpsc::Receiver<Input>,
) -> Result<()> {
    let user = connection.text("username");
    let mut password = connection.text("password").to_string();
    let mut authenticated = timed(session.authenticate_none(user)).await?.success();
    let mut partial = false;
    let use_password = connection
        .options
        .get("usePassword")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let mut key_error = None;
    if !authenticated
        && connection.flag("useKey")
        && let Some(paths) = connection
            .options
            .get("privateKeyPaths")
            .and_then(serde_json::Value::as_array)
    {
        for path in paths
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|p| !p.is_empty())
        {
            let path = path.to_string();
            let mut loaded = load_key(path.clone(), password.clone()).await?;
            if matches!(loaded, Err(keys::Error::KeyIsEncrypted)) {
                let passphrase = events
                    .ask(
                        "SSH 私钥口令".into(),
                        vec![Prompt {
                            label: path.clone(),
                            echo: false,
                        }],
                        input,
                    )
                    .await?
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                loaded = load_key(path, passphrase).await?;
            }
            let key = match loaded {
                Ok(key) => key,
                Err(error) => {
                    key_error = Some(error.to_string());
                    continue;
                }
            };
            let hash = timed(session.best_supported_rsa_hash()).await?.flatten();
            let result = timed(
                session
                    .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)),
            )
            .await?;
            authenticated = result.success();
            partial = matches!(
                result,
                client::AuthResult::Failure {
                    partial_success: true,
                    ..
                }
            );
            if authenticated || partial {
                break;
            }
        }
    }
    if !authenticated {
        if use_password && !partial && password.is_empty() {
            password = events
                .ask(
                    "SSH 密码".into(),
                    vec![Prompt {
                        label: "密码".into(),
                        echo: false,
                    }],
                    input,
                )
                .await?
                .into_iter()
                .next()
                .unwrap_or_default();
        }
        if use_password && !partial {
            let result = timed(session.authenticate_password(user, &password)).await?;
            authenticated = result.success();
            partial = matches!(
                result,
                client::AuthResult::Failure {
                    partial_success: true,
                    ..
                }
            );
        }
        if !authenticated {
            let mut response =
                timed(session.authenticate_keyboard_interactive_start(user, None::<String>))
                    .await?;
            loop {
                match response {
                    client::KeyboardInteractiveAuthResponse::Success => {
                        authenticated = true;
                        break;
                    }
                    client::KeyboardInteractiveAuthResponse::Failure { .. } => break,
                    client::KeyboardInteractiveAuthResponse::InfoRequest {
                        name,
                        instructions,
                        prompts,
                    } => {
                        let answers = if prompts.is_empty() {
                            Vec::new()
                        } else if use_password
                            && !partial
                            && !password.is_empty()
                            && prompts.len() == 1
                            && prompts[0]
                                .prompt
                                .trim()
                                .trim_end_matches(':')
                                .eq_ignore_ascii_case("password")
                        {
                            vec![password.clone()]
                        } else {
                            events
                                .ask(
                                    format!("{name}\n{instructions}"),
                                    prompts
                                        .into_iter()
                                        .map(|p| Prompt {
                                            label: p.prompt,
                                            echo: p.echo,
                                        })
                                        .collect(),
                                    input,
                                )
                                .await?
                        };
                        response =
                            timed(session.authenticate_keyboard_interactive_respond(answers))
                                .await?;
                    }
                }
            }
        }
    }
    ensure!(
        authenticated,
        "SSH 认证失败，请检查用户名、密码或私钥{}",
        key_error
            .map(|error| format!("：{error}"))
            .unwrap_or_default()
    );
    password.clear();
    Ok(())
}

async fn load_key(
    path: String,
    password: String,
) -> Result<std::result::Result<keys::PrivateKey, keys::Error>> {
    Ok(tokio::task::spawn_blocking(move || {
        keys::load_secret_key(
            path,
            if password.is_empty() {
                None
            } else {
                Some(&password)
            },
        )
    })
    .await?)
}
