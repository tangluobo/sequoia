//! Direct scrcpy input. Gestures are sent during movement, never replayed as shell swipes on release.
use super::*;
fn key(action: u8, code: u32) -> Vec<u8> {
    let mut message = vec![0, action];
    message.extend_from_slice(&code.to_be_bytes());
    message.extend_from_slice(&[0; 8]);
    message
}
fn touch(owner: u64, action: u8, x: u16, y: u16, size: [u16; 2]) -> Vec<u8> {
    let mut message = vec![2, action];
    message.extend_from_slice(&owner.to_be_bytes());
    message.extend_from_slice(&u32::from(x.min(size[0].saturating_sub(1))).to_be_bytes());
    message.extend_from_slice(&u32::from(y.min(size[1].saturating_sub(1))).to_be_bytes());
    message.extend_from_slice(&size[0].to_be_bytes());
    message.extend_from_slice(&size[1].to_be_bytes());
    message.extend_from_slice(&(if action == 1 { 0u16 } else { u16::MAX }).to_be_bytes());
    message.extend_from_slice(&[0; 8]);
    message
}
fn android_key(code: u8, extended: bool) -> Option<u32> {
    Some(match (code, extended) {
        (1, _) => 4,
        (14, _) => 67,
        (15, _) => 61,
        (28, _) => 66,
        (0x48, true) => 19,
        (0x50, true) => 20,
        (0x4b, true) => 21,
        (0x4d, true) => 22,
        (0x47, true) => 3,
        (0x53, true) => 112,
        _ => return None,
    })
}
pub(super) async fn run(
    mut socket: impl AsyncWrite + Unpin,
    input: &mut super::super::pool::Inputs,
    shared: &Arc<Mutex<Shared>>,
    notifier: &Notifier,
) -> Result<()> {
    let mut contacts = std::collections::BTreeMap::new();
    let mut pending = None;
    let mut announced_sizes = std::collections::BTreeMap::new();
    loop {
        let mut event = match pending.take() {
            Some(event) => event,
            None => match input.recv().await {
                Some(event) => event,
                None => return Ok(()),
            },
        };
        let owner = event.owner;
        if !input.active(&event) {
            continue;
        }
        let motion = |event: &super::super::pool::Event| {
            matches!(
                event.input,
                Some(Input::Pointer {
                    button: None,
                    wheel: 0,
                    ..
                })
            ) && event.owner == owner
        };
        if motion(&event) {
            for _ in 0..64 {
                match input.try_recv() {
                    Ok(next) if motion(&next) => event = next,
                    Ok(next) => {
                        pending = Some(next);
                        break;
                    }
                    Err(_) => break,
                }
            }
        }
        let size = announced_sizes.get(&owner).copied().unwrap_or_else(|| {
            shared
                .lock()
                .ok()
                .and_then(|s| s.adb_video.map(|stats| stats.size))
                .unwrap_or([0, 0])
        });
        let Some(event) = event.input else {
            announced_sizes.remove(&owner);
            if let Some((x, y, size)) = contacts.remove(&owner) {
                timeout(
                    Duration::from_secs(2),
                    socket.write_all(&touch(owner, 1, x, y, size)),
                )
                .await
                .context("scrcpy 释放触摸超时")??;
            }
            continue;
        };
        let mut bytes = Vec::with_capacity(128);
        match event {
            Input::Resize { width, height } if width > 0 && height > 0 => {
                if let Some((x, y, old_size)) = contacts.remove(&owner) {
                    bytes = touch(owner, 1, x, y, old_size);
                }
                announced_sizes.insert(owner, [width, height]);
            }
            Input::AndroidKey(code) => {
                bytes.extend(key(0, code.into()));
                bytes.extend(key(1, code.into()));
            }
            Input::ScanCode {
                code,
                extended,
                down: true,
            } => {
                if let Some(code) = android_key(code, extended) {
                    bytes.extend(key(0, code));
                    bytes.extend(key(1, code));
                }
            }
            Input::Unicode { value, down: true } => {
                let mut text = String::new();
                let mut next = Some(value);
                while let Some(value) = next.take() {
                    if (32..=126).contains(&value) {
                        text.push(char::from_u32(value.into()).unwrap());
                    } else {
                        input_notice(shared, notifier, "ADB 键盘输入仅支持 ASCII 文本".into());
                    }
                    if text.len() >= 300 {
                        break;
                    }
                    match input.try_recv() {
                        Ok(super::super::pool::Event {
                            owner: next_owner,
                            input: Some(Input::Unicode { value, down: true }),
                        }) if next_owner == owner => next = Some(value),
                        Ok(other) => {
                            pending = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                if !text.is_empty() {
                    bytes.push(1);
                    bytes.extend_from_slice(&(text.len() as u32).to_be_bytes());
                    bytes.extend_from_slice(text.as_bytes());
                }
            }
            Input::Pointer {
                x,
                y,
                button: Some(1),
                down,
                ..
            } if size != [0, 0] => {
                if down {
                    contacts.insert(owner, (x, y, size));
                } else {
                    contacts.remove(&owner);
                }
                bytes = touch(owner, if down { 0 } else { 1 }, x, y, size);
            }
            Input::Pointer {
                x,
                y,
                button: None,
                wheel: 0,
                ..
            } if contacts.contains_key(&owner) && size != [0, 0] => {
                contacts.insert(owner, (x, y, size));
                bytes = touch(owner, 2, x, y, size);
            }
            Input::Pointer {
                button: Some(2),
                down: true,
                ..
            } => {
                bytes.extend(key(0, 4));
                bytes.extend(key(1, 4));
            }
            Input::Pointer { x, y, wheel, .. } if wheel != 0 && size != [0, 0] => {
                bytes.push(3);
                bytes.extend(u32::from(x).to_be_bytes());
                bytes.extend(u32::from(y).to_be_bytes());
                bytes.extend(size[0].to_be_bytes());
                bytes.extend(size[1].to_be_bytes());
                bytes.extend(0i16.to_be_bytes());
                bytes.extend((if wheel > 0 { 2048i16 } else { -2048i16 }).to_be_bytes());
                bytes.extend([0; 4]);
            }
            _ => (),
        }
        if !bytes.is_empty() {
            timeout(Duration::from_secs(2), socket.write_all(&bytes))
                .await
                .context("scrcpy 输入写入超时")??;
        }
    }
}
