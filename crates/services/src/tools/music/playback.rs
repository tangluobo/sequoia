use crate::backup::Control;
use anyhow::{Context, Result};
use rodio::{Decoder, DeviceSinkBuilder, Player};
use std::{
    fs::File,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tempfile::TempDir;

#[derive(Debug)]
pub struct Playback {
    control: Control,
    active: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Playback {
    pub(super) async fn start(
        path: PathBuf,
        temporary: Option<TempDir>,
        control: Control,
    ) -> Result<Self> {
        let (ready, started) = tokio::sync::oneshot::channel();
        let active = Arc::new(AtomicBool::new(true));
        let active_in_thread = active.clone();
        let control_in_thread = control.clone();
        let thread = thread::Builder::new()
            .name("tomato-music".into())
            .spawn(move || {
                let _temporary = temporary;
                let opened = (|| -> Result<_> {
                    control_in_thread.check()?;
                    let audio =
                        Decoder::try_from(File::open(path)?).context("无法解码 FLAC 录音")?;
                    let device =
                        DeviceSinkBuilder::open_default_sink().context("无法打开默认音频设备")?;
                    let player = Player::connect_new(device.mixer());
                    player.append(audio);
                    Ok((device, player))
                })();
                match opened {
                    Ok((_device, player)) => {
                        if ready.send(Ok(())).is_ok() {
                            while !player.empty() && !control_in_thread.cancelled() {
                                thread::park_timeout(Duration::from_millis(150));
                            }
                        }
                        player.stop();
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
                active_in_thread.store(false, Ordering::Release);
                control_in_thread.object();
            })?;
        let playback = Self {
            control,
            active,
            thread,
        };
        started.await.context("音频播放初始化失败")??;
        Ok(playback)
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.control.cancel();
        self.thread.thread().unpark();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::{Command, CommandHandler, Engine, Output},
        tools::music::{Request, Track},
    };

    #[tokio::test]
    #[ignore = "requires an audio output device and TOMATO_MUSIC_PLAYBACK_FIXTURE pointing to an owned local FLAC"]
    async fn local_flac_plays_through_command_handler_and_releases_after_drop() -> Result<()> {
        let path = PathBuf::from(
            std::env::var_os("TOMATO_MUSIC_PLAYBACK_FIXTURE")
                .context("Missing owned playback fixture")?,
        );
        let control = Control::default();
        let output = Engine::default()
            .execute(Command::Music {
                request: Request::Preview {
                    track: Track {
                        item: String::new(),
                        file: String::new(),
                        title: "Playback fixture".into(),
                        artist: String::new(),
                        recording: String::new(),
                        bytes: 0,
                        sha1: None,
                    },
                    local_file: Some(path),
                },
                control: Some(control.clone()),
            })
            .await?;
        let Output::Music(crate::tools::music::Output::Preview { playback }) = output else {
            anyhow::bail!("Expected initialized playback");
        };
        let active = playback.active.clone();
        assert!(active.load(Ordering::Acquire));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(playback.is_active());
        drop(playback);
        assert!(control.cancelled());
        tokio::time::timeout(Duration::from_secs(3), async {
            while active.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .context("Playback resources remained active after drop")?;
        Ok(())
    }
}
