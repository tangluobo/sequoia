//! SSH and SFTP operations for unattended certificate deployment.
use super::*;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use tokio::io::AsyncWriteExt;

pub(crate) async fn certificate(
    connection: &Connection,
    known_hosts: PathBuf,
    directory: &str,
    reload: &str,
    chain: &[u8],
    key: &[u8],
) -> Result<()> {
    let (input, mut receiver) = mpsc::channel(1);
    drop(input); // Scheduled deployments cannot answer interactive authentication prompts.
    let events = Events {
        state: Arc::new(Mutex::new(Status::Connecting)),
        notifier: Default::default(),
    };
    let connect::Connected {
        session,
        _parents,
        _socket_guard,
    } = connect::authenticated(connection, known_hosts, &mut receiver, &events)
        .await
        .context("无法使用已保存的 SSH 配置连接部署主机")?;
    let tools = tools::Contexts {
        session: Arc::new(session),
        sftp: Default::default(),
        editors: Default::default(),
    };
    let sftp = tools.sftp().await.context("无法启动证书部署的 SFTP 会话")?;
    let version = format!(".tomato-certificate-{}", uuid::Uuid::new_v4().simple());
    let stage = format!("{directory}/{version}");
    tools
        .exec(&format!(
            "umask 077; mkdir -p -- {} && mkdir -- {}",
            quote(directory),
            quote(&stage)
        ))
        .await
        .context("无法创建远程证书目录")?;
    let files = [("fullchain.pem", chain), ("privkey.pem", key)];
    let uploaded = async {
        for (file, bytes) in files {
            let mut permissions = FileAttributes::empty();
            permissions.permissions = Some(0o600);
            let mut handle = sftp
                .open_with_flags_and_attributes(
                    format!("{stage}/{file}"),
                    OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
                    permissions,
                )
                .await?;
            handle.write_all(bytes).await?;
            handle.close().await?;
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if let Err(error) = uploaded {
        for (file, _) in files {
            let _ = sftp.remove_file(format!("{stage}/{file}")).await;
        }
        let _ = sftp.remove_dir(&stage).await;
        return Err(error.context("上传远程证书文件失败"));
    }
    // Keep the pair together. Leave an uncertain version intact if the SSH
    // channel disconnects during activation, since it may already be in use.
    tools
        .exec(&activation_script(directory, &version, reload))
        .await
        .map_err(|_| anyhow::anyhow!("启用远程证书或执行部署命令失败，请检查目标目录和服务命令"))?;
    Ok(())
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn activation_script(directory: &str, version: &str, reload: &str) -> String {
    format!(
        r#"set -eu
cd -- {}
version={}
reload={}
link="$version-link"
cleanup() {{
    rm -f -- "$link" || :
    if [ "$(readlink current 2>/dev/null || :)" != "$version" ]; then
        rm -f -- "$version/fullchain.pem" "$version/privkey.pem" || :
        rmdir -- "$version" 2>/dev/null || :
    fi
}}
trap cleanup EXIT
old=''
if [ -L current ]; then old=$(readlink current); elif [ -e current ]; then exit 1; fi
ln -s -- "$version" "$link"
mv -Tf -- "$link" current
if [ -n "$reload" ] && ! sh -c "$reload"; then
    if [ -n "$old" ]; then
        ln -s -- "$old" "$link"
        mv -Tf -- "$link" current
    else
        rm -f -- current
    fi
    exit 1
fi
case "$old" in
    .tomato-certificate-*)
        case "${{old#.tomato-certificate-}}" in
            ''|*[!0-9a-f]*) ;;
            *) rm -f -- "$old/fullchain.pem" "$old/privkey.pem" || :; rmdir -- "$old" 2>/dev/null || : ;;
        esac
        ;;
esac"#,
        quote(directory),
        quote(version),
        quote(reload)
    )
}
