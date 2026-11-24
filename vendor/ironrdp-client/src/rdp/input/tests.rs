use super::*;
use ironrdp_pdu::input::fast_path::KeyboardFlags;
use std::{future::Future, pin::Pin};
use tokio::sync::Semaphore;

struct Wire {
    started: mpsc::UnboundedSender<Vec<u8>>,
    completed: mpsc::UnboundedSender<Vec<u8>>,
    permits: Arc<Semaphore>,
}
impl FramedWrite for Wire {
    type WriteAllFut<'a> = Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>>;
    fn write_all<'a>(&'a mut self, bytes: &'a [u8]) -> Self::WriteAllFut<'a> {
        Box::pin(async move {
            self.started.send(bytes.to_vec()).unwrap();
            self.permits.acquire().await.unwrap().forget();
            self.completed.send(bytes.to_vec()).unwrap();
            Ok(())
        })
    }
}
type Observed = mpsc::UnboundedReceiver<Vec<u8>>;
fn wire(permits: usize) -> (Wire, Observed, Observed, Arc<Semaphore>) {
    let (started, seen) = mpsc::unbounded_channel();
    let (completed, done) = mpsc::unbounded_channel();
    let permits = Arc::new(Semaphore::new(permits));
    (
        Wire {
            started,
            completed,
            permits: permits.clone(),
        },
        seen,
        done,
        permits,
    )
}
async fn receive(receiver: &mut Observed) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(2), receiver.recv())
        .await
        .unwrap()
        .unwrap()
}
async fn ready(sender: &FastPathSender) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !sender.state.active.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn keys() -> Events {
    [
        FastPathInputEvent::KeyboardEvent(KeyboardFlags::empty(), 0x1d),
        FastPathInputEvent::KeyboardEvent(KeyboardFlags::empty(), 0x2f),
        FastPathInputEvent::KeyboardEvent(KeyboardFlags::RELEASE, 0x2f),
        FastPathInputEvent::KeyboardEvent(KeyboardFlags::RELEASE, 0x1d),
    ]
    .into_iter()
    .collect()
}
fn encoded(events: &Events) -> Vec<u8> {
    ironrdp_core::encode_vec(&FastPathInput::new(events.to_vec()).unwrap()).unwrap()
}

#[cfg(feature = "clipboard")]
#[tokio::test]
async fn clipboard_format_list_is_written_before_later_ctrl_v_even_when_control_is_stalled() {
    use ironrdp_cliprdr::pdu::{ClipboardFormat, ClipboardFormatId, ClipboardPdu, FormatList};
    let lane = Lane::new();
    let fast = lane.sender();
    let (controls, mut receiver) = InputSender::channel(Some(&lane));
    let (wire, mut started, mut completed, permits) = wire(0);
    let mut writer = Writer::start(wire, Some(lane.clone()));
    ready(&fast).await;
    let formats = vec![ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)];
    controls
        .send(RdpInputEvent::Clipboard(
            ClipboardMessage::SendInitiateCopy(formats.clone()),
        ))
        .unwrap();
    let keys = keys();
    fast.try_send(keys.clone()).unwrap();
    // The control consumer represents a decoder that has not returned yet.
    assert!(
        tokio::time::timeout(Duration::from_millis(20), started.recv())
            .await
            .is_err()
    );
    let control = receiver.recv().await.unwrap();
    assert!(matches!(
        control.event,
        RdpInputEvent::Clipboard(ClipboardMessage::SendInitiateCopy(_))
    ));
    let format_list = ironrdp_core::encode_vec(&ClipboardPdu::FormatList(
        FormatList::new_unicode(&formats, true).unwrap(),
    ))
    .unwrap();
    let expected = format_list.clone();
    let write = tokio::spawn(async move {
        writer.write_owned(format_list).await.unwrap();
        writer
    });
    assert_eq!(receive(&mut started).await, expected);
    // Dequeue/encoding alone must not release Ctrl+V before the TLS write ACK.
    assert!(completed.try_recv().is_err());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), started.recv())
            .await
            .is_err()
    );
    permits.add_permits(1);
    assert_eq!(receive(&mut completed).await, expected);
    let _writer = write.await.unwrap();
    lane.complete_control(control.sequence);
    assert_eq!(receive(&mut started).await, encoded(&keys));
    permits.add_permits(1);
    assert_eq!(receive(&mut completed).await, encoded(&keys));
}

#[tokio::test]
async fn idle_mouse_bypasses_decoder_and_activation_pause_preserves_input_fifo() {
    let lane = Lane::new();
    let fast = lane.sender();
    let (_controls, _receiver) = InputSender::channel(Some(&lane));
    let (wire, mut started, mut completed, _) = wire(8);
    let _writer = Writer::start(wire, Some(lane.clone()));
    ready(&fast).await;
    let motion: Events = [FastPathInputEvent::MouseEvent(MousePdu {
        flags: PointerFlags::MOVE,
        number_of_wheel_rotation_units: 0,
        x_position: 21,
        y_position: 34,
    })]
    .into_iter()
    .collect();
    fast.try_send(motion.clone()).unwrap();
    assert_eq!(receive(&mut completed).await, encoded(&motion));
    assert_eq!(receive(&mut started).await, encoded(&motion));
    lane.pause(true);
    let key_events = keys();
    for event in &key_events {
        fast.try_send([*event].into_iter().collect()).unwrap();
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(20), started.recv())
            .await
            .is_err()
    );
    lane.pause(false);
    for event in key_events {
        let expected = encoded(&[event].into_iter().collect());
        assert_eq!(receive(&mut completed).await, expected);
    }
}

#[tokio::test]
async fn failed_control_enqueue_does_not_leave_an_unreachable_fence() {
    let lane = Lane::new();
    let fast = lane.sender();
    let (controls, receiver) = InputSender::channel(Some(&lane));
    let (wire, _started, mut completed, _) = wire(4);
    let _writer = Writer::start(wire, Some(lane));
    ready(&fast).await;
    drop(receiver);
    assert!(controls.send(RdpInputEvent::Close).is_err());
    let keys = keys();
    fast.try_send(keys.clone()).unwrap();
    assert_eq!(receive(&mut completed).await, encoded(&keys));
}

#[tokio::test]
async fn reconnect_retires_resize_fence_without_replaying_old_keys() {
    let lane = Lane::new();
    let fast = lane.sender();
    let (controls, mut receiver) = InputSender::channel(Some(&lane));
    let (first, _started_first, _completed, _) = wire(0);
    let old_writer = Writer::start(first, Some(lane.clone()));
    ready(&fast).await;
    controls
        .send(RdpInputEvent::Resize {
            width: 800,
            height: 600,
            scale_factor: 100,
            physical_size: None,
        })
        .unwrap();
    fast.try_send(keys()).unwrap();
    let resize = receiver.recv().await.unwrap();
    lane.pause(true);
    lane.complete_control(resize.sequence);
    drop(old_writer);
    let (second, _started_second, mut completed, _) = wire(8);
    let _new_writer = Writer::start(second, Some(lane));
    ready(&fast).await;
    let new_keys: Events = [FastPathInputEvent::KeyboardEvent(
        KeyboardFlags::empty(),
        0x20,
    )]
    .into_iter()
    .collect();
    fast.try_send(new_keys.clone()).unwrap();
    assert_eq!(receive(&mut completed).await, encoded(&new_keys));
    assert!(completed.try_recv().is_err());
}
