//! Server policy and protocol adapters share the original account/share model.
pub mod ftp;
pub mod http;
mod smb;
pub async fn start(
    config: tomato_core::server::ServerConfig,
) -> anyhow::Result<crate::network::RunningServer> {
    match config.kind {
        tomato_core::server::ServerType::Ftp => ftp::start(config).await,
        tomato_core::server::ServerType::Http => http::start(config).await,
        tomato_core::server::ServerType::Smb => smb::start(config).await,
    }
}
