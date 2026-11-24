# 设计表编辑时保持行高

用户反馈在类型下拉及普通单元格进入编辑时，表格像闪动一样。逐帧复现确认：普通编辑使其后各行向上移动 1 个逻辑像素，退出后恢复。先前回归只检查活动单元格和下拉框的位置，没有覆盖后续行。

原因是字段表已用 `allocate_exact_size` 分配整行，随后又用 `Ui::put` 放入带内缩的编辑框；`put` 会将父布局游标推进到该内缩控件的底边，下一个表格行因此从不同位置开始。类型下拉内的文字框也使用了同一路径。

两个编辑器现在使用不推进父布局的 `Ui::place`。普通文字编辑仍使用原字体和尺寸，并垂直居中。整行、行选择器、网格线和滚动区域继续由原表格布局分配，编辑器仅占据单元格内部；没有改动默认 30 逻辑像素的行高、列宽、下拉锚点或提交/取消流程。

原 Java 只读参照为 `TableStructureView.java` 中 `setFixedCellSize`：行高由表格统一指定（默认字体 12 + 18），不会由编辑器改变。此修复保持这一行为，不声明整个设计器已与 Java 像素对等。

验证记录：

- `target/designer-height-before-1165.log` 在修复前通过真实 egui 控件复现位移：第二行从 y=121 变为 y=120，后续行一起变化。
- `app/tests/designer/types/geometry.rs` 在 100%、125%、150%、200% 缩放下逐帧比较全部六行的真实绘制坐标，覆盖 F2、文字输入、取消、类型下拉开合、候选提交和切换至第二行编辑。
- `target/designer-height-checks-1167.json` 记录格式、严格工作区全目标 Clippy、522 项工作区默认回归、全目标构建和 CLI 自检通过；74 项依原环境条件忽略。
- `scripts/qa/designer-row-height.ps1` 捕获独立空配置的原生静态表格及七种编辑状态，逐图检查不受编辑器遮挡的行选择器底边；开发版均为 y=120/150/180，行高 30。七项实际控件状态验证通过，全部窗口正常退出。初次图片分析误计表头边线，校正为三条数据行边线后，对已有原始帧缓冲重新分析通过，证据为 `target/designer-height-dev-1169-geometry.json`、`target/designer-height-geometry-1176.log`。

使用隔离配置，不连接保存的数据库、不写系统剪贴板、不注入操作系统输入。

2026-10-23 成品验收完成：`target/qa-candidate-designer-height-1172/tomato.exe`，工作区默认优化 release，99,139,584 字节；SHA-256 `FD58FA7F9DB9B267FC1F2ED73404893A6DAEE8098EFF8D931D9351682CF6F143`。生产检查点 `designer-height-1170` 的 2185 个文件在构建和验收前后均一致；相对前版只修改两个字段编辑器及其原生验收输入夹具，见 `target/designer-height-scope-1177.json`。嵌入图标与现有源图标一致。

同一 EXE 的静态表格及七种原生编辑状态全部正常退出，行边线均为 y=120/150/180。每个原生捕获目录的 GUI 哈希均与成品一致；状态及像素几何证明在 `target/designer-height-release-1173.json`、`target/designer-height-release-1173-geometry.json`，总验收记录为 `target/designer-height-acceptance-1175.json` 及成品 `build-info.json`。共享连接、全局/开发/DDNS 设置、下载历史、Cargo.lock、源图标和上一版 EXE 的保护哈希保持不变，三个用户实例保留。

与前版 1152 同批空闲对照：自动后端工作集 168.69 → 168.34 MiB、私有提交 255.58 → 255.30 MiB；Vulkan 工作集 114.36 → 113.91 MiB、私有提交 189.28 → 187.71 MiB。两版采用独立空配置、15 秒预热及十个一秒样本，本次差异属于小幅波动，不能表示活动设计器内存峰值下降。该修复不引入后台任务、定时器或缓存，字段与候选仍按可见行绘制。证据为 `target/window-probes/designer-height-memory-1174/validation.json`。
