//! Version-pinned scrcpy over device-local abstract sockets.
mod input;
mod video;
use super::*;
use crate::remote_desktop::YuvFrame;
use tokio::io::AsyncWrite;
const SERVER: &[u8] = include_bytes!("../../../assets/adb/scrcpy-server-v3.3.4.jar");
const VERSION: &str = "3.3.4";
static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static FINISHED: tokio::sync::Notify = tokio::sync::Notify::const_new();
struct Complete;
impl Drop for Complete {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        FINISHED.notify_waiters();
    }
}
pub(in crate::remote_desktop) async fn wait_cleanup() {
    loop {
        let notified = FINISHED.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if ACTIVE.load(std::sync::atomic::Ordering::Acquire) == 0 {
            return;
        }
        notified.await;
    }
}
struct Cleanup {
    transport: native::Transport,
    remote: String,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let complete = Complete;
        let (transport, remote) = (self.transport.clone(), self.remote.clone());
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _complete = complete;
                let _ = timeout(Duration::from_secs(5), async {
                    if let Ok(mut stream) = transport.open(&format!("exec:rm -f {remote}")).await {
                        let _ = tokio::io::copy(&mut stream, &mut tokio::io::sink()).await;
                    }
                })
                .await;
            });
        }
    }
}
pub(super) struct Session {
    video: DuplexStream,
    control: DuplexStream,
    _server: vnc::Worker,
    _cleanup: Cleanup,
}
impl Session {
    pub(super) async fn open(c: &Connection, transport: &native::Transport) -> Result<Self> {
        let digest = ring::digest::digest(&ring::digest::SHA256, SERVER);
        let actual = digest
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        ensure!(
            actual == "8588238c9a5a00aa542906b6ec7e6d5541d9ffb9b5d0f6e1bc0e365e2303079e",
            "内置 scrcpy 服务校验失败"
        );
        let id = uuid::Uuid::new_v4();
        let scid = u32::from_be_bytes(id.as_bytes()[..4].try_into()?) & 0x7fff_ffff;
        let remote = format!("/data/local/tmp/tomato-{}.jar", id.simple());
        let socket = format!("localabstract:scrcpy_{scid:08x}");
        ACTIVE.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let cleanup = Cleanup {
            transport: transport.clone(),
            remote: remote.clone(),
        };
        let (max_size, bitrate) = super::quality::VideoQuality::encoder_settings(c);
        let arguments = [
            format!("CLASSPATH={remote}"),
            "app_process".into(),
            "/".into(),
            "com.genymobile.scrcpy.Server".into(),
            VERSION.into(),
            format!("scid={scid:x}"),
            "tunnel_forward=true".into(),
            "video=true".into(),
            "video_codec=h264".into(),
            format!("max_size={max_size}"),
            format!("video_bit_rate={bitrate}"),
            "max_fps=60".into(),
            "video_codec_options=i-frame-interval=1".into(),
            "send_codec_meta=true".into(),
            "send_frame_meta=true".into(),
            "audio=false".into(),
            "control=true".into(),
            "send_device_meta=false".into(),
            "send_dummy_byte=true".into(),
            "clipboard_autosync=false".into(),
            "power_on=false".into(),
            "cleanup=false".into(),
            "log_level=warn".into(),
        ];
        let (mut video, control, server) =
            open_channels(transport, &remote, &socket, &arguments).await?;
        timeout(Duration::from_secs(5), async {
            let codec = video.read_u32().await?;
            ensure!(
                codec == u32::from_be_bytes(*b"h264"),
                "设备没有返回 H.264 视频"
            );
            video::dimensions(video.read_u32().await?, video.read_u32().await?)?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("scrcpy 初始化视频超时")??;
        Ok(Self {
            video,
            control,
            _server: server,
            _cleanup: cleanup,
        })
    }
    pub(super) async fn run(
        self,
        inputs: &mut super::pool::Inputs,
        shared: &Arc<Mutex<Shared>>,
        notifier: &Notifier,
    ) -> Result<()> {
        let Self {
            video,
            control,
            _server,
            _cleanup,
        } = self;
        let queue = Arc::new(video::Queue::default());
        let _stop = video::Stop(queue.clone());
        let (decoder_queue, state, repaint) = (queue.clone(), shared.clone(), notifier.clone());
        let mut decode =
            tokio::task::spawn_blocking(move || video::decode(decoder_queue, state, repaint));
        let (mut control_read, control_write) = tokio::io::split(control);
        let mut sink = tokio::io::sink();
        let result = tokio::select! {
            result = video::receive(video, queue.clone()) => result,
            result = input::run(control_write, inputs, shared, notifier) => result,
            result = &mut decode => return result.context("ADB 视频解码线程退出")?,
            result = tokio::io::copy(&mut control_read, &mut sink) => {
                result?; Err(anyhow::anyhow!("scrcpy 控制通道已断开，请重新连接"))
            }
        };
        queue.close();
        decode.await.context("ADB 视频解码线程退出")??;
        result
    }
}
async fn open_channels(
    transport: &native::Transport,
    remote: &str,
    socket: &str,
    arguments: &[String],
) -> Result<(DuplexStream, DuplexStream, vnc::Worker)> {
    transport.push(remote, SERVER).await?;
    let mut output = transport
        .open(&format!("exec:{}", arguments.join(" ")))
        .await?;
    let output = vnc::Worker(tokio::spawn(async move {
        let _ = tokio::io::copy(&mut output, &mut tokio::io::sink()).await;
    }));
    let (video, control) = timeout(Duration::from_secs(8), async {
        let video = loop {
            if output.0.is_finished() {
                anyhow::bail!("scrcpy 服务提前退出");
            }
            match transport.open(socket).await {
                Ok(mut video) => {
                    ensure!(video.read_u8().await? == 0, "scrcpy 握手无效");
                    break video;
                }
                Err(error)
                    if matches!(
                        error.downcast_ref(),
                        Some(native::ChannelError::ChannelClosed)
                    ) =>
                {
                    tokio::time::sleep(Duration::from_millis(40)).await
                }
                Err(error) => return Err(error),
            }
        };
        let control = transport.open(socket).await?;
        Ok::<_, anyhow::Error>((video, control))
    })
    .await
    .context("scrcpy 启动超时")??;
    Ok((video, control, output))
}
