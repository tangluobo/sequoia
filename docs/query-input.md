# 新建查询输入修复

对照原 Java `AbstractDbHandler.handleNewQuery` 与 `SqlEditorView`，新建查询应打开可编辑的空白 SQL 标签。Rust 的两个入口最终都经过 `open_query_file`，原先只选中了标签，没有把键盘焦点交给 SQL 编辑器。

修复前，完整主窗口的真实 egui 控件回归确认 `immediate=false, clicked blank space=true`：直接输入丢失，点击空白编辑区后输入可用。证据保留在 `target/query-input-repro-1226.log`。旧测试大多先给模型填入 SQL，再点击文字位置，未覆盖这个场景。

编辑状态现在保留一个单次焦点请求，在可编辑控件首次显示时消费；打开已有文件时等待读取结束，控件被模态界面禁用时保留请求。普通输入、工具栏操作与后续重绘不会再次抢焦点。空白 SQL 也保留 17 点行高的字体布局，不向实际 SQL 添加空格或换行，使首行光标和行号有正常几何尺寸。修改不增加正常查询的常驻文本副本、缓存或后台任务。

新增回归覆盖完整主窗口中新建空白查询、立即输入中文 SQL、点击下方空白继续输入、通过 Ctrl+Enter 在独立 SQLite 会话中执行；编辑器检查覆盖多个空白点击位置、加载期间延后焦点、转向另一个输入框后不抢回焦点，以及空白首行光标高度。修复后的相关工作区筛选回归通过，见 `target/query-input-focused-1227.log`。

原生窗口验收使用显式启用的 `query_input` 模式及独立空配置，只向本程序的 egui 输入流发送事件。它通过正常控制器分别创建对象/数据库范围查询和查询树查询，检查立即输入、真实 Tab 补全、中文、点击空白、全选清空后重新输入、实际 SQLite 执行、GPU 帧缓冲和正常退出。它不操作系统鼠标键盘、不写系统剪贴板、不访问用户保存的主机。运行脚本为 `scripts/qa/query-input.ps1`。

开发 GUI 的两个原生主窗口场景已通过，证据为 `target/query-input-dev-1240.json`。空白首行高度均为 17，立即输入 `SEL` 后实际 Tab 补为 `SELECT`，中文输入和空白处续写保留；实际 Ctrl+A/Backspace 清空后重新输入，Ctrl+Enter 的 SQLite 结果为“输入正常”。两个窗口均正常退出，已查看的两个 1200×900 截图逐字节相同，帧缓冲 SHA256 为 `202C4A09535221091D0042AB2602289B28B6872D639C50AF229D5877CEDB8F4F`。

本轮最终源范围冻结为 `query-input-1238`，共 2190 个生产文件。相对 1212 只有八个 UI 文件变化（包括隔离验收和控件回归），服务层、依赖锁与图标均未变化；见 `target/query-input-scope-1241.json`。原 Java 目录保持只读。

完整工作区默认回归已通过 530 项，75 项保持原有环境条件下的忽略，50 个测试套件没有失败。最终格式与严格全目标 Clippy 通过，记录分别为 `target/query-input-fmt-1238.log`、`target/query-input-clippy-1238.log`。原生验收输入的 Ctrl 修饰键同时设置 Windows 逻辑快捷键标志，确保 Ctrl+A 真正全选；初次原生验收在清空检查处拒绝通过，修正后重新验收。

优化成品为 `target/qa-candidate-query-input-1232/tomato.exe`，98,545,152 字节，SHA256 `99922BC698D876E632223337FC717E154F5191D4469E9422096EC002D077E677`。同一成品在两个原生窗口中重复完成上述流程并正常退出，见 `target/query-input-release-1233.json`；两张成品截图均与已检查开发图逐像素、逐字节一致，见 `target/query-input-framebuffers-1242.json`。完整全目标构建和 CLI 自检通过，嵌入图标全部帧与源图标原字节相符。最终 `build-info.json` 状态为 `verified`，汇总为 `target/query-input-acceptance-1235.json`。

同批四次空闲内存对照中，1212→1232 的 auto 工作集为 212.02→212.08 MiB、私有提交 255.39→255.21 MiB；Vulkan 工作集 145.84→145.84 MiB、私有提交 190.38→190.41 MiB，基本持平。每次独立空配置，预热 15 秒、采样 10 次，保留一个用户实例；该结果不表示活动查询的峰值上限。证据为 `target/window-probes/query-input-memory-1234/validation.json`。五个用户配置文件、依赖锁、源图标及旧版 1212 EXE 的八个保护输入哈希均未改变，详见 `target/query-input-protected-1223.json`。
