use super::*;
pub(super) enum Message {
    Frame(Frame),
    Clipboard(String),
    Bell,
    Ignored,
    Closed,
}
pub(super) struct Screen {
    width: u16,
    height: u16,
    pixels: Vec<u32>,
}
impl Screen {
    pub(super) fn new(width: u16, height: u16) -> Result<Self> {
        let count = usize::from(width) * usize::from(height);
        ensure!(
            width > 0 && height > 0 && count <= 64 * 1024 * 1024,
            "VNC 画面尺寸无效或超过 6400 万像素"
        );
        Ok(Self {
            width,
            height,
            pixels: vec![0; count],
        })
    }
    pub(super) fn snapshot(&self) -> Frame {
        Frame {
            width: self.width,
            height: self.height,
            pixels: self.pixels.clone(),
            region: None,
        }
    }
    pub(super) fn request(&self, incremental: bool) -> [u8; 10] {
        [
            3,
            u8::from(incremental),
            0,
            0,
            0,
            0,
            (self.width >> 8) as u8,
            self.width as u8,
            (self.height >> 8) as u8,
            self.height as u8,
        ]
    }
    pub(super) async fn read_message<R: AsyncRead + Unpin>(
        &mut self,
        read: &mut R,
    ) -> Result<Message> {
        let kind = match read.read_u8().await {
            Ok(kind) => kind,
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(Message::Closed);
            }
            Err(error) => return Err(error.into()),
        };
        match kind {
            0 => {
                read.read_u8().await?;
                let count = read.read_u16().await?;
                for _ in 0..count {
                    let x = read.read_u16().await?;
                    let y = read.read_u16().await?;
                    let width = read.read_u16().await?;
                    let height = read.read_u16().await?;
                    let encoding = read.read_i32().await?;
                    if encoding == -223 {
                        ensure!(x == 0 && y == 0, "VNC DesktopSize 坐标无效");
                        *self = Self::new(width, height)?;
                        continue;
                    }
                    ensure!(
                        u32::from(x) + u32::from(width) <= u32::from(self.width)
                            && u32::from(y) + u32::from(height) <= u32::from(self.height),
                        "VNC 矩形超出画面范围"
                    );
                    match encoding {
                        0 => self.raw(read, x, y, width, height).await?,
                        5 => self.hextile(read, x, y, width, height).await?,
                        other => bail!("VNC 不支持的图像编码: {other}"),
                    }
                }
                Ok(Message::Frame(self.snapshot()))
            }
            1 => {
                read.read_u8().await?;
                read.read_u16().await?;
                let count = usize::from(read.read_u16().await?);
                // Discard unwanted colour-map data without an allocation.
                let mut bytes = [0; 6];
                for _ in 0..count {
                    read.read_exact(&mut bytes).await?;
                }
                Ok(Message::Ignored)
            }
            2 => Ok(Message::Bell),
            3 => {
                let mut padding = [0; 3];
                read.read_exact(&mut padding).await?;
                let length = read.read_u32().await? as usize;
                ensure!(length <= 16 * 1024 * 1024, "VNC 剪贴板文本过长");
                let mut bytes = vec![0; length];
                read.read_exact(&mut bytes).await?;
                Ok(Message::Clipboard(
                    bytes.into_iter().map(char::from).collect(),
                ))
            }
            other => bail!("VNC 未知服务端消息: {other}"),
        }
    }
    async fn raw<R: AsyncRead + Unpin>(
        &mut self,
        read: &mut R,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    ) -> Result<()> {
        // At most one scanline of encoded pixels is retained, never a second
        // full rectangle alongside the persistent framebuffer.
        let mut row = vec![0; usize::from(width) * 4];
        for yy in y..y + height {
            read.read_exact(&mut row).await?;
            let offset = usize::from(yy) * usize::from(self.width) + usize::from(x);
            for (dst, bytes) in self.pixels[offset..offset + usize::from(width)]
                .iter_mut()
                .zip(row.as_chunks::<4>().0)
            {
                *dst = u32::from_le_bytes(*bytes) & 0xffffff;
            }
        }
        Ok(())
    }
    fn fill(&mut self, x: u16, y: u16, width: u16, height: u16, color: u32) {
        for yy in y..y + height {
            let offset = usize::from(yy) * usize::from(self.width) + usize::from(x);
            self.pixels[offset..offset + usize::from(width)].fill(color);
        }
    }
    async fn hextile<R: AsyncRead + Unpin>(
        &mut self,
        read: &mut R,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    ) -> Result<()> {
        let mut background = None;
        let mut foreground = None;
        for ty in (0..height).step_by(16) {
            for tx in (0..width).step_by(16) {
                let tw = (width - tx).min(16);
                let th = (height - ty).min(16);
                let flags = read.read_u8().await?;
                ensure!(flags & !31 == 0, "VNC Hextile 标志无效");
                if flags & 1 != 0 {
                    self.raw(read, x + tx, y + ty, tw, th).await?;
                    continue;
                }
                if flags & 2 != 0 {
                    background = Some(read.read_u32_le().await? & 0xffffff);
                }
                let background = background.context("VNC Hextile 缺少背景颜色")?;
                self.fill(x + tx, y + ty, tw, th, background);
                if flags & 4 != 0 {
                    foreground = Some(read.read_u32_le().await? & 0xffffff);
                }
                if flags & 8 == 0 {
                    continue;
                }
                let count = read.read_u8().await?;
                for _ in 0..count {
                    let color = if flags & 16 != 0 {
                        read.read_u32_le().await? & 0xffffff
                    } else {
                        foreground.context("VNC Hextile 缺少前景颜色")?
                    };
                    let xy = read.read_u8().await?;
                    let wh = read.read_u8().await?;
                    let sx = u16::from(xy >> 4);
                    let sy = u16::from(xy & 15);
                    let sw = u16::from(wh >> 4) + 1;
                    let sh = u16::from(wh & 15) + 1;
                    ensure!(sx + sw <= tw && sy + sh <= th, "VNC Hextile 子矩形越界");
                    self.fill(x + tx + sx, y + ty + sy, sw, sh, color);
                }
            }
        }
        Ok(())
    }
}
