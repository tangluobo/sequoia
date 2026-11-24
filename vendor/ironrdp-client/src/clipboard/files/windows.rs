use super::*;
use ironrdp_cliprdr::pdu::ClipboardFormatId;
use std::sync::OnceLock;
use virtual_file_transfer::{ClipboardSession, FactorySource, SourceReader, VirtualFile};

fn current() -> &'static Mutex<Weak<Offer>> {
    static CURRENT: OnceLock<Mutex<Weak<Offer>>> = OnceLock::new();
    CURRENT.get_or_init(|| Mutex::new(Weak::new()))
}
pub(super) fn sequence() -> u32 {
    clipboard_win::seq_num().map_or(0, |n| n.get())
}

pub(super) fn local_changed(
    bridge: &Arc<Bridge>,
    formats: Vec<ClipboardFormat>,
) -> Option<ClipboardMessage> {
    let sequence = sequence();
    let offer = current()
        .lock()
        .unwrap()
        .upgrade()
        .filter(|offer| offer.sequence.load(Ordering::Acquire) == sequence);
    if let Some(offer) = offer {
        if offer.origin == bridge.id {
            return None;
        }
        if bridge.file_enabled.load(Ordering::Acquire) {
            if bridge.last_local_sequence.swap(sequence, Ordering::AcqRel) == sequence {
                return None;
            }
            let _ = bridge.offer_local(offer.files.clone());
            return None;
        }
    }
    bridge.publish(Publication::Clear);
    if bridge.file_enabled.load(Ordering::Acquire)
        && formats
            .iter()
            .any(|f| f.id() == ClipboardFormatId::CF_HDROP)
    {
        if bridge.last_local_sequence.swap(sequence, Ordering::AcqRel) == sequence {
            return None;
        }
        match clipboard_win::get_clipboard::<Vec<String>, _>(clipboard_win::formats::FileList) {
            Ok(paths) if !paths.is_empty() && paths.len() <= MAX_FILES => {
                source::queue_offer(bridge.clone(), paths, sequence)
            }
            Ok(_) => {}
            Err(error) => {
                bridge.last_local_sequence.store(0, Ordering::Release);
                tracing::warn!(%error, "Cannot read local clipboard file list");
            }
        }
        return None;
    }
    bridge.local.lock().unwrap().take();
    Some(ClipboardMessage::SendInitiateCopy(
        formats
            .into_iter()
            .filter(|f| f.id() != ClipboardFormatId::CF_HDROP && !file_format(f))
            .collect(),
    ))
}

pub(crate) struct FileOwner {
    bridge: Arc<Bridge>,
    session: Option<ClipboardSession>,
    offer: Option<Arc<Offer>>,
}
impl FileOwner {
    pub(crate) fn new(bridge: Arc<Bridge>) -> Self {
        Self {
            bridge,
            session: None,
            offer: None,
        }
    }
    fn clear(&mut self) {
        self.session.take(); // The library checks ownership before clearing Windows data.
        self.offer.take();
    }
    fn publish(&mut self, offer: Arc<Offer>) -> Result<(), String> {
        self.clear();
        let mut files = Vec::with_capacity(offer.files.len());
        for entry in offer.files.iter() {
            let descriptor = &entry.descriptor;
            let name = descriptor.relative_path.as_ref().map_or_else(
                || descriptor.name.clone(),
                |p| format!("{p}\\{}", descriptor.name),
            );
            let source = entry.source.clone();
            let factory =
                FactorySource::new(move || Ok(SourceReader::seekable(source.clone().open()?)));
            let mut file = VirtualFile::new(&name, Box::new(factory)).map_err(|e| e.to_string())?;
            if let Some(size) = descriptor.file_size {
                file = file.with_size(size);
            }
            if let Some(attributes) = descriptor.attributes {
                file = file.with_attributes(attributes.bits());
            }
            if let Some(time) = descriptor.last_write_time {
                file = file.with_last_write_time(time);
            }
            files.push(file);
        }
        // Register before OleSetClipboard emits notifications. Other sessions
        // route these same sources directly, preserving lazy remote-to-remote copies.
        *current().lock().unwrap() = Arc::downgrade(&offer);
        let session = virtual_file_transfer::copy_to_clipboard(files).map_err(|e| e.to_string())?;
        offer.sequence.store(sequence(), Ordering::Release);
        self.session = Some(session);
        self.offer = Some(offer);
        Ok(())
    }
    pub(crate) fn process_pending(&mut self) {
        let event = self.bridge.publication.lock().unwrap().take();
        match event {
            Some(Publication::Files(offer)) if self.bridge.active.load(Ordering::Acquire) => {
                if let Err(error) = self.publish(offer) {
                    tracing::warn!(%error, "Cannot publish remote clipboard files");
                }
            }
            Some(Publication::Clear) => self.clear(),
            _ => {}
        }
    }
}
impl Drop for FileOwner {
    fn drop(&mut self) {
        self.bridge.close();
        self.clear();
    }
}

#[cfg(test)]
mod native_tests;
