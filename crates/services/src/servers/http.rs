//! Native bounded HTTP file service; one owned task tree per listening server.
mod pages;
mod token;
use anyhow::{Context, Result, ensure};
use axum::body::Body;
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use http::{Method, Request, Response, StatusCode, header};
use hyper::{body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{convert::Infallible, path::PathBuf, sync::Arc, time::Duration};
use token::Tokens;
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::watch, task::JoinSet};
use tomato_core::server::{ServerConfig, SharedDirectory};

pub async fn start(config: ServerConfig) -> Result<crate::network::RunningServer> {
    validate(&config)?;
    let listener = TcpListener::bind((config.bind_address.as_str(), config.port)).await?;
    let address = listener.local_addr()?;
    let (updates, current) = watch::channel(config);
    let tokens = Arc::new(Tokens::new()?);
    let task = tokio::spawn(async move {
        let mut clients = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept(), if clients.len() < 128 => {
                    let Ok((stream, _)) = accepted else { break; };
                    let current = current.clone();
                    let tokens = tokens.clone();
                    clients.spawn(async move {
                        let service = service_fn(move |request: Request<Incoming>| {
                            let config = current.borrow().clone();
                            let tokens = tokens.clone();
                            async move {
                                let head = request.method() == Method::HEAD;
                                let request = request.map(Body::new);
                                let mut response = match handle(request, &config, &tokens).await {
                                    Ok(response) => response,
                                    Err(error) => {
                                        let status = error.downcast_ref::<std::io::Error>().map_or(StatusCode::INTERNAL_SERVER_ERROR, |e| match e.kind() {
                                            std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
                                            std::io::ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
                                            _ => StatusCode::INTERNAL_SERVER_ERROR,
                                        });
                                        failure(status)
                                    }
                                };
                                if head { *response.body_mut() = Body::empty(); }
                                Ok::<_, Infallible>(response)
                            }
                        });
                        // Idle headers are bounded; active large transfers have no absolute lifetime cap.
                        let _ = http1::Builder::new().timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(30))
                            .max_buf_size(64 * 1024).serve_connection(TokioIo::new(stream), service).await;
                    });
                }
                _ = clients.join_next(), if !clients.is_empty() => {}
            }
        }
    });
    Ok(crate::network::RunningServer::from_task(address, task).with_updates(updates))
}

fn validate(config: &ServerConfig) -> Result<()> {
    config.validate()?;
    ensure!(
        config.anonymous_access
            || config.accounts.iter().any(|a| a.enabled)
            || (config.shared_directories.is_empty() && config.root_directory.is_empty()),
        "匿名访问已关闭，请至少配置一个启用账号后再启动HTTP文件服务"
    );
    Ok(())
}
fn account(request: &Request<Body>, config: &ServerConfig) -> Option<String> {
    let value = request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, encoded) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Basic") || encoded.len() > 8192 {
        return None;
    }
    let bytes = STANDARD.decode(encoded).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let (name, password) = text.split_once(':')?;
    config
        .accounts
        .iter()
        .find(|a| a.enabled && a.username == name && a.password == password)
        .map(|a| a.username.clone())
}
fn allowed(share: &SharedDirectory, user: Option<&str>) -> bool {
    share.allowed_users.is_empty()
        || user.is_some_and(|u| share.allowed_users.iter().any(|s| s == u))
}
fn query(request: &Request<Body>, name: &str) -> Option<String> {
    url::form_urlencoded::parse(request.uri().query()?.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}
fn decode_path(raw: &str) -> Option<String> {
    // Java's URLDecoder uses form encoding for path segments, including '+'.
    let bytes = raw.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'%'
            && (i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit())
        {
            return None;
        }
    }
    percent_encoding::percent_decode_str(&raw.replace('+', " "))
        .decode_utf8()
        .ok()
        .map(|p| p.into_owned())
}
async fn target(root: &str, relative: &str) -> Result<Option<(PathBuf, PathBuf)>> {
    let root = tokio::fs::canonicalize(root).await?;
    if !root.is_dir() {
        anyhow::bail!("共享路径不是目录");
    }
    let mut target = root.clone();
    for name in relative.split('/').filter(|s| !s.is_empty() && *s != ".") {
        if name == ".." || name.contains(['\\', ':', '\0']) {
            return Ok(None);
        }
        target.push(name);
        match tokio::fs::symlink_metadata(&target).await {
            Ok(metadata) => {
                // Canonical checks also catch Windows directory junctions.
                if metadata.is_symlink()
                    || !tokio::fs::canonicalize(&target).await?.starts_with(&root)
                {
                    return Ok(None);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Some((root, target)))
}
async fn handle(
    request: Request<Body>,
    config: &ServerConfig,
    tokens: &Tokens,
) -> Result<Response<Body>> {
    let raw = request.uri().path();
    if raw.len() > 16 * 1024 {
        return Ok(failure(StatusCode::URI_TOO_LONG));
    }
    let Some(path) = decode_path(raw) else {
        return Ok(failure(StatusCode::BAD_REQUEST));
    };
    let user = account(&request, config);
    let read = matches!(*request.method(), Method::GET | Method::HEAD);
    let token = query(&request, "token").is_some_and(|t| tokens.valid(request.method(), raw, &t));
    let authorized = user.is_some() || (read && config.anonymous_access);
    if config.shared_directories.is_empty() && config.root_directory.is_empty() {
        return Ok(pages::welcome());
    }
    let (share, relative, context) = if config.shared_directories.is_empty() {
        (
            SharedDirectory {
                alias: "root".into(),
                path: config.root_directory.clone(),
                ..Default::default()
            },
            path.trim_start_matches('/').to_owned(),
            "/".into(),
        )
    } else {
        let (alias, relative) = path
            .trim_start_matches('/')
            .split_once('/')
            .unwrap_or((path.trim_start_matches('/'), ""));
        if alias.is_empty() {
            if !authorized {
                return Ok(unauthorized());
            }
            return Ok(pages::shares(
                config
                    .shared_directories
                    .iter()
                    .filter(|s| allowed(s, user.as_deref())),
                tokens,
            ));
        }
        let Some(share) = config.shared_directories.iter().find(|s| s.alias == alias) else {
            return Ok(failure(StatusCode::NOT_FOUND));
        };
        (
            share.clone(),
            relative.to_owned(),
            format!("/{}/", pages::segment(alias)),
        )
    };
    if !token {
        if !authorized {
            return Ok(unauthorized());
        }
        if !allowed(&share, user.as_deref()) {
            return Ok(failure(StatusCode::FORBIDDEN));
        }
    }
    if !read && share.read_only {
        return Ok(failure(StatusCode::FORBIDDEN));
    }
    let Some((root, target)) = target(&share.path, &relative).await? else {
        return Ok(failure(StatusCode::FORBIDDEN));
    };
    if read {
        let metadata = tokio::fs::metadata(&target).await?;
        if metadata.is_dir() {
            return pages::directory(&target, raw, &context, tokens).await;
        }
        let file = tokio::fs::File::open(&target).await?;
        let modified: chrono::DateTime<chrono::Utc> = metadata.modified()?.into();
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                mime_guess::from_path(&target)
                    .first_or_octet_stream()
                    .as_ref(),
            )
            .header(header::CONTENT_LENGTH, metadata.len())
            .header(
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename*=UTF-8''{}",
                    pages::segment(&target.file_name().unwrap_or_default().to_string_lossy())
                        .replace('+', "%20")
                ),
            )
            .header(
                header::LAST_MODIFIED,
                modified.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
            )
            .body(Body::from_stream(
                tokio_util::io::ReaderStream::with_capacity(file, 64 * 1024),
            ))?);
    }
    if target == root {
        return Ok(failure(StatusCode::FORBIDDEN));
    }
    match *request.method() {
        Method::PUT => {
            let parent = target.parent().context("文件没有父目录")?;
            tokio::fs::create_dir_all(parent).await?;
            let parent = parent.to_owned();
            let temporary =
                tokio::task::spawn_blocking(move || tempfile::NamedTempFile::new_in(parent))
                    .await??;
            let (file, temporary) = temporary.into_parts();
            let mut file = tokio::fs::File::from_std(file);
            let mut stream = request.into_body().into_data_stream();
            let mut total = 0u64;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                file.write_all(&chunk).await?;
                total += chunk.len() as u64;
            }
            file.sync_all().await?;
            drop(file);
            tokio::task::spawn_blocking(move || temporary.persist(target)).await??;
            Ok(json(
                StatusCode::CREATED,
                format!("{{\"ok\":true,\"size\":{total}}}"),
            ))
        }
        Method::DELETE => {
            if tokio::fs::metadata(&target).await?.is_dir() {
                tokio::fs::remove_dir_all(target).await?;
            } else {
                tokio::fs::remove_file(target).await?;
            }
            Ok(json(StatusCode::OK, "{\"ok\":true}".into()))
        }
        Method::POST if query(&request, "action").as_deref() == Some("mkdir") => {
            tokio::fs::create_dir_all(target).await?;
            Ok(json(StatusCode::OK, "{\"ok\":true}".into()))
        }
        _ => Ok(failure(StatusCode::METHOD_NOT_ALLOWED)),
    }
}
fn json(status: StatusCode, body: String) -> Response<Body> {
    response(status, "application/json", body)
}
fn response(status: StatusCode, content_type: &str, body: String) -> Response<Body> {
    let length = body.len();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        content_type.parse().expect("static content type"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, length.into());
    response
}
fn failure(status: StatusCode) -> Response<Body> {
    response(
        status,
        "text/html; charset=utf-8",
        format!(
            "<html><body><h1>{} {}</h1></body></html>",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Error")
        ),
    )
}
fn unauthorized() -> Response<Body> {
    let mut response = failure(StatusCode::UNAUTHORIZED);
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        "Basic realm=\"File Server\""
            .parse()
            .expect("static header"),
    );
    response
}
