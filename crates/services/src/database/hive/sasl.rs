//! Thrift SASL and RFC 4752 GSSAPI. Credentials stay in the system ticket cache.
use super::{config::Config, transport::Socket};
use anyhow::{Context, Result, ensure};
use cross_krb5::{ClientCtx, InitiateFlags, K5Ctx as _, PendingClientCtx, Step};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const MAX_TOKEN: usize = 1024 * 1024;
pub(super) const MAX_FRAME: usize = 8 * 1024 * 1024;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Qop {
    Auth,
    Integrity,
    Confidentiality,
}
impl Qop {
    fn parse(value: &str) -> Result<Vec<Self>> {
        let value = if value.is_empty() { "auth" } else { value };
        value
            .split(',')
            .map(|v| {
                Ok(match v.trim().to_ascii_lowercase().as_str() {
                    "auth" => Self::Auth,
                    "auth-int" => Self::Integrity,
                    "auth-conf" => Self::Confidentiality,
                    _ => anyhow::bail!("Hive sasl.qop 需要为 auth、auth-int 或 auth-conf"),
                })
            })
            .collect()
    }
    fn bit(self) -> u8 {
        match self {
            Self::Auth => 1,
            Self::Integrity => 2,
            Self::Confidentiality => 4,
        }
    }
}
pub(super) struct Layer {
    context: ClientCtx,
    qop: Qop,
    peer_max: usize,
}
impl Layer {
    pub fn payload(&self) -> usize {
        (self.peer_max - 1024).min(64 * 1024)
    }
    pub fn wrap(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        let token = self
            .context
            .wrap(self.qop == Qop::Confidentiality, bytes)?
            .to_vec();
        ensure!(
            token.len() <= self.peer_max,
            "Hive SASL 输出超过服务器缓冲上限"
        );
        Ok(token)
    }
    pub fn unwrap(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        let token = self.context.unwrap(bytes)?.to_vec();
        ensure!(
            !token.is_empty() && token.len() <= MAX_FRAME,
            "Hive SASL 解码长度无效"
        );
        Ok(token)
    }
}
async fn send(socket: &mut Socket, status: u8, bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= MAX_TOKEN, "Hive SASL 认证令牌过大");
    socket.write_u8(status).await?;
    socket.write_u32(bytes.len() as u32).await?;
    socket.write_all(bytes).await?;
    socket.flush().await?;
    Ok(())
}
async fn receive(socket: &mut Socket) -> Result<(u8, Vec<u8>)> {
    let status = socket.read_u8().await?;
    let length = socket.read_u32().await? as usize;
    ensure!(length <= MAX_TOKEN, "Hive SASL 认证响应过大");
    let mut bytes = vec![0; length];
    socket.read_exact(&mut bytes).await?;
    // Never forward the peer's token or error payload to logs.
    ensure!(matches!(status, 2 | 5), "Hive SASL 服务器拒绝了认证");
    Ok((status, bytes))
}
pub(super) fn principal(config: &Config) -> Result<String> {
    let principal = config.option("principal");
    ensure!(
        !principal.is_empty(),
        "Hive Kerberos 需要 principal=服务/主机@域"
    );
    let (service, remaining) = principal
        .split_once('/')
        .context("Hive Kerberos principal 缺少服务或主机")?;
    let (host, realm) = remaining
        .rsplit_once('@')
        .context("Hive Kerberos principal 缺少域")?;
    ensure!(
        !service.is_empty()
            && !host.is_empty()
            && !realm.is_empty()
            && !principal
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            && !remaining.contains(['\\', '\0']),
        "Hive Kerberos principal 无效"
    );
    // _HOST always names the original server, including over an SSH tunnel.
    Ok(format!(
        "{service}/{}@{realm}",
        if host == "_HOST" {
            config.host.as_str()
        } else {
            host
        }
    ))
}

/// Hive's JDBC HTTP transport sends a fresh, one-shot Kerberos service ticket.
/// It deliberately does not request a mutual-authentication token: the Hive
/// servlet does not return AP-REP. HTTPS still verifies the original server.
pub(super) async fn http_ticket(principal: String) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || -> Result<_> {
        let (_, token) = ClientCtx::new(
            InitiateFlags::DISABLE_MUTUAL_AUTH | InitiateFlags::DISABLE_CONFIDENTIALITY,
            None,
            &principal,
            None,
        )
        .context("无法获取 Hive HTTP Kerberos 票据，请先登录系统 Kerberos 或配置凭据缓存")?;
        ensure!(token.len() <= MAX_TOKEN, "Hive HTTP Kerberos 令牌过大");
        Ok(token.to_vec())
    })
    .await?
}
fn selection(offer: &[u8], choices: &[Qop]) -> Result<(Qop, usize, [u8; 4])> {
    ensure!(offer.len() == 4, "Hive GSSAPI 安全层响应长度无效");
    let maximum = u32::from_be_bytes([0, offer[1], offer[2], offer[3]]) as usize;
    ensure!(
        offer[0] & 6 != 0 || maximum == 0,
        "Hive GSSAPI 无安全层时缓冲大小必须为0"
    );
    let qop = choices
        .iter()
        .copied()
        .find(|qop| offer[0] & qop.bit() != 0)
        .context("Hive 服务器不支持配置的 sasl.qop，未降低安全级别")?;
    ensure!(
        qop == Qop::Auth || maximum > 1024,
        "Hive GSSAPI 服务器缓冲过小"
    );
    let bytes = if qop == Qop::Auth {
        [1, 0, 0, 0]
    } else {
        [qop.bit(), 0x80, 0, 0]
    };
    Ok((qop, maximum, bytes))
}
enum ContextState {
    Pending(PendingClientCtx),
    Established(ClientCtx),
}
pub(super) async fn authenticate(socket: &mut Socket, config: &Config) -> Result<Option<Layer>> {
    if !config.kerberos() {
        let user = if config.username.is_empty() {
            "anonymous"
        } else {
            &config.username
        };
        let password = if config.password.is_empty() {
            "anonymous"
        } else {
            &config.password
        };
        ensure!(
            !user.contains('\0') && !password.contains('\0'),
            "Hive SASL 凭据包含 NUL"
        );
        send(socket, 1, b"PLAIN").await?;
        send(socket, 2, format!("\0{user}\0{password}").as_bytes()).await?;
        let (status, _) = receive(socket).await?;
        ensure!(status == 5, "Hive SASL 密码认证未完成");
        return Ok(None);
    }
    let principal = principal(config)?;
    let choices = Qop::parse(config.option("sasl.qop"))?;
    let authzid = config.option("sasl.authorization.id").to_owned();
    ensure!(
        authzid.len() <= 4096 && !authzid.contains('\0'),
        "Hive 授权标识无效"
    );
    let (pending, token) = tokio::task::spawn_blocking(move || -> Result<_> {
        let (context, token) = ClientCtx::new(InitiateFlags::default(), None, &principal, None)
            .context("无法获取 Hive Kerberos 票据，请先登录系统 Kerberos 或配置凭据缓存")?;
        Ok((context, token.to_vec()))
    })
    .await??;
    send(socket, 1, b"GSSAPI").await?;
    send(socket, 2, &token).await?;
    let mut context = ContextState::Pending(pending);
    for _ in 0..32 {
        let (status, token) = receive(socket).await?;
        ensure!(status == 2, "Hive GSSAPI 认证提前结束");
        match context {
            ContextState::Pending(pending) => {
                let (next, token) = tokio::task::spawn_blocking(move || -> Result<_> {
                    Ok(match pending.step(&token)? {
                        Step::Continue((context, token)) => {
                            (ContextState::Pending(context), token.to_vec())
                        }
                        Step::Finished((context, token)) => (
                            ContextState::Established(context),
                            token.map(|t| t.to_vec()).unwrap_or_default(),
                        ),
                    })
                })
                .await??;
                send(socket, 2, &token).await?;
                context = next;
            }
            ContextState::Established(mut context) => {
                let offer = context.unwrap(&token)?.to_vec();
                let (qop, maximum, header) = selection(&offer, &choices)?;
                let mut choice = header.to_vec();
                choice.extend_from_slice(authzid.as_bytes());
                let token = context.wrap(false, &choice)?.to_vec();
                send(socket, 5, &token).await?;
                let (status, bytes) = receive(socket).await?;
                ensure!(
                    status == 5 && bytes.is_empty(),
                    "Hive GSSAPI 安全层确认失败"
                );
                return Ok((qop != Qop::Auth).then_some(Layer {
                    context,
                    qop,
                    peer_max: maximum.min(MAX_FRAME),
                }));
            }
        }
    }
    anyhow::bail!("Hive GSSAPI 认证轮数超过32次")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gssapi_uses_original_host_and_never_silently_downgrades_qop() -> Result<()> {
        let mut connection = tomato_core::Connection::new("HIVE");
        connection.set("host", "hive.internal.test");
        connection.set("port", 10000);
        connection.set(
            "database",
            "default;principal=hive/_HOST@FIXTURE.TEST;sasl.qop=auth-conf,auth-int",
        );
        let config = Config::new(&connection, "", None)?;
        assert_eq!(principal(&config)?, "hive/hive.internal.test@FIXTURE.TEST");
        let choices = Qop::parse(config.option("sasl.qop"))?;
        assert!(selection(&[1, 0, 0, 0], &choices).is_err());
        let (qop, max, reply) = selection(&[7, 0, 0xff, 0xff], &choices)?;
        assert!(qop == Qop::Confidentiality);
        assert_eq!(max, 65535);
        assert_eq!(reply, [4, 0x80, 0, 0]);
        assert!(selection(&[1, 0, 1, 0], &[Qop::Auth]).is_err());
        assert_eq!(selection(&[1, 0, 0, 0], &[Qop::Auth])?.2, [1, 0, 0, 0]);
        assert!(selection(&[7, 0, 0, 0], &choices).is_err());
        assert!(Qop::parse("invalid").is_err());
        Ok(())
    }
}
