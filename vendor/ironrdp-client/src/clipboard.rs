#[cfg(windows)]
pub(crate) mod files;

use ironrdp_cliprdr::backend::{ClipboardMessage, ClipboardMessageProxy};
use tracing::error;

use crate::rdp::{InputSender, RdpInputEvent};

/// Shim that forwards CLIPRDR events into the `RdpInputEvent` channel.
#[derive(Clone, Debug)]
pub(crate) struct ClientClipboardMessageProxy {
    tx: InputSender,
    #[cfg(windows)]
    files: Option<std::sync::Arc<files::Bridge>>,
}

impl ClientClipboardMessageProxy {
    #[cfg(windows)]
    pub(crate) fn with_files(tx: InputSender, files: std::sync::Arc<files::Bridge>) -> Self {
        Self { tx, files: Some(files) }
    }
}

impl ClipboardMessageProxy for ClientClipboardMessageProxy {
    fn send_clipboard_message(&self, message: ClipboardMessage) {
        #[cfg(windows)]
        let message = if let ClipboardMessage::SendInitiateCopy(formats) = message {
            match &self.files {
                Some(files) => match files.local_changed(formats) {
                    Some(message) => message,
                    None => return,
                },
                None => ClipboardMessage::SendInitiateCopy(formats),
            }
        } else {
            message
        };
        if self.tx.send(RdpInputEvent::Clipboard(message)).is_err() {
            error!("Failed to send clipboard message; receiver is closed");
        }
    }
}
