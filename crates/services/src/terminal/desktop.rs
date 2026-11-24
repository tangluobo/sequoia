use anyhow::{Result, bail};
use tomato_core::Connection;
pub fn launch_desktop(connection: &Connection) -> Result<()> {
    match connection.kind() {
        "RDP" | "VNC" | "ADB" => bail!("{} 使用标签内的 DesktopSession 会话", connection.kind()),
        "SPICE" => bail!("内置 SPICE 协议仍在开发中"),
        other => bail!("{other} 不是远程桌面类型"),
    }
}
