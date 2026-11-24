use super::*;
pub(in crate::panels::terminal::tools) fn action(
    dialog: &Dialog,
) -> Option<anyhow::Result<ManagementAction>> {
    if !matches!(
        dialog.kind,
        Form::InitializePv | Form::VolumeGroup | Form::ExtendLv | Form::Mount
    ) {
        return None;
    }
    Some((|| {
        let value = |i: usize| {
            dialog
                .fields
                .get(i)
                .map(|v| v.trim())
                .unwrap_or_default()
                .to_owned()
        };
        let action = match dialog.kind {
            Form::InitializePv => ManagementAction::InitializePv(value(0)),
            Form::VolumeGroup => ManagementAction::VolumeGroup {
                pv: value(0),
                vg: value(1),
            },
            Form::ExtendLv => ManagementAction::ExtendLv {
                device: value(0),
                amount: value(1).to_ascii_uppercase(),
            },
            _ => ManagementAction::Mount {
                device: value(0),
                directory: value(1),
                filesystem: match value(2).as_str() {
                    "不格式化" => None,
                    "ext4" => Some("ext4".into()),
                    "xfs" => Some("xfs".into()),
                    _ => anyhow::bail!("文件系统应为 不格式化/ext4/xfs"),
                },
                persist: match value(3).as_str() {
                    "true" => true,
                    "false" => false,
                    _ => anyhow::bail!("fstab 设置无效"),
                },
            },
        };
        management::command(&action)?;
        Ok(action)
    })())
}
pub(in crate::panels::terminal::tools) fn is_storage(dialog: &Dialog) -> bool {
    action(dialog).is_some()
        || matches!(
            dialog.confirm,
            Some(Operation::Manage(
                ManagementAction::InitializePv(_)
                    | ManagementAction::VolumeGroup { .. }
                    | ManagementAction::ExtendLv { .. }
                    | ManagementAction::Mount { .. }
            ))
        )
}
pub(super) fn confirmation(kind: &Form) -> &'static str {
    match kind {
        Form::InitializePv => "初始化磁盘会清除其全部数据",
        Form::VolumeGroup => "将 PV 加入卷组",
        Form::ExtendLv => "扩展逻辑卷及其文件系统",
        _ => "不格式化时仅挂载；格式化将清除设备数据",
    }
}
