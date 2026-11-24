use super::*;
pub(crate) fn choose(
    session: &tomato_services::terminal::Terminal,
    workspace: &mut Workspace,
    state: &TransferState,
) {
    if !matches!(
        state,
        TransferState::ChooseUpload | TransferState::ChooseDirectory
    ) {
        workspace.choosing_transfer = false;
    }
    if !workspace.choosing_transfer
        && matches!(
            state,
            TransferState::ChooseUpload | TransferState::ChooseDirectory
        )
    {
        workspace.choosing_transfer = true;
        let choice = if *state == TransferState::ChooseUpload {
            rfd::FileDialog::new()
                .set_title("选择要上传的文件")
                .pick_files()
                .map(Choice::Upload)
        } else {
            rfd::FileDialog::new()
                .set_title("选择保存目录")
                .pick_folder()
                .map(Choice::Download)
        }
        .unwrap_or(Choice::Cancel);
        let _ = session.transfer_choice(choice);
    }
}
