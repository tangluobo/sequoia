use super::{Body, Response, StatusCode, Tokens, response};
use anyhow::{Result, ensure};
use std::{fmt::Write, path::Path};
use tomato_core::server::SharedDirectory;

pub(super) fn segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn html(body: String) -> Response<Body> {
    let mut response = response(StatusCode::OK, "text/html; charset=utf-8", body);
    response.headers_mut().insert(
        http::header::CACHE_CONTROL,
        "no-store".parse().expect("static header"),
    );
    response.headers_mut().insert(
        http::header::REFERRER_POLICY,
        "no-referrer".parse().expect("static header"),
    );
    response
}
pub(super) fn welcome() -> Response<Body> {
    html(concat!("<html><head><title>HTTP File Server</title>",
        "<style>body{font-family:Segoe UI,Arial;padding:40px;background:#f5f5f5}",
        ".box{background:#fff;padding:40px;border-radius:8px;box-shadow:0 2px 8px rgba(0,0,0,.08);max-width:500px;margin:0 auto;text-align:center}",
        "h1{color:#07c160;margin:0 0 16px}p{color:#666;line-height:1.8}</style></head>",
        "<body><div class=\"box\"><h1>HTTP File Server</h1><p>服务器运行正常。<br>请在软件中配置共享目录后访问。</p></div></body></html>").into())
}
pub(super) fn shares<'a>(
    shares: impl Iterator<Item = &'a SharedDirectory>,
    tokens: &Tokens,
) -> Response<Body> {
    let mut body = concat!("<html><head><title>HTTP File Server - 共享目录</title>",
        "<style>body{font-family:Segoe UI,Arial;padding:30px;background:#f5f5f5}",
        "h1{color:#333;margin:0 0 24px;font-size:22px}",
        ".card{background:#fff;padding:20px;margin:12px 0;border-radius:8px;box-shadow:0 2px 6px rgba(0,0,0,.06)}",
        "a{color:#07c160;text-decoration:none;font-weight:bold;font-size:16px}",
        "a:hover{text-decoration:underline}.desc{color:#888;font-size:12px;margin-top:6px}</style></head>",
        "<body><h1>📁 共享目录列表</h1>").to_owned();
    for share in shares {
        let link = tokens.link(&format!("/{}/", segment(&share.alias)));
        let _ = write!(
            body,
            "<div class=\"card\"><a href=\"{}\">📂 {}</a><div class=\"desc\">路径: {} | {}</div></div>",
            escape(&link),
            escape(&share.alias),
            escape(&share.path),
            if share.read_only {
                "🔒 只读"
            } else {
                "✏️ 可写"
            }
        );
    }
    body.push_str("</body></html>");
    html(body)
}
pub(super) async fn directory(
    target: &Path,
    raw: &str,
    context: &str,
    tokens: &Tokens,
) -> Result<Response<Body>> {
    let title = format!(
        "📂 {}",
        escape(&target.file_name().unwrap_or_default().to_string_lossy())
    );
    let mut body = format!("<html><head><title>{title}</title><style>");
    body.push_str(concat!("body{font-family:Segoe UI,Arial;padding:20px;background:#f5f5f5}",
        "h1{color:#333;font-size:18px;margin:0 0 16px}",
        "table{width:100%;background:#fff;border-radius:8px;overflow:hidden;box-shadow:0 2px 6px rgba(0,0,0,.06);border-collapse:collapse}",
        "th,td{padding:10px 14px;text-align:left;border-bottom:1px solid #f0f0f0;font-size:13px}",
        "th{background:#fafafa;color:#666;font-weight:600}tr:hover td{background:#f9f9f9}",
        "a{color:#07c160;text-decoration:none}.up{color:#999;font-size:12px;margin-bottom:12px;display:inline-block}</style></head>"));
    let _ = write!(body, "<body><h1>{title}</h1>");
    let current = format!("{}/", raw.trim_end_matches('/'));
    if current != context {
        let parent = current
            .trim_end_matches('/')
            .rsplit_once('/')
            .map_or("/", |(p, _)| p);
        let parent = format!("{}/", parent.trim_end_matches('/'));
        let _ = write!(
            body,
            "<a class=\"up\" href=\"{}\">⬆️ 返回上级</a>",
            escape(&tokens.link(&parent))
        );
    }
    body.push_str("<table><tr><th>名称</th><th>大小</th><th>修改时间</th></tr>");
    let mut directory = tokio::fs::read_dir(target).await?;
    let mut entries = Vec::new();
    while let Some(entry) = directory.next_entry().await? {
        ensure!(entries.len() < 20_000, "目录超过 20000 项，请缩小共享范围");
        let metadata = entry.metadata().await?;
        if entry.file_type().await?.is_symlink() {
            continue;
        }
        // Prevent listing junction contents as downloadable links.
        if !tokio::fs::canonicalize(entry.path())
            .await?
            .starts_with(target)
        {
            continue;
        }
        entries.push((entry.file_name().to_string_lossy().into_owned(), metadata));
    }
    entries.sort_by(|(a, am), (b, bm)| {
        bm.is_dir()
            .cmp(&am.is_dir())
            .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
    });
    for (name, metadata) in entries {
        let directory = metadata.is_dir();
        let icon = if directory { "📁" } else { "📄" };
        let size = if directory {
            "-".into()
        } else {
            size(metadata.len())
        };
        let modified: chrono::DateTime<chrono::Local> = metadata.modified()?.into();
        let link = tokens.link(&format!(
            "{current}{}{}",
            segment(&name),
            if directory { "/" } else { "" }
        ));
        let _ = write!(
            body,
            "<tr><td><a href=\"{}\">{icon} {}</a></td><td>{size}</td><td>{}</td></tr>",
            escape(&link),
            escape(&name),
            modified.format("%Y-%m-%d %H:%M:%S")
        );
    }
    body.push_str("</table></body></html>");
    Ok(html(body))
}
fn size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.)
    } else {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.)
    }
}
