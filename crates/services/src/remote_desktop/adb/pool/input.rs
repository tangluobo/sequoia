//! Bounded per-view input identity and cancellation without a detached task per view.
use super::*;
use std::collections::{BTreeSet, VecDeque};
#[derive(Clone, Copy)]
pub(in crate::remote_desktop::adb) struct Event {
    pub owner: u64,
    pub input: Option<Input>,
}
pub(in crate::remote_desktop::adb) struct Inputs {
    receiver: mpsc::Receiver<Event>,
    views: Arc<Mutex<Views>>,
    changed: Arc<tokio::sync::Notify>,
    known: BTreeSet<u64>,
    released: VecDeque<Event>,
}
impl Inputs {
    pub(super) fn clear(&mut self) {
        self.known.clear();
        self.released.clear();
        for _ in 0..64 {
            if self.receiver.try_recv().is_err() {
                break;
            }
        }
    }
    pub(super) fn new(
        receiver: mpsc::Receiver<Event>,
        views: Arc<Mutex<Views>>,
        changed: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            receiver,
            views,
            changed,
            known: Default::default(),
            released: Default::default(),
        }
    }
    fn live(&self, owner: u64) -> bool {
        self.views
            .lock()
            .is_ok_and(|v| v.subscribers.contains_key(&owner))
    }
    pub(in crate::remote_desktop::adb) fn active(&self, event: &Event) -> bool {
        event.input.is_none() || self.live(event.owner)
    }
    fn retire(&mut self) {
        let mut retired = Vec::new();
        for owner in &self.known {
            if !self.live(*owner) {
                retired.push(*owner);
            }
        }
        for owner in retired {
            self.known.remove(&owner);
            self.released.push_back(Event { owner, input: None });
        }
    }
    pub(in crate::remote_desktop::adb) async fn recv(&mut self) -> Option<Event> {
        loop {
            self.retire();
            if let Some(event) = self.released.pop_front() {
                return Some(event);
            }
            tokio::select! { biased;
                _ = self.changed.notified() => {},
                event = self.receiver.recv() => {
                    let event = event?;
                    if self.live(event.owner) { self.known.insert(event.owner); return Some(event); }
                }
            }
        }
    }
    pub(in crate::remote_desktop::adb) fn try_recv(
        &mut self,
    ) -> Result<Event, mpsc::error::TryRecvError> {
        self.retire();
        if let Some(event) = self.released.pop_front() {
            return Ok(event);
        }
        loop {
            let event = self.receiver.try_recv()?;
            if self.live(event.owner) {
                self.known.insert(event.owner);
                return Ok(event);
            }
        }
    }
}
