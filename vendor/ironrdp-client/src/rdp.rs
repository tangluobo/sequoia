mod diagnostics;
mod graphics;
mod input;
pub(crate) mod redirection;
use core::net::SocketAddr;
use core::num::NonZeroU16;
use core::time::Duration;
pub use input::{FastPathSender, InputDispatchStats, InputSender};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ironrdp_connector::connection_activation::ConnectionActivationState;
use ironrdp_connector::{ConnectionResult, ConnectorResult};
use ironrdp_core::WriteBuf;
use ironrdp_displaycontrol::client::DisplayControlClient;
use ironrdp_displaycontrol::pdu::MonitorLayoutEntry;
#[cfg(all(windows, feature = "dvc-com-plugin"))]
use ironrdp_dvc::DvcProcessor as _;
use ironrdp_echo::client::EchoClient;
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_graphics::pointer::DecodedPointer;
use ironrdp_pdu::input::MousePdu;
use ironrdp_pdu::input::fast_path::FastPathInputEvent;
use ironrdp_pdu::input::mouse::PointerFlags;
#[cfg(any(feature = "dvc-pipe-proxy", all(windows, feature = "dvc-com-plugin")))]
use ironrdp_pdu::pdu_other_err;
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{
    ActiveStageBuilder, ActiveStageOutput, GracefulDisconnectReason, SessionResult, fast_path,
};
use ironrdp_svc::SvcMessage;
use ironrdp_tokio::reqwest::ReqwestNetworkClient;
use ironrdp_tokio::{FramedWrite, single_sequence_step_read, split_tokio_framed};
use smallvec::SmallVec;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
#[cfg(any(feature = "clipboard", all(windows, feature = "dvc-com-plugin")))]
use tracing::error;
#[cfg(feature = "clipboard")]
use tracing::warn;
use tracing::{debug, info, trace};

#[cfg(feature = "clipboard")]
use crate::config::ClipboardType;
#[cfg(feature = "clipboard")]
use ironrdp_cliprdr::backend::{ClipboardMessage, CliprdrBackendFactory};
#[cfg(all(windows, feature = "dvc-com-plugin"))]
use ironrdp_dvc_com_plugin::load_dvc_plugin;
#[cfg(feature = "dvc-pipe-proxy")]
use ironrdp_dvc_pipe_proxy::DvcNamedPipeProxy;
#[cfg(feature = "sound")]
use ironrdp_rdpsnd_native::cpal;

use crate::config::{Config, RDCleanPathConfig, Transport};

// ── Public event types ────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum RdpOutputEvent {
    Image {
        buffer: Vec<u32>,
        width: NonZeroU16,
        height: NonZeroU16,
    },
    /// Coalesced dirty pixels in row-major order; width/height are the full canvas size.
    ImageRegion {
        buffer: Vec<u32>,
        width: NonZeroU16,
        height: NonZeroU16,
        region: ironrdp_pdu::geometry::InclusiveRectangle,
    },
    /// Native opaque RGBA patches; dimensions describe the retained full canvas.
    RgbaRegions {
        width: NonZeroU16,
        height: NonZeroU16,
        regions: Vec<(ironrdp_pdu::geometry::InclusiveRectangle, Vec<u32>)>,
    },
    ConnectionFailure(ironrdp_connector::ConnectorError),
    PointerDefault,
    PointerHidden,
    PointerPosition {
        x: u16,
        y: u16,
    },
    PointerBitmap(Arc<DecodedPointer>),
    Terminated(SessionResult<GracefulDisconnectReason>),
}

#[derive(Debug)]
pub enum RdpInputEvent {
    Resize {
        width: u16,
        height: u16,
        scale_factor: u32,
        /// Physical display size in millimetres (width, height).
        physical_size: Option<(u32, u32)>,
    },
    FastPath(SmallVec<[FastPathInputEvent; 2]>),
    Close,
    #[cfg(feature = "clipboard")]
    Clipboard(ClipboardMessage),
    SendDvcMessages {
        channel_id: u32,
        messages: Vec<SvcMessage>,
    },
}

// ── RdpClient ─────────────────────────────────────────────────────────────────

pub struct RdpClient {
    config: Config,
    output_event_sender: mpsc::Sender<RdpOutputEvent>,
    input_event_sender: InputSender,
    input_event_receiver: mpsc::UnboundedReceiver<input::Control>,
    fast_input: Option<input::Lane>,
}

/// Cancellation handle for the native clipboard's dedicated STA.
pub struct ClipboardOwner {
    #[cfg(all(windows, feature = "clipboard"))]
    _clipboard: Option<ironrdp_cliprdr_native::ThreadedWinClipboard>,
    #[cfg(all(windows, feature = "clipboard"))]
    files: Option<std::sync::Arc<crate::clipboard::files::Bridge>>,
}
impl ClipboardOwner {
    pub fn cancellation(&self) -> impl Fn() + Send + Sync + 'static {
        #[cfg(all(windows, feature = "clipboard"))]
        let cancel = self._clipboard.as_ref().map(|owner| owner.cancellation());
        #[cfg(all(windows, feature = "clipboard"))]
        let files = self.files.clone();
        move || {
            #[cfg(all(windows, feature = "clipboard"))]
            {
                // Wake pending OLE file readers before the bounded native
                // event queue is torn down by the STA.
                if let Some(files) = &files {
                    files.close();
                }
                if let Some(cancel) = &cancel {
                    cancel();
                }
            }
        }
    }
    /// Join the native STA from the protocol worker, never the application UI.
    pub fn shutdown(self) {
        self.cancellation()();
        #[cfg(all(windows, feature = "clipboard"))]
        if let Some(owner) = self._clipboard {
            owner.shutdown();
        }
    }
}
/// Sendable protocol state; the native clipboard window itself never crosses threads.
pub struct PreparedClient {
    client: RdpClient,
    #[cfg(feature = "clipboard")]
    cliprdr_factory: Option<Box<dyn CliprdrBackendFactory + Send>>,
}

impl RdpClient {
    pub fn new(config: Config, output_event_sender: mpsc::Sender<RdpOutputEvent>) -> Self {
        let fast_input = (!config.connector.pointer_software_rendering).then(input::Lane::new);
        let (input_event_sender, input_event_receiver) = InputSender::channel(fast_input.as_ref());
        Self {
            config,
            output_event_sender,
            input_event_sender,
            input_event_receiver,
            fast_input,
        }
    }

    /// Return a clone of the input-event sender for injecting keyboard, mouse, and clipboard
    /// events from the GUI thread.
    pub fn input_sender(&self) -> InputSender {
        self.input_event_sender.clone()
    }

    /// Native pointer input is written independently of image decompression.
    pub fn fastpath_sender(&self) -> Option<FastPathSender> {
        self.fast_input.as_ref().map(input::Lane::sender)
    }

    pub fn prepare(self) -> ConnectorResult<(PreparedClient, ClipboardOwner)> {
        // ── Clipboard initialisation (compile-time gated) ─────────────────────
        //
        // On Windows the native STA outlives the connection loop; its HWND/OLE
        // resources are created and destroyed on that same dedicated thread.
        // Other platforms use the stub backend here.
        #[cfg(all(windows, feature = "clipboard"))]
        let _win_clipboard;
        #[cfg(all(windows, feature = "clipboard"))]
        let mut file_bridge = None;

        #[cfg(feature = "clipboard")]
        let cliprdr_factory: Option<Box<dyn CliprdrBackendFactory + Send>>;

        #[cfg(feature = "clipboard")]
        {
            match self.config.channels.clipboard {
                ClipboardType::Disable => {
                    cliprdr_factory = None;
                    #[cfg(windows)]
                    {
                        _win_clipboard = None;
                    }
                }
                ClipboardType::Stub => {
                    use ironrdp_cliprdr_native::StubClipboard;
                    let stub = StubClipboard::new();
                    cliprdr_factory = Some(stub.backend_factory());
                    #[cfg(windows)]
                    {
                        _win_clipboard = None;
                    }
                }
                ClipboardType::Enable => {
                    #[cfg(windows)]
                    {
                        use crate::clipboard::ClientClipboardMessageProxy;
                        use ironrdp_cliprdr_native::ThreadedWinClipboard;
                        let files =
                            crate::clipboard::files::Bridge::new(self.input_event_sender.clone());
                        let owner_bridge = files.clone();
                        match ThreadedWinClipboard::start(
                            ClientClipboardMessageProxy::with_files(
                                self.input_event_sender.clone(),
                                files.clone(),
                            ),
                            move || {
                                let mut file_owner =
                                    crate::clipboard::files::FileOwner::new(owner_bridge);
                                Box::new(move || file_owner.process_pending())
                            },
                        ) {
                            Ok((win_cb, factory)) => {
                                file_bridge = Some(files.clone());
                                cliprdr_factory = Some(files.wrap(factory));
                                _win_clipboard = Some(win_cb);
                            }
                            Err(e) => {
                                return Err(ironrdp_connector::custom_err!(
                                    "Windows clipboard initialization",
                                    e
                                ));
                            }
                        }
                    }

                    #[cfg(not(windows))]
                    {
                        use ironrdp_cliprdr_native::StubClipboard;
                        let stub = StubClipboard::new();
                        cliprdr_factory = Some(stub.backend_factory());
                    }
                }
            }
        }

        Ok((
            PreparedClient {
                client: self,
                #[cfg(feature = "clipboard")]
                cliprdr_factory,
            },
            ClipboardOwner {
                #[cfg(all(windows, feature = "clipboard"))]
                _clipboard: _win_clipboard,
                #[cfg(all(windows, feature = "clipboard"))]
                files: file_bridge,
            },
        ))
    }
    pub async fn run(self) {
        let output = self.output_event_sender.clone();
        match self.prepare() {
            Ok((client, _owner)) => client.run().await,
            Err(error) => {
                let _ = output.send(RdpOutputEvent::ConnectionFailure(error)).await;
            }
        }
    }
}
impl PreparedClient {
    pub async fn run(self) {
        let Self {
            mut client,
            #[cfg(feature = "clipboard")]
            cliprdr_factory,
        } = self;
        // Resolve the per-connection cliprdr factory reference once.  `Option<&dyn …>` is `Copy`,
        // so it can be threaded into every connect attempt across reconnects.
        #[cfg(feature = "clipboard")]
        let cliprdr_factory: CliprdrFactoryRef<'_> = cliprdr_factory.as_deref();
        #[cfg(not(feature = "clipboard"))]
        let cliprdr_factory: CliprdrFactoryRef<'_> = core::marker::PhantomData;

        // ── Connection + session loop ─────────────────────────────────────────
        let mut redirections = 0;
        // Redirect credentials are one-time. A later resize that needs a full
        // reconnect must start at the original broker, not replay a spent token.
        let mut original_config = None;
        loop {
            let (connection_result, framed) = match &client.config.transport {
                Transport::Direct => {
                    match connect_direct(
                        &client.config,
                        &client.input_event_sender,
                        cliprdr_factory,
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = client
                                .output_event_sender
                                .send(RdpOutputEvent::ConnectionFailure(e))
                                .await;
                            break;
                        }
                    }
                }

                #[cfg(feature = "gateway")]
                Transport::Gateway(gw) => {
                    match connect_gateway(
                        &client.config,
                        gw,
                        &client.input_event_sender,
                        cliprdr_factory,
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = client
                                .output_event_sender
                                .send(RdpOutputEvent::ConnectionFailure(e))
                                .await;
                            break;
                        }
                    }
                }

                Transport::RDCleanPath(rdcp) => {
                    match connect_rdcleanpath_transport(
                        &client.config,
                        rdcp,
                        &client.input_event_sender,
                        cliprdr_factory,
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = client
                                .output_event_sender
                                .send(RdpOutputEvent::ConnectionFailure(e))
                                .await;
                            break;
                        }
                    }
                }
            };

            match active_session(
                framed,
                connection_result,
                &client.output_event_sender,
                &mut client.input_event_receiver,
                client.config.fake_events_interval,
                client.fast_input.clone(),
            )
            .await
            {
                Ok(RdpControlFlow::Redirect(redirect)) => {
                    if original_config.is_none() {
                        original_config = Some(client.config.clone());
                    }
                    redirections += 1;
                    let result = if redirections > 6 {
                        Err(anyhow::anyhow!("RDP server redirection exceeded six hops"))
                    } else {
                        redirect.apply(&mut client.config)
                    };
                    if let Err(error) = result {
                        let _ = client
                            .output_event_sender
                            .send(RdpOutputEvent::ConnectionFailure(
                                ironrdp_connector::custom_err!(
                                    "Server redirection",
                                    std::io::Error::other(error.to_string())
                                ),
                            ))
                            .await;
                        break;
                    }
                    client.config.redirection = Some(Arc::new(*redirect));
                }
                Ok(RdpControlFlow::ReconnectWithNewSize { width, height }) => {
                    if let Some(original) = original_config.take() {
                        client.config = original;
                        redirections = 0;
                    }
                    client.config.connector.desktop_size.width = width;
                    client.config.connector.desktop_size.height = height;
                }
                Ok(RdpControlFlow::TerminatedGracefully(reason)) => {
                    let _ = client
                        .output_event_sender
                        .send(RdpOutputEvent::Terminated(Ok(reason)))
                        .await;
                    break;
                }
                Err(e) => {
                    let _ = client
                        .output_event_sender
                        .send(RdpOutputEvent::Terminated(Err(e)))
                        .await;
                    break;
                }
            }
        }
    }
}

// ── Connector builder ─────────────────────────────────────────────────────────

/// Reference to the cliprdr backend factory threaded into the connect helpers.
///
/// Collapses to a zero-sized placeholder when the `clipboard` feature is disabled, so the
/// connect-helper signatures don't need `#[cfg]` on this parameter.
#[cfg(feature = "clipboard")]
type CliprdrFactoryRef<'a> = Option<&'a (dyn CliprdrBackendFactory + Send)>;
#[cfg(not(feature = "clipboard"))]
type CliprdrFactoryRef<'a> = core::marker::PhantomData<&'a ()>;

/// Build a fully wired [`ironrdp_connector::ClientConnector`] with all feature-gated channels attached.
///
/// This helper is used by all transport paths. The cliprdr backend is (re)built here, per
/// connection, from `cliprdr_factory`.
fn build_connector(
    config: &Config,
    client_addr: SocketAddr,
    input_sender: &InputSender,
    cliprdr_factory: CliprdrFactoryRef<'_>,
) -> ironrdp_connector::ClientConnector {
    // `input_sender` is only consumed by the optional DVC wirings below, and `cliprdr_factory`
    // only by the optional CLIPRDR attachment; discard them explicitly when those are compiled out.
    #[cfg(not(any(feature = "dvc-pipe-proxy", all(windows, feature = "dvc-com-plugin"))))]
    let _ = input_sender;
    #[cfg(not(feature = "clipboard"))]
    let _ = cliprdr_factory;

    let mut drdynvc = ironrdp_dvc::DrdynvcClient::new()
        .with_dynamic_channel(DisplayControlClient::new(|_| Ok(Vec::new())))
        .with_dynamic_channel(EchoClient::new());
    if config.connector.support_dyn_vc_gfx_protocol {
        drdynvc = drdynvc.with_dynamic_channel(graphics::client(config.channels.graphics_h264));
    }

    // Attach DVC pipe proxies.
    #[cfg(feature = "dvc-pipe-proxy")]
    for proxy in &config.dvc_pipe_proxies {
        let channel_name = proxy.channel_name.clone();
        let pipe_name = proxy.pipe_name.clone();
        trace!(%channel_name, %pipe_name, "Creating DVC pipe proxy");
        let sender = input_sender.clone();
        drdynvc = drdynvc.with_dynamic_channel(DvcNamedPipeProxy::new(
            &channel_name,
            &pipe_name,
            move |channel_id, messages| {
                sender
                    .send(RdpInputEvent::SendDvcMessages {
                        channel_id,
                        messages,
                    })
                    .map_err(|_| pdu_other_err!("send DVC messages to the event loop"))?;
                Ok(())
            },
        ));
    }

    // Load DVC COM plugins (Windows + dvc-com-plugin feature).
    #[cfg(all(windows, feature = "dvc-com-plugin"))]
    {
        for plugin_path in &config.dvc_plugins {
            info!(dll = %plugin_path.display(), "Loading DVC COM plugin");
            let sender_clone = input_sender.clone();
            match load_dvc_plugin(plugin_path, move || {
                let sender = sender_clone.clone();
                Box::new(move |channel_id, messages| {
                    sender
                        .send(RdpInputEvent::SendDvcMessages {
                            channel_id,
                            messages,
                        })
                        .map_err(|_| pdu_other_err!("send COM DVC messages to the event loop"))?;
                    Ok(())
                })
            }) {
                Ok(channels) => {
                    for channel in channels {
                        info!(channel_name = %channel.channel_name(), "Registered COM DVC channel");
                        drdynvc = drdynvc.with_dynamic_channel(channel);
                    }
                }
                Err(e) => {
                    error!(dll = %plugin_path.display(), error = %e, "Failed to load DVC COM plugin");
                }
            }
        }
    }

    // Attach user-defined DVC channels from the extension registry.
    for attach_dvc in &config.extensions.dvc_channels {
        attach_dvc(&mut drdynvc, &config.properties);
    }

    // Clone the connector config so we can apply runtime overrides before handing it to the
    // connector.  We want to set `enable_audio_playback` consistently with `channels.sound`.
    let mut connector_config = config.connector.clone();

    // If sound is disabled at runtime (or the feature is off) ensure the connector doesn't
    // advertise audio support, which would confuse the server.
    #[cfg(not(feature = "sound"))]
    {
        connector_config.enable_audio_playback = false;
    }
    #[cfg(feature = "sound")]
    if !config.channels.sound {
        connector_config.enable_audio_playback = false;
    }

    // Honor the runtime QOI/QOIZ codec toggles. Both codecs are compiled in and advertised by
    // default, but can be disabled at runtime; when disabled we drop them from the advertised
    // bitmap codec list so the server won't negotiate them.
    #[cfg(any(feature = "qoi", feature = "qoiz"))]
    if let Some(bitmap) = connector_config.bitmap.as_mut() {
        use ironrdp_pdu::rdp::capability_sets::CodecProperty;

        bitmap.codecs.0.retain(|codec| match codec.property {
            #[cfg(feature = "qoi")]
            CodecProperty::Qoi => config.channels.qoi,
            #[cfg(feature = "qoiz")]
            CodecProperty::QoiZ => config.channels.qoiz,
            _ => true,
        });
    }

    let mut connector = ironrdp_connector::ClientConnector::new(connector_config, client_addr)
        .with_static_channel(drdynvc);
    if let Some(redirect) = &config.redirection {
        connector = connector.with_redirected_session(
            redirect.session_id,
            redirect.routing.clone(),
            redirect.opaque_password().map(<[u8]>::to_vec),
        );
    }

    // Attach RDPSND (audio).
    #[cfg(feature = "sound")]
    if config.channels.sound {
        connector = connector.with_static_channel(ironrdp_rdpsnd::client::Rdpsnd::new(Box::new(
            cpal::RdpsndBackend::new(),
        )));
    }

    // Attach RDPDR (device redirection).
    #[cfg(feature = "rdpdr")]
    if config.channels.rdpdr.enabled {
        #[cfg_attr(
            not(feature = "smartcard"),
            expect(
                unused_mut,
                reason = "rdpdr_channel is only reassigned when the smartcard feature is enabled"
            )
        )]
        let mut rdpdr_channel = ironrdp_rdpdr::Rdpdr::new(
            Box::new(ironrdp_rdpdr::NoopRdpdrBackend),
            "IronRDP".to_owned(),
        );
        #[cfg(feature = "smartcard")]
        if config.channels.rdpdr.smartcard {
            rdpdr_channel = rdpdr_channel.with_smartcard(0);
        }
        connector = connector.with_static_channel(rdpdr_channel);
    }

    // Attach CLIPRDR (clipboard redirection). The backend is built fresh per connection.
    #[cfg(feature = "clipboard")]
    if let Some(factory) = cliprdr_factory {
        let backend = factory.build_cliprdr_backend();
        connector.attach_static_channel(ironrdp_cliprdr::Cliprdr::new(backend));
    }

    // Attach user-defined static channels from the extension registry.
    for attach_sc in &config.extensions.static_channels {
        attach_sc(&mut connector, &config.properties);
    }

    connector
}

// ── Transport-specific connect helpers ────────────────────────────────────────

trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite {}
type UpgradedFramed = ironrdp_tokio::TokioFramed<Box<dyn AsyncReadWrite + Unpin + Send + Sync>>;

/// Direct TCP → TLS connection (no gateway).
async fn connect_direct(
    config: &Config,
    input_sender: &InputSender,
    cliprdr_factory: CliprdrFactoryRef<'_>,
) -> ConnectorResult<(ConnectionResult, UpgradedFramed)> {
    let result = connect_direct_once(config, input_sender, cliprdr_factory).await;
    if let Err(error) = &result && config.legacy_security_fallback {
        use ironrdp_connector::ConnectorErrorKind;
        use ironrdp_pdu::nego::FailureCode;
        let standard = matches!(error.kind(), ConnectorErrorKind::StandardRdpRequired)
            || matches!(error.kind(), ConnectorErrorKind::Negotiation(failure) if failure.code() == FailureCode::SSL_NOT_ALLOWED_BY_SERVER);
        let tls = matches!(error.kind(), ConnectorErrorKind::Negotiation(failure) if failure.code() == FailureCode::SSL_REQUIRED_BY_SERVER);
        if standard || tls {
            let mut retry = config.clone();
            retry.connector.enable_tls = tls;
            retry.connector.enable_credssp = false;
            return connect_direct_once(&retry, input_sender, cliprdr_factory).await;
        }
    }
    result
}
async fn connect_direct_once(
    config: &Config,
    input_sender: &InputSender,
    cliprdr_factory: CliprdrFactoryRef<'_>,
) -> ConnectorResult<(ConnectionResult, UpgradedFramed)> {
    let stream = if let Some(connect) = &config.tcp_connector {
        connect(config.destination.clone()).await
    } else {
        TcpStream::connect(config.destination.to_string()).await
    }
        .map_err(|e| ironrdp_connector::custom_err!("TCP connect", e))?;
    // Interactive input consists of tiny packets. Nagle plus the server's
    // delayed acknowledgements can otherwise stall mouse motion for 200 ms.
    stream
        .set_nodelay(true)
        .map_err(|e| ironrdp_connector::custom_err!("TCP_NODELAY", e))?;
    let client_addr = stream
        .local_addr()
        .map_err(|e| ironrdp_connector::custom_err!("get socket local address", e))?;
    let framed = ironrdp_tokio::TokioFramed::new(stream);

    let connector = build_connector(config, client_addr, input_sender, cliprdr_factory);

    tls_handshake_and_finalize(framed, connector, config).await
}

/// RDS gateway TCP → gateway auth → TLS connection.
#[cfg(feature = "gateway")]
async fn connect_gateway(
    config: &Config,
    gw: &crate::config::GatewayConfig,
    input_sender: &InputSender,
    cliprdr_factory: CliprdrFactoryRef<'_>,
) -> ConnectorResult<(ConnectionResult, UpgradedFramed)> {
    use ironrdp_mstsgu::GwConnectTarget;

    // Build the GwConnectTarget.  `server` is the RDP target derived from `config.destination`.
    // TODO: preserve the destination port; ironrdp-mstsgu may currently hard-code 3389.
    let gw_target = GwConnectTarget {
        gw_endpoint: gw.endpoint.clone(),
        gw_user: gw.username.clone(),
        gw_pass: gw.password.clone(),
        server: config.destination.name().to_owned(),
    };

    let (gw_stream, client_addr) =
        ironrdp_mstsgu::GwClient::connect(&gw_target, &config.connector.client_name)
            .await
            .map_err(|e| ironrdp_connector::custom_err!("GW connect", e))?;

    let framed = ironrdp_tokio::TokioFramed::new(gw_stream);

    let connector = build_connector(config, client_addr, input_sender, cliprdr_factory);

    tls_handshake_and_finalize(framed, connector, config).await
}

/// RDCleanPath WebSocket → RDCleanPath handshake connection.
async fn connect_rdcleanpath_transport(
    config: &Config,
    rdcp: &RDCleanPathConfig,
    input_sender: &InputSender,
    cliprdr_factory: CliprdrFactoryRef<'_>,
) -> ConnectorResult<(ConnectionResult, UpgradedFramed)> {
    let hostname = rdcp
        .url
        .host_str()
        .ok_or_else(|| ironrdp_connector::general_err!("host missing from the URL"))?;
    let port = rdcp.url.port_or_known_default().unwrap_or(443);

    let socket = TcpStream::connect((hostname, port))
        .await
        .map_err(|e| ironrdp_connector::custom_err!("TCP connect", e))?;
    socket
        .set_nodelay(true)
        .map_err(|e| ironrdp_connector::custom_err!("set TCP_NODELAY", e))?;
    let client_addr = socket
        .local_addr()
        .map_err(|e| ironrdp_connector::custom_err!("get socket local address", e))?;

    let (ws, _) = tokio_tungstenite::client_async_tls(rdcp.url.as_str(), socket)
        .await
        .map_err(|e| ironrdp_connector::custom_err!("WS connect", e))?;
    let ws = crate::ws::websocket_compat(ws);
    let mut framed = ironrdp_tokio::TokioFramed::new(ws);

    let mut connector = build_connector(config, client_addr, input_sender, cliprdr_factory);

    let destination = config.destination.to_string();
    let (upgraded, server_public_key) = rdcleanpath_handshake(
        &mut framed,
        &mut connector,
        destination,
        rdcp.auth_token.clone(),
        None,
    )
    .await?;

    let connection_result = ironrdp_tokio::connect_finalize(
        upgraded,
        connector,
        &mut framed,
        &mut ReqwestNetworkClient::new(),
        (&config.destination).into(),
        server_public_key,
        config.kerberos_config.clone(),
    )
    .await?;

    let (ws, leftover_bytes) = framed.into_inner();
    let erased_stream: Box<dyn AsyncReadWrite + Unpin + Send + Sync> = Box::new(ws);
    let upgraded_framed =
        ironrdp_tokio::TokioFramed::new_with_leftover(erased_stream, leftover_bytes);

    Ok((connection_result, upgraded_framed))
}

// ── Shared TLS handshake ──────────────────────────────────────────────────────

async fn tls_handshake_and_finalize<S>(
    mut framed: ironrdp_tokio::TokioFramed<S>,
    mut connector: ironrdp_connector::ClientConnector,
    config: &Config,
) -> ConnectorResult<(ConnectionResult, UpgradedFramed)>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
{
    let should_upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector).await?;

    if matches!(&connector.state, ironrdp_connector::ClientConnectorState::EnhancedSecurityUpgrade { selected_protocol } if selected_protocol.is_standard_rdp_security()) {
        let security = connector.standard_security().ok_or_else(|| ironrdp_connector::general_err!("Standard RDP transport was not explicitly enabled"))?;
        let (stream, leftover) = framed.into_inner();
        let stream = ironrdp_connector::standard_security::Stream::new(stream, security);
        let stream:Box<dyn AsyncReadWrite + Unpin + Send + Sync> = Box::new(stream);
        let mut framed = ironrdp_tokio::TokioFramed::new_with_leftover(stream,leftover);
        let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade,&mut connector);
        let result = ironrdp_tokio::connect_finalize(upgraded,connector,&mut framed,&mut ReqwestNetworkClient::new(),(&config.destination).into(),Vec::new(),config.kerberos_config.clone()).await?;
        return Ok((result,framed));
    }

    debug!("TLS upgrade");

    let (initial_stream, leftover_bytes) = framed.into_inner();

    let (mut tls_stream, tls_cert) =
        ironrdp_tls::upgrade(initial_stream, config.destination.name())
            .await
            .map_err(|e| ironrdp_connector::custom_err!("TLS upgrade", e))?;

    if matches!(&connector.state, ironrdp_connector::ClientConnectorState::EnhancedSecurityUpgrade { selected_protocol } if selected_protocol.contains(ironrdp_pdu::nego::SecurityProtocol::RDSTLS))
    {
        let redirect = config
            .redirection
            .as_ref()
            .ok_or_else(|| ironrdp_connector::general_err!("RDSTLS missing server redirection"))?;
        redirection::authenticate(&mut tls_stream, redirect)
            .await
            .map_err(|e| {
                ironrdp_connector::custom_err!(
                    "RDSTLS authentication",
                    std::io::Error::other(e.to_string())
                )
            })?;
    }
    let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);

    let erased_stream: Box<dyn AsyncReadWrite + Unpin + Send + Sync> = Box::new(tls_stream);
    let mut upgraded_framed =
        ironrdp_tokio::TokioFramed::new_with_leftover(erased_stream, leftover_bytes);

    let server_public_key = ironrdp_tls::extract_tls_server_public_key(&tls_cert)
        .ok_or_else(|| ironrdp_connector::general_err!("unable to extract tls server public key"))?
        .to_owned();

    let connection_result = ironrdp_tokio::connect_finalize(
        upgraded,
        connector,
        &mut upgraded_framed,
        &mut ReqwestNetworkClient::new(),
        (&config.destination).into(),
        server_public_key,
        config.kerberos_config.clone(),
    )
    .await?;

    Ok((connection_result, upgraded_framed))
}

// ── RDCleanPath handshake ─────────────────────────────────────────────────────

async fn rdcleanpath_handshake<S>(
    framed: &mut ironrdp_tokio::Framed<S>,
    connector: &mut ironrdp_connector::ClientConnector,
    destination: String,
    proxy_auth_token: String,
    pcb: Option<String>,
) -> ConnectorResult<(ironrdp_tokio::Upgraded, Vec<u8>)>
where
    S: ironrdp_tokio::FramedRead + FramedWrite,
{
    use ironrdp_connector::Sequence as _;
    use x509_cert::der::Decode as _;

    #[derive(Clone, Copy, Debug)]
    struct RDCleanPathHint;
    const RDCLEANPATH_HINT: RDCleanPathHint = RDCleanPathHint;

    impl ironrdp_pdu::PduHint for RDCleanPathHint {
        fn find_size(&self, bytes: &[u8]) -> ironrdp_core::DecodeResult<Option<(bool, usize)>> {
            match ironrdp_rdcleanpath::RDCleanPathPdu::detect(bytes) {
                ironrdp_rdcleanpath::DetectionResult::Detected { total_length, .. } => {
                    Ok(Some((true, total_length)))
                }
                ironrdp_rdcleanpath::DetectionResult::NotEnoughBytes => Ok(None),
                ironrdp_rdcleanpath::DetectionResult::Failed => Err(ironrdp_core::other_err!(
                    "RDCleanPathHint",
                    "detection failed"
                )),
            }
        }
    }

    let mut buf = WriteBuf::new();
    info!("Begin RDCleanPath connection procedure");

    // Send X224 + RDCleanPath request.
    {
        let ironrdp_connector::ClientConnectorState::ConnectionInitiationSendRequest =
            connector.state
        else {
            return Err(ironrdp_connector::general_err!(
                "invalid connector state (send request)"
            ));
        };
        debug_assert!(connector.next_pdu_hint().is_none());
        let written = connector.step_no_input(&mut buf)?;
        let x224_pdu_len = written.size().expect("written size");
        debug_assert_eq!(x224_pdu_len, buf.filled_len());
        let x224_pdu = buf.filled().to_vec();

        let rdcleanpath_req = ironrdp_rdcleanpath::RDCleanPathPdu::new_request(
            x224_pdu,
            destination,
            proxy_auth_token,
            pcb,
        )
        .map_err(|e| ironrdp_connector::custom_err!("new RDCleanPath request", e))?;
        debug!(message = ?rdcleanpath_req, "Send RDCleanPath request");
        let rdcleanpath_req = rdcleanpath_req
            .to_der()
            .map_err(|e| ironrdp_connector::custom_err!("RDCleanPath request encode", e))?;
        framed
            .write_all(&rdcleanpath_req)
            .await
            .map_err(|e| ironrdp_connector::custom_err!("couldn't write RDCleanPath request", e))?;
    }

    // Read RDCleanPath response.
    {
        let rdcleanpath_res = framed
            .read_by_hint(&RDCLEANPATH_HINT)
            .await
            .map_err(|e| ironrdp_connector::custom_err!("read RDCleanPath response", e))?;
        let rdcleanpath_res = ironrdp_rdcleanpath::RDCleanPathPdu::from_der(&rdcleanpath_res)
            .map_err(|e| ironrdp_connector::custom_err!("RDCleanPath response decode", e))?;
        debug!(message = ?rdcleanpath_res, "Received RDCleanPath PDU");

        let (x224_connection_response, server_cert_chain) = match rdcleanpath_res
            .into_enum()
            .map_err(|e| ironrdp_connector::custom_err!("invalid RDCleanPath PDU", e))?
        {
            ironrdp_rdcleanpath::RDCleanPath::Request { .. } => {
                return Err(ironrdp_connector::general_err!(
                    "received unexpected RDCleanPath type (request)"
                ));
            }
            ironrdp_rdcleanpath::RDCleanPath::Response {
                x224_connection_response,
                server_cert_chain,
                server_addr: _,
            } => (x224_connection_response, server_cert_chain),
            ironrdp_rdcleanpath::RDCleanPath::GeneralErr(error) => {
                return Err(ironrdp_connector::custom_err!(
                    "received RDCleanPath error",
                    error
                ));
            }
            ironrdp_rdcleanpath::RDCleanPath::NegotiationErr {
                x224_connection_response,
            } => {
                if let Ok(x224_confirm) = ironrdp_core::decode::<
                    ironrdp_pdu::x224::X224<ironrdp_pdu::nego::ConnectionConfirm>,
                >(&x224_connection_response)
                {
                    if let ironrdp_pdu::nego::ConnectionConfirm::Failure { code } = x224_confirm.0 {
                        let negotiation_failure = ironrdp_connector::NegotiationFailure::from(code);
                        return Err(ironrdp_connector::ConnectorError::new(
                            "RDP negotiation failed",
                            ironrdp_connector::ConnectorErrorKind::Negotiation(negotiation_failure),
                        ));
                    }
                }
                return Err(ironrdp_connector::general_err!(
                    "received RDCleanPath negotiation error"
                ));
            }
        };

        let ironrdp_connector::ClientConnectorState::ConnectionInitiationWaitConfirm { .. } =
            connector.state
        else {
            return Err(ironrdp_connector::general_err!(
                "invalid connector state (wait confirm)"
            ));
        };
        debug_assert!(connector.next_pdu_hint().is_some());

        buf.clear();
        let written = connector.step(x224_connection_response.as_bytes(), &mut buf)?;
        debug_assert!(written.is_nothing());

        let server_cert = server_cert_chain.into_iter().next().ok_or_else(|| {
            ironrdp_connector::general_err!("server cert chain missing from rdcleanpath response")
        })?;

        let cert = x509_cert::Certificate::from_der(server_cert.as_bytes())
            .map_err(|e| ironrdp_connector::custom_err!("server cert decode", e))?;

        let server_public_key = cert
            .tbs_certificate
            .subject_public_key_info
            .subject_public_key
            .as_bytes()
            .ok_or_else(|| {
                ironrdp_connector::general_err!("subject public key BIT STRING is not aligned")
            })?
            .to_owned();

        let should_upgrade = ironrdp_tokio::skip_connect_begin(connector);
        let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, connector);

        Ok((upgraded, server_public_key))
    }
}

// ── Active session ────────────────────────────────────────────────────────────

enum RdpControlFlow {
    Redirect(Box<redirection::Redirection>),
    ReconnectWithNewSize { width: u16, height: u16 },
    TerminatedGracefully(GracefulDisconnectReason),
}

// Only charge synchronous decoding. A pending transport read yields to the
// owner's event loop, so it starts a fresh cooperative budget on resumption.
fn decode_step<T>(work: &AtomicU64, step: impl FnOnce() -> T) -> T {
    let started = std::time::Instant::now();
    let output = step();
    let nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    work.store(
        work.load(Ordering::Relaxed).saturating_add(nanos),
        Ordering::Relaxed,
    );
    output
}
async fn wait_for_pdu<T>(work: &AtomicU64, future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    std::future::poll_fn(|context| {
        let result = Future::poll(future.as_mut(), context);
        if result.is_pending() {
            work.store(0, Ordering::Relaxed);
        }
        result
    })
    .await
}

async fn active_session(
    framed: UpgradedFramed,
    connection_result: ConnectionResult,
    output_event_sender: &mpsc::Sender<RdpOutputEvent>,
    input_event_receiver: &mut mpsc::UnboundedReceiver<input::Control>,
    fake_events_interval: Option<Duration>,
    fast_input: Option<input::Lane>,
) -> SessionResult<RdpControlFlow> {
    let (mut reader, writer) = split_tokio_framed(framed);
    let mut writer = input::Writer::start(writer, fast_input.clone());
    let desktop_size = connection_result.desktop_size;
    let io_channel_id = connection_result.io_channel_id;
    if usize::from(desktop_size.width) * usize::from(desktop_size.height) > 16_777_216 {
        return Err(ironrdp_session::general_err!(
            "RDP desktop exceeds 16 million pixels"
        ));
    }
    let mut image = DecodedImage::new(PixelFormat::RgbA32, desktop_size.width, desktop_size.height);

    // We retain the factory to drive the Deactivation-Reactivation Sequence locally.
    let activation_factory = connection_result.activation_factory;

    let mut active_stage = ActiveStageBuilder {
        static_channels: connection_result.static_channels,
        user_channel_id: connection_result.user_channel_id,
        io_channel_id: connection_result.io_channel_id,
        message_channel_id: connection_result.message_channel_id,
        share_id: connection_result.share_id,
        compression_type: connection_result.compression_type,
        enable_server_pointer: connection_result.enable_server_pointer,
        pointer_software_rendering: connection_result.pointer_software_rendering,
    }
    .build();

    // Timer interval for driving clipboard lock timeouts.
    let mut cleanup_interval = tokio::time::interval(Duration::from_secs(5));

    // Anti-idle: track the time of the last real input and the last known mouse position so we can
    // synthesize a no-op mouse move when the session has been idle for too long. Default to the
    // middle of the screen so a synthetic move before any real input doesn't snap the pointer to a
    // corner.
    let mut last_input = tokio::time::Instant::now();
    let mut last_mouse_pos = (desktop_size.width / 2, desktop_size.height / 2);
    let mut fake_events_interval = fake_events_interval
        .map(|interval| tokio::time::interval(core::cmp::max(interval, Duration::from_secs(1))));

    let mut graphics = graphics::Updates::default();
    // A short protocol batching window bounds pixel copies. The retained canvas
    // independently coalesces patches until the renderer's next actual paint.
    let mut present = tokio::time::interval(Duration::from_millis(4));
    present.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let decode_work = AtomicU64::new(0);
    let mut pipeline = diagnostics::Pipeline::enabled();
    let disconnect_reason = 'outer: loop {
        if let Some(pipeline) = &mut pipeline {
            pipeline.report();
        }
        // Bound each decode slice so input, cancellation and outgoing protocol
        // traffic remain responsive even while the socket is continuously ready.
        if decode_work.load(Ordering::Relaxed) >= 4_000_000 {
            decode_work.store(0, Ordering::Relaxed);
            tokio::task::yield_now().await;
        }
        let mut control_sequence = None;
        let outputs = tokio::select! {
            biased;
            input_event = input_event_receiver.recv() => {
                let input_event = input_event.ok_or_else(|| ironrdp_session::general_err!("GUI is stopped"))?;

                control_sequence = Some(input_event.sequence);
                let input_event = input_event.event;
                last_input = tokio::time::Instant::now();

                match input_event {
                    RdpInputEvent::Resize { width, height, scale_factor, physical_size } => {
                        trace!(width, height, "Resize event");
                        let width = u32::from(width);
                        let height = u32::from(height);
                        // TODO: Make adjust_display_size take and return width and height as u16.
                        // From the function's doc comment, the width and height values must be less than or equal to 8192 pixels.
                        // Therefore, we can remove unnecessary casts from u16 to u32 and back.
                        let (width, height) = MonitorLayoutEntry::adjust_display_size(width, height);
                        debug!(width, height, "Adjusted display size");
                        if let Some(response_frame) = active_stage.encode_resize(width, height, Some(scale_factor), physical_size) {
                            vec![ActiveStageOutput::ResponseFrame(response_frame?)]
                        } else {
                            // TODO(#271): use the "auto-reconnect cookie": https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/15b0d1c9-2891-4adb-a45e-deb4aeeeab7c
                            debug!("Reconnecting with new size");
                            let width = u16::try_from(width).expect("always in the range");
                            let height = u16::try_from(height).expect("always in the range");
                            // This command retires with the old transport; later
                            // controls remain fenced until the replacement session handles them.
                            if let Some(lane) = &fast_input {
                                lane.pause(true);
                                lane.complete_control(control_sequence.unwrap_or(0));
                            }
                            return Ok(RdpControlFlow::ReconnectWithNewSize { width, height })
                        }
                    }
                    RdpInputEvent::FastPath(events) => {
                        trace!(?events);
                        for event in &events {
                            if let FastPathInputEvent::MouseEvent(mouse) = event {
                                last_mouse_pos = (mouse.x_position, mouse.y_position);
                            }
                        }
                        decode_step(&decode_work, || active_stage.process_fastpath_input(&mut image, &events))?
                    }
                    RdpInputEvent::Close => {
                        active_stage.graceful_shutdown()?
                    }
                    #[cfg(feature = "clipboard")]
                    RdpInputEvent::Clipboard(event) => {
                        if let Some(cliprdr_client) = active_stage.get_svc_processor_mut::<ironrdp_cliprdr::CliprdrClient>() {
                            if let Some(svc_messages) = match event {
                                ClipboardMessage::SendInitiateCopy(formats) => {
                                    Some(cliprdr_client.initiate_copy(&formats)
                                        .map_err(|e| ironrdp_session::custom_err!("CLIPRDR", e))?)
                                }
                                ClipboardMessage::SendInitiateFileCopy(files) => {
                                    match cliprdr_client.initiate_file_copy(files) {
                                        Ok(messages) => Some(messages),
                                        Err(error) => { warn!(%error, "Clipboard file operation rejected"); None }
                                    }
                                }
                                ClipboardMessage::SendFormatData(response) => {
                                    Some(cliprdr_client.submit_format_data(response)
                                        .map_err(|e| ironrdp_session::custom_err!("CLIPRDR", e))?)
                                }
                                ClipboardMessage::SendInitiatePaste(format) => {
                                    Some(cliprdr_client.initiate_paste(format)
                                        .map_err(|e| ironrdp_session::custom_err!("CLIPRDR", e))?)
                                }
                                ClipboardMessage::SendFileContentsRequest(request) => {
                                    match cliprdr_client.request_file_contents(request) {
                                        Ok(messages) => Some(messages),
                                        Err(error) => { warn!(%error, "Clipboard file operation rejected"); None }
                                    }
                                }
                                ClipboardMessage::SendFileContentsResponse(response) => {
                                    match cliprdr_client.submit_file_contents(response) {
                                        Ok(messages) => Some(messages),
                                        Err(error) => { warn!(%error, "Clipboard file operation rejected"); None }
                                    }
                                }
                                ClipboardMessage::Error(e) => {
                                    error!("Clipboard backend error: {}", e);
                                    None
                                }
                            } {
                                let frame = active_stage.process_svc_processor_messages(svc_messages)?;
                                vec![ActiveStageOutput::ResponseFrame(frame)]
                            } else {
                                Vec::new()
                            }
                        } else {
                            warn!("Clipboard event received, but Cliprdr is not available");
                            Vec::new()
                        }
                    }
                    RdpInputEvent::SendDvcMessages { channel_id, messages } => {
                        trace!(channel_id, ?messages, "Send DVC messages");
                        let frame = active_stage.encode_dvc_messages(messages)?;
                        vec![ActiveStageOutput::ResponseFrame(frame)]
                    }
                }
            }
            _ = present.tick(), if graphics.pending() => {
                let copied_at = pipeline.as_ref().map(|_| std::time::Instant::now());
                let picture = decode_step(&decode_work, || graphics.take(&image))?;
                let copied = copied_at.map(|at| at.elapsed());
                let wait_at = pipeline.as_ref().map(|_| std::time::Instant::now());
                output_event_sender.send(picture).await
                    .map_err(|e| ironrdp_session::custom_err!("output_event_sender", e))?;
                if let (Some(pipeline), Some(copied), Some(wait_at)) = (&mut pipeline, copied, wait_at) {
                    pipeline.presented(copied, wait_at.elapsed());
                }
                continue 'outer;
            }

            _ = cleanup_interval.tick() => {
                // Drive clipboard lock timeout cleanup.
                #[cfg(feature = "clipboard")]
                if let Some(cliprdr_client) = active_stage.get_svc_processor_mut::<ironrdp_cliprdr::CliprdrClient>() {
                    match cliprdr_client.drive_timeouts() {
                        Ok(svc_messages) => {
                            let frame = active_stage.process_svc_processor_messages(svc_messages)?;
                            if !frame.is_empty() {
                                vec![ActiveStageOutput::ResponseFrame(frame)]
                            } else {
                                Vec::new()
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "Clipboard timeout cleanup failed");
                            Vec::new()
                        }
                    }
                } else {
                    Vec::new()
                }
                #[cfg(not(feature = "clipboard"))]
                Vec::new()
            }
            _ = async { match fake_events_interval.as_mut() {
                Some(interval) => interval.tick().await,
                None => core::future::pending().await,
            }} => {
                if let Some((activity, position)) = fast_input.as_ref().and_then(input::Lane::activity) {
                    last_input = last_input.max(activity);
                    if let Some(position) = position { last_mouse_pos = position; }
                }
                // Anti-idle: synthesize a no-op mouse move if the session has been idle for at least
                // the configured interval, keeping the connection alive without user interaction.
                if last_input.elapsed() >= fake_events_interval.as_ref().map_or(Duration::MAX, |i| i.period()) {
                    last_input = tokio::time::Instant::now();
                    let mut events = SmallVec::<[FastPathInputEvent; 2]>::new();
                    events.push(FastPathInputEvent::MouseEvent(MousePdu {
                        flags: PointerFlags::MOVE,
                        number_of_wheel_rotation_units: 0,
                        x_position: last_mouse_pos.0,
                        y_position: last_mouse_pos.1,
                    }));
                    decode_step(&decode_work, || active_stage.process_fastpath_input(&mut image, &events))?
                } else {
                    Vec::new()
                }
            }
            frame = wait_for_pdu(&decode_work, reader.read_pdu()) => {
                let (action, payload) = frame.map_err(|e| ironrdp_session::custom_err!("read frame", e))?;
                trace!(?action, frame_length = payload.len(), "Frame received");
                if action == ironrdp_pdu::Action::X224 {
                    if let Some(redirect) = redirection::detect(&payload, io_channel_id)
                        .map_err(|e| ironrdp_session::custom_err!("Server redirection", std::io::Error::other(e.to_string())))? {
                        if redirect.informational() { continue 'outer; }
                        return Ok(RdpControlFlow::Redirect(Box::new(redirect)));
                    }
                }
                let at = pipeline.as_ref().map(|_| std::time::Instant::now());
                let decoded = decode_step(&decode_work, || active_stage.process(&mut image, action, &payload, reader.last_read_at()))?;
                if let (Some(pipeline), Some(at)) = (&mut pipeline, at) { pipeline.decoded(payload.len(), at.elapsed()); }
                decoded
            }
        };

        // Preserve individual EGFX/legacy damage rectangles. A single union of
        // distant small updates needlessly uploads most of a desktop every frame.
        let damage = active_stage.take_damage_regions();
        let precise_damage = !damage.is_empty();
        for region in damage {
            graphics.mark(region, image.width(), image.height());
        }
        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(frame) => writer
                    .write_owned(frame)
                    .await
                    .map_err(|e| ironrdp_session::custom_err!("write response", e))?,
                ActiveStageOutput::GraphicsUpdate(region) => {
                    if !precise_damage {
                        graphics.mark(region, image.width(), image.height());
                    }
                }
                ActiveStageOutput::PointerDefault => {
                    output_event_sender
                        .send(RdpOutputEvent::PointerDefault)
                        .await
                        .map_err(|e| ironrdp_session::custom_err!("output_event_sender", e))?;
                }
                ActiveStageOutput::PointerHidden => {
                    output_event_sender
                        .send(RdpOutputEvent::PointerHidden)
                        .await
                        .map_err(|e| ironrdp_session::custom_err!("output_event_sender", e))?;
                }
                ActiveStageOutput::PointerPosition { x, y } => {
                    output_event_sender
                        .send(RdpOutputEvent::PointerPosition { x, y })
                        .await
                        .map_err(|e| ironrdp_session::custom_err!("output_event_sender", e))?;
                }
                ActiveStageOutput::PointerBitmap(pointer) => {
                    output_event_sender
                        .send(RdpOutputEvent::PointerBitmap(pointer))
                        .await
                        .map_err(|e| ironrdp_session::custom_err!("output_event_sender", e))?;
                }
                ActiveStageOutput::DeactivateAll => {
                    if let Some(lane) = &fast_input {
                        lane.pause(true);
                    }
                    // Deactivation-Reactivation Sequence:
                    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/dfc234ce-481a-4674-9a5d-2a7bafb14432
                    debug!("Executing Deactivation-Reactivation Sequence");
                    let mut connection_activation = activation_factory.create();
                    let mut buf = WriteBuf::new();
                    'activation_seq: loop {
                        let written = single_sequence_step_read(
                            &mut reader,
                            &mut connection_activation,
                            &mut buf,
                        )
                        .await
                        .map_err(|e| {
                            ironrdp_session::custom_err!(
                                "read deactivation-reactivation sequence step",
                                e
                            )
                        })?;
                        if written.size().is_some() {
                            writer.write_all(buf.filled()).await.map_err(|e| {
                                ironrdp_session::custom_err!(
                                    "write deactivation-reactivation sequence step",
                                    e
                                )
                            })?;
                        }
                        if let ConnectionActivationState::Finalized {
                            desktop_size,
                            share_id,
                            enable_server_pointer,
                            pointer_software_rendering,
                        } = connection_activation.connection_activation_state()
                        {
                            debug!(
                                ?desktop_size,
                                "Deactivation-Reactivation Sequence completed"
                            );
                            if usize::from(desktop_size.width) * usize::from(desktop_size.height)
                                > 16_777_216
                            {
                                return Err(ironrdp_session::general_err!(
                                    "RDP desktop exceeds 16 million pixels"
                                ));
                            }
                            image = DecodedImage::new(
                                PixelFormat::RgbA32,
                                desktop_size.width,
                                desktop_size.height,
                            );
                            active_stage.set_fastpath_processor(
                                fast_path::ProcessorBuilder {
                                    io_channel_id: connection_activation.io_channel_id(),
                                    user_channel_id: connection_activation.user_channel_id(),
                                    share_id,
                                    enable_server_pointer,
                                    pointer_software_rendering,
                                }
                                .build(),
                            );
                            active_stage.set_share_id(share_id);
                            active_stage.set_enable_server_pointer(enable_server_pointer);
                            if let Some(lane) = &fast_input {
                                lane.pause(false);
                            }
                            break 'activation_seq;
                        }
                    }
                }
                ActiveStageOutput::MultitransportRequest(pdu) => {
                    debug!(
                        request_id = pdu.request_id,
                        requested_protocol = ?pdu.requested_protocol,
                        "Multitransport request received (UDP transport not implemented)"
                    );
                }
                ActiveStageOutput::AutoDetect(request) => {
                    debug!(?request, "Auto-detect");
                }
                ActiveStageOutput::MonitorLayout(_) => trace!("Server monitor layout received"),
                ActiveStageOutput::WindowingOrders(_) => trace!("Server windowing orders received"),
                ActiveStageOutput::SaveSessionInfo { logon_complete } => {
                    debug!(logon_complete, "Server logon notification")
                }
                ActiveStageOutput::AutoReconnectCookie(_) => {
                    trace!("Server reconnect cookie received")
                }
                ActiveStageOutput::AutoReconnectFailed => {
                    debug!("Server automatic reconnection rejected")
                }
                ActiveStageOutput::Terminate(reason) => break 'outer reason,
            }
        }
        // Completing the fence requires every response write above to have
        // reached the sole TLS writer, not merely removal from the control FIFO.
        if let Some(sequence) = control_sequence
            && let Some(lane) = &fast_input
        {
            lane.complete_control(sequence);
        }
        // EGFX has already committed a complete compositor update. Waiting for
        // the bitmap timer here adds another scheduler tick (often ~16 ms on
        // Windows) and can halve visible cadence when combined with UI vsync.
        // Keep timed batching for legacy fragmented bitmap/RemoteFX updates.
        if active_stage.take_graphics_frame_ready() && graphics.pending() {
            let copied_at = pipeline.as_ref().map(|_| std::time::Instant::now());
            let picture = decode_step(&decode_work, || graphics.take(&image))?;
            let copied = copied_at.map(|at| at.elapsed());
            let wait_at = pipeline.as_ref().map(|_| std::time::Instant::now());
            output_event_sender
                .send(picture)
                .await
                .map_err(|e| ironrdp_session::custom_err!("output_event_sender", e))?;
            if let (Some(pipeline), Some(copied), Some(wait_at)) = (&mut pipeline, copied, wait_at)
            {
                pipeline.presented(copied, wait_at.elapsed());
            }
        }
    };

    if graphics.pending() {
        output_event_sender
            .send(graphics.take(&image)?)
            .await
            .map_err(|e| ironrdp_session::custom_err!("output_event_sender", e))?;
    }
    Ok(RdpControlFlow::TerminatedGracefully(disconnect_reason))
}
