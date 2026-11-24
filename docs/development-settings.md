# 开发目录与运行环境设置

对照只读 Java `DevelopmentProjectSettingsDialog.java`、`DevelopmentManagerPane.showAddDirectoryDialog` 和 `DevelopmentConfigManager.java`。通过独立 JavaFX 场景取得原窗口尺寸和控件位置，未运行已保存的用户项目。

项目配置保留原项目名称、识别类型、JDK/Node/Flutter 和构建工具选择；按环境类型过滤，显示名称和版本。SDK 管理保留原新增/删除操作，构建管理支持新增、修改、双击修改和删除。修改工具时保留 ID 和未知字段；切换类型清空原类型的路径、版本和 settings，Gradle 禁用 Maven settings 输入。删除引用的环境后更新当前项目的选择。

路径校验和版本识别通过后台命令处理。Node 验证文件，JDK/Flutter 和构建工具验证目录，Maven 另验证 settings 文件。JDK 优先读取有界 release 文件，否则运行所选环境自己的版本命令。命令超时为 5 秒；关闭或切换后的旧回复不能覆盖新弹窗。版本命令的启动器警告和分隔线不作为版本号。

新增目录使用原 460×220 客户区、绿色确认按钮和原字段顺序。空字段与无效路径显示警告；重名按不区分大小写比较，确认后才添加。浏览后在后台识别项目类型，确认时重新检查目录。名称由用户填写；添加其他目录不会切走当前已选项目。所有表单先修改草稿，父窗口取消不持久化。

继续直接读写旧 development.json，保留未知字段。第一次保存保留原文件字节备份，持锁并校验外部修改后原子写入。关闭表单释放文本撤销状态；设置列表仅绘制可见行，单个设置字段的撤销状态限制为 512 KiB，没有额外的后台刷新循环。

原版窗口对照工具：

- `crates/core/tests/java/DevelopmentSettingsOracle.java`：项目、SDK/构建管理、选中、新增、修改、Gradle 和类型切换共 10 种状态。
- `crates/core/tests/java/DevelopmentAddDirectoryOracle.java`：实际原版新增目录窗口。
- `scripts/qa/development-settings.ps1`：使用隔离配置捕获 Rust 实际窗口；预览返回的动作不派发，避免截图触发版本命令或配置保存。
- `crates/ui-egui/src/app/tests/development.rs`：通过真实控件、后台路径校验和配置仓库验证保存、取消及引用选择，不发送操作系统输入。
- `crates/services/tests/development_settings.rs`：真实临时文件和版本命令，覆盖无效路径、JDK 元数据、命令失败和带空格/特殊字符的路径。

字体光栅、控件阴影和焦点边缘仍需逐状态比较，不能把布局对照解释为全应用逐像素一致。最终构建、测试和内存证据如下；旧 `development-404` 成品不包含本次设置重写。


## 2026-10-21 检查点 419

优化成品 `target/qa-release-development-settings-419/release/tomato.exe`，SHA-256 `CD8529A52D341A1D34190C059F395EA08167D1777DB0AE519EC330E523B7030B`，96,078,336 字节。`build-info.json` 记录成品、源码和各项证据；1,501 个运行源文件构建前后摘要一致。416 优化构建因发现输入框内边距问题主动撤回，未作为最终成品，记录保留在 target。

格式、严格工作区 Clippy、454 项默认测试（69 项条件测试默认忽略，46 组）、全目标构建和开发/发布 CLI 自检通过。默认测试中的 3 条设置流程操作实际控件、后台校验和配置仓库：SDK 空值校验/新增/取消/JDK 引用保存；目录重名确认的拒绝及接受；构建工具类型切换/未知字段/旧 ID/取消删除。真实 Node、Maven 和 Gradle 的 3 项运行复测通过，包括子进程回收、增量与资源同步、缓存回退、main/test 编译和依赖加载；均使用自有文件与离线缓存。

成品捕获 11 种设置窗口、3 种查询日期/时间窗口及 5 种开发主页面。`scripts/qa/compare-development-settings.py` 对 11 种实际 Java/Rust 截图检查尺寸，并记录全部不同像素，尺寸匹配但像素不完全一致；检查不设置掩盖差异的容差，图片也不作为所有交互对等的证明。证据为 `target/development-settings-comparison-release-419.json`、`target/window-probes/development-settings-release-419/validation.json` 及 `target/development-settings-final-validation-419.json`。

共享旧配置只读检查为 60 个连接、4 个分组；共享 connections/global/development/DDNS 文件、原 Java 参考和旧 EXE 摘要保持，EXE 六尺寸图标字节一致。没有访问保存的主机。隔离空配置、自动后端、预热 15 秒/采样 10 秒：419 工作集 180.63 MiB、私有提交 204.57 MiB；同轮 404 为 179.88 / 204.29 MiB，基本持平。两次采样都保留另一个用户实例，不能推断活动开发、远程桌面或其他场景的内存下降。

查询结果原位编辑器另按原 SqlEditorView.createTextField 恢复 4px 水平内边距、黑色文字和垂直居中，并限制绘制范围；打开表已有对应设置继续保持。长命令捕获用有界环形尾部保留末尾错误和 Gradle classpath，长期运行命令仍不保存第二份捕获历史；日志 CR/LF/CRLF 跨管道块正常化，保留跨块 UTF-8 和 ANSI 状态。完整应用迁移和全部界面逐像素一致尚未完成。
