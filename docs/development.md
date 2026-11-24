# 开发目录迁移

对照只读原项目 `DevelopmentManagerPane`、`DevelopmentConfigManager`、`DevelopmentProjectSettingsDialog` 和共享 `SqlEditorPane`。

开发导航现在接入真实后台命令：目录添加、项目配置、SDK/构建工具管理、文件树与代码读写、Git/实时变更、选中文件提交、显式推送、回滚与忽略、Maven/Gradle 依赖、Java main 与 npm 脚本运行、重启、停止和日志。Flutter 与原版一样提供 SDK 配置；原版没有 Flutter 项目启动器。

直接加载原 `development.json`，支持数组根节点以及旧 `jdks`/`nodes` 合并到运行环境。未知字段保留；第一次修改备份原字节；加载不写回；版本摘要拒绝覆盖外部修改。项目切换、文件切换与关闭应用保留未保存确认。文件写入再次检查版本，失败保留草稿。

后台进程使用独立 Windows Job Object / Unix 进程组，停止和关闭释放整个子进程树。日志与依赖使用独立进程槽，刷新依赖不会停止应用。日志每项目上限 2 MB、缓存至多 32 项；单个编辑文件上限 8 MiB，撤销缓存上限 32 MiB；目录与 Git 结果有数量上限。文件树与版本树仅绘制可见行。Git 操作限定所选项目的相对路径，提交选中文件不会连带提交其他已暂存文件。

Maven 启动先编译项目，再在选中模块解析 runtime/test 依赖。识别原版三种失败缓存诊断，依次使用 `-U` 重试和离线本地 JAR 回退，依赖解析沿用成功的离线参数。增量状态按模块和 main/test 分开：无变更跳过编译，至多 24 个变更文件使用所选 JDK 的 javac，资源新增/修改/删除同步至 classes；构建描述、SDK/构建工具、settings、wrapper、profile 或源码删除导致完整重编译。状态最多 8 项、合计 20,000 个文件记录，超限或无法增量时回退真实 Maven 构建。

长期运行的应用不额外保留命令捕获副本。界面确认收到日志后，后台释放对应前缀；任务结束后再确认末批输出，释放大缓冲。界面回滚内容仍受每项目 2 MB 上限约束。日志颜色、粗体和单击 HTTP 链接按原 `HighlightedLogPane` 规则，尾部标点不计入网址；只读文本仍可选择复制。

代码编辑器接入语法颜色、40px 行号区、12px 变更标记区、点击标记跳转、Ctrl+S、Ctrl+D、Tab/Shift+Tab 和有界粘贴。保存后后台重新计算真实 Git 差异。版本树支持目录复选、部分选中状态与文件打开；已删除文件读取 HEAD 内容供复制和查看。

可复验资料：

- `crates/core/tests/java/DevelopmentOracle.java` 编译调用原版源码，配置金样在 `crates/core/tests/fixtures/development/`。
- `scripts/qa/development.ps1` 创建空连接配置和不含 SDK 的独立项目，捕获真实原生窗口；不会启动保存的用户项目。
- 服务测试使用临时文件、临时 Git 仓库和真实 Node 的自有脚本；界面测试操作实际项目、文件、保存和运行控件。
- `scripts/qa/development-runtime.ps1` 使用真实 Node/JDK/Maven 和独立复制的离线 Maven 仓库，验证启动/重启/停止、子进程回收、Java 编译/增量/资源同步、失败缓存回退及界面文件菜单的启动流程。
- `crates/services/tests/development_gradle.rs` 使用自有离线 wrapper、复制的 Gradle 和独立 Gradle 用户目录，覆盖 main/test 编译与启动和依赖 action，不使用保存的用户项目。

原版创建了“Git 仓库 / 实时 / 刷新”标题栏，却没有把该标题栏加入版本页。本实现保留原窗口的可见结构；实时快照能力在后台命令中保留，不把未显示的切换控件描述为已验收的界面入口。

2026-10-21 检查点 `development-404`：格式、严格 Clippy、448 项默认测试（69 项条件测试默认忽略，45 组）、全目标构建、CLI 自检、优化构建及 EXE 图标检查通过。6 项独立真实流程覆盖 Node 启停/重启与子进程回收、Maven 编译/增量/资源同步及失败缓存回退、界面文件菜单运行 Java、日志链接点击和 Gradle main/test/依赖。另有独立 Maven 检查确认增量运行没有隐式回退完整 Maven。

发布版五种开发窗口状态（空白、编辑、版本、依赖、日志）与同配置的原 Java 窗口对照；原图标、展开箭头、35px 树行、41px 控制栏、44px 编辑工具栏及25px内部标签已接入。仍存在字体光栅、原生控件绘制和少量位置差异；项目配置、SDK/构建工具弹窗尚未完成所有状态的原生视觉对照，不能声明整个开发模块或全应用逐像素一致。

成品为 `target/qa-release-development-404/release/tomato.exe`，SHA-256 `A1CB92C40CC6DBD6E03CB1AC9A1EBB363D8286FEF28143007DC8520FAAD6F650`。共享连接、global、development、DDNS 配置和旧 EXE 的摘要保持；旧连接配置只读校验为 60 连接 / 4 分组。没有连接保存主机。

隔离空配置、自动后端、预热 15 秒 / 采样 10 秒，工作集 180.57 MiB、私有提交 204.33 MiB。同轮顺序采样的 369 为 180.03 / 204.57 MiB，基本持平；这不是活动开发项目的内存峰值，也不能据此宣称所有场景内存下降。完整证据在成品同目录 `build-info.json`。


2026-10-21 后续检查点 `development-settings-419` 接续上述运行能力，项目/SDK/构建工具设置及新增目录的布局和实际流程见 [项目设置](development-settings.md)。它还修复查询编辑器内边距/对齐，以及有界长命令输出丢失末尾诊断和日志 CR/LF 差异。最新优化成品、454 项默认回归、19 种成品窗口和空闲配对采样的证据以该文档及新成品 build-info.json 为准；404 的数值保留为历史记录。
