# Tomato Rust

将 JavaFX Tomato 迁移到原生 Rust 工作台。参考 [WordCraft](https://github.com/storytold/wordcraft) 的 Rust workspace、egui/eframe + wgpu、业务引擎与 UI 分离、CLI 共用命令以及跨平台 GitHub Actions。

**这是正在迁移的版本，尚未达到旧项目全部功能对等。** 具体已实现、外部客户端委托和待迁移功能见 [功能迁移表](docs/migration.md)。旧项目位于 `D:\Data\Git\tangluobo\tomata`，本项目独立位于 `F:\data\git\rust\tomato`。

最新交付：[1682 EXE](dist/tomato-1682-windows-x64/tomato.exe)、[便携包](dist/tomato-1682-windows-x64/tomato-portable.zip)、[Windows 安装器](dist/tomato-1682-windows-x64/tomato-1682-setup.exe)。本轮实际流程与验收边界见 [1682 迁移记录](docs/migration-1682.md)。

S3 与证书存储的连接、临时密钥和完整签发上传流程见 [S3 迁移说明](docs/s3.md)。云服务、DDNS、ACME 证书与 Kafka/RocketMQ 的本轮原生实现和验证边界见 [云与消息队列迁移说明](docs/cloud-mq.md)。历史文件视图优化 GUI 运行 `F:/data/git/rust/tomato/target/qa-candidate-file-views-645/tomato.exe`（图标/详情/多列视图铺满内容区，接通空白拖选和边缘滚动；473 项默认回归、14 组协议/视图组合及三种原生窗口检查通过，见 [文件视图与框选](docs/file-selection.md)；同目录 build-info.json 记录成品和验证证据，不附带旧 CLI）。RDP 仍有整屏卡顿及文件/声音实际使用验收缺口，见 [RDP 通道](docs/rdp-channels.md)。此前项目设置优化构建为 `F:/data/git/rust/tomato/target/qa-release-development-settings-419/release/tomato.exe`（接通项目/SDK/构建工具配置和目录确认，修复查询编辑器对齐与有界日志末尾诊断；454 项默认测试、3 项真实运行流程及 19 种成品窗口状态通过，详见 [项目设置](docs/development-settings.md)；全量迁移和逐像素对等仍未完成）；同目录 `build-info.json` 记录产物和验证证据，主标签及溢出菜单的原版对照见 [标签样式](docs/tab-style.md)。打开表的原位编辑、点击/键盘事件与保存验收见 [单元格编辑](docs/cell-editing.md)。设计表工具栏、单线网格、查询结果及 MQ 内部标签见 [设计器与内部标签](docs/inner-style.md)。标题栏底线修复见 [分隔线说明](docs/title-divider.md)。MQ 详情列宽、缩窗和文字对齐见 [布局验收](docs/text-layout.md)。RDP 已解除 GUI 对协议推进的阻塞，整体流畅度和活动内存限制见 [RDP 执行与图像上传](docs/rdp-rendering.md)。旧 `target/release` 与 dist 未覆盖。

本轮数据库树按需加载优化 GUI：`F:/data/git/rust/tomato/target/qa-candidate-database-lazy-669/tomato.exe`。连接只列库，双击逐级加载目标库/模式；修正未打开节点的展开箭头、搜索中误显示的对象文件夹和空目录重复加载。475 项默认回归、19 项隔离后端回归、12 条真实数据库 UI 流程及成品原生窗口检查通过，旧配置不变；见 [数据库树按需加载](docs/database-tree-loading.md)。

RDP 修复继续进行：900 优化候选版实连原配置“110”，用户在标签与独立窗口均确认单光标但窗口内容仍卡顿；889 以及 860 关闭全部诊断后也未通过，均不作为修复完成版交付。已加入保持像素精度的小波逆变换 SIMD 和原地重叠安全矩形移动，减少临时像素分配；计时改为有界内存记录、关闭后集中写出。继续对齐 Java 的图形缓存与网络检测默认协商，保留完整窗口拖动、字体平滑和内存上限。新的优化构建与实际标签/独立窗口验收仍在进行；此前的回归、失败记录和测量边界见 [RDP 光标、输入与退出](docs/rdp-cursor-damage.md)。

此前 RDP 优化成品：`F:/data/git/rust/tomato/target/qa-candidate-rdp-rlgr-731/tomato.exe`，便携包 `target/tomato-windows-x64-rdp-rlgr-731-gui.zip`。该轮修正 RLGR 熵解码、完整帧发布、瓦片合并与音频线程生命周期；复杂 Progressive 场景实际提交吞吐提高，仍有流畅度验收缺口。478 项默认回归通过；测量、内存和限制见 [RDP 成品说明](docs/rdp-frame-delivery.md)。

## 运行

下载便携包后，Windows 可直接运行包内的 `tomato.exe`。下面的 Cargo 命令用于源码工作区。

安装 stable Rust（至少 1.95，含 MSVC 或所在平台的 C/C++ 工具链）。

```sh
cargo run -p tomato
cargo run -p tomato-cli -- self-check
cargo run -p tomato-cli -- --help
```

Linux 编译依赖：

```sh
sudo apt-get install build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxi-dev libxrandr-dev libxcursor-dev libasound2-dev libudev-dev libfontconfig1-dev fonts-noto-cjk unixodbc-dev
```

Windows 需要 Visual Studio C++ Build Tools，macOS 需要 Xcode Command Line Tools。中文优先加载系统字体：微软雅黑、PingFang、Noto CJK 或文泉驿。

Windows 也可运行 `./dev.ps1` 启动；脚本能找到用户目录下的 Cargo，无须修改系统 PATH。其他 Cargo 参数可直接跟在脚本后，例如 `./dev.ps1 test --workspace --locked`。构建临时文件放在所选 `CARGO_TARGET_DIR`（默认工程 `target`）下的 `build-temp`，退出脚本后恢复调用者的环境变量。

## 当前可用

- 连接与分组：创建、编辑、复制连接、终端复制会话、树内重命名、分组内新建、搜索、Ctrl/Shift 多选、批量子树删除、删除目录时保留子节点、逐项导入选择及重复项分析、目录与隧道引用 ID 重映射。保存失败保留原模型及草稿。
- 导入旧版 `TOMATO_ENCRYPTED` AES/ECB 配置与明文 JSON，保留旧版扩展字段。
- MySQL / PostgreSQL / SQLite：复用会话、多语句、事务、表列表、CSV / JSON 结果导出。
- 表工作台：schema/表树、分页、结构查看、列排序；行修改先暂存后保存，支持取消、TSV 导入、复制 INSERT/JSON/文本。SQLite、MySQL/PostgreSQL 隔离服务回归已通过；CI 结果以实际工作流为准。
- Redis：JSON 参数数组执行命令，可使用 ACL 用户名和 TLS。
- 原生 SSH：密码、私钥、多因素交互认证、主机密钥记录、引用式跳板；终端内 rz/sz 流式传输，右下角 Docker/端口/文件/监控工具。连接树提供终端、容器、服务、端口、存储、文件子项及容器操作。
- 原生 SFTP：浏览、批量上传下载、递归目录、重命名和删除；独立文件页有图标/列表/多列视图。双击文件通过默认应用打开，修改后自动回传。终端侧栏可选择跟随 OSC 7 目录。
- PTY 本地终端、ANSI 单元格颜色、双击选词/三击选行、复制粘贴、VT 状态响应和滚动历史。
- 本地/S3 目录树按需展开、折叠、刷新及原文件图标；新建目录/Markdown、改名、删除和拖放。Markdown 提供编辑/分栏/预览、图片、原工具栏、独立查找替换、HTML 导出和未保存关闭确认；S3 文档直接经对象存储读写，失败保留草稿。PDF 等差距见迁移清单。
- JSON 格式化与编码、SVG / 位图转换、图标背景透明化、VOC → COCO、Linux 快捷方式、Hosts 编辑、HTTP 下载、TCP 转发、颜色转置游戏。
- 原生 HTTP/FTP 文件服务：沿用旧账号、共享目录与只读权限，支持配置热更新；标签关闭时释放服务与活动连接。FTP/S3/OSS 和本地目录共用三种文件视图及后台操作 actor。
- RDP 已接入 IronRDP 应用内 TLS / CredSSP 会话、画面与键鼠；Windows 原生剪贴板已接线，独立窗口/标签和全屏已接通；局部图像更新、TCP_NODELAY 和本地光标绘制已接入。音频及真实 Windows 桌面完整互通验收尚未完成。VNC 已接入原生 RFB 3.3/3.7/3.8、DES 密码认证、Raw/Hextile 图像、内嵌缩放、键鼠和剪贴板，真实服务器及完整视觉仍待验收。SPICE 与原版一样暂不可用；ADB 已接入标签内原生 H.264 / GPU I420 连续视频、实时触摸拖动、画质切换和 PNG 兼容回退；跨窗口共享会话仍待迁移，性能验证范围见 [ADB 视频](docs/adb-video.md)。

Windows 主界面、树、表格、终端和对话框已统一调整字体像素对齐，保持字号；旧连接文件直接读取。图标工具接入真实预览/裁切/保存，快捷方式接入完整字段与保存，HTTP 下载器接入并发范围请求、取消/重试、恢复、历史和原生对话框。BT/eD2k 及其余差距继续按 [迁移清单](docs/migration.md) 标记，尚不声明全量迁移或逐像素一致。

主窗口按原版恢复连接、工具、服务器、开发、设置五个入口；连接页初始内容区为空白。右键连接列表新建连接，或在工具侧栏打开 JSON 等工具。连接列表双击打开，右键编辑、复制或删除。数据库连接类型中增加 SQLite，可使用 `:memory:` 无服务器体验查询。

打开数据库连接后，在左侧展开数据库/schema，双击表名打开数据标签，右键表名查看结构。首次单击选中，再次点击已选单元格、双击或 F2 在单元格内编辑；Enter 或点击其他单元格/工具栏暂存，Esc 取消当前输入；底部添加、删除、保存、取消修改、刷新按钮操作暂存区，翻页前会检查未保存内容。删除需要主键，无主键表仅支持安全单行更新。导入接受 UTF-8 TSV/TXT，无表头，空自增字段使用默认值、生成列忽略，二进制列使用十六进制；单批最多 1,000 行。结构设计器已接入字段/索引编辑、SQL 预览、数据库保存及相关标签刷新；复制表、跨连接数据传输和 PostgreSQL nb3 备份还原已接入真实流程，已通过隔离数据库回归。完整差距见迁移清单。

## 配置和认证

从 0.1.1 起，默认直接使用原版 `~/.tomato/connections.json`（Windows 为 `%USERPROFILE%\.tomato\connections.json`），旧连接无需导入。可以通过 `TOMATO_DATA_DIR` 指定隔离目录，此时使用该目录内的 `connections.json`；CLI 也支持 `--config <文件>`。

连接文件沿用原版 `TOMATO_ENCRYPTED` + AES/ECB 格式，保留原 ID、分组、隧道引用、已保存密码和未知字段。启动只读，首次修改前备份为 `connections.json.before-rust.bak`；保存前检测外部修改，检测到冲突会拒绝覆盖。导出仍默认移除敏感字段，并禁止导出覆盖当前配置。详见 [共享配置说明](docs/configuration.md)。未实现的协议仍按迁移清单标记，读取配置不代表已经实现对应协议。

SSH、RDP 与 VNC 不需要系统协议客户端，密码不进入子进程参数。SFTP 使用 russh-sftp；ADB 的内嵌兼容模式需要 Android platform-tools 的 adb，可直接使用旧配置的 adb 程序路径。原生 RDP 已接入 RDPSND 声音和 Windows 按需文件剪贴板/跨会话转发；协议与静音设备测试通过，Explorer 实际粘贴和真实远端声音听感尚未完成验收。

## CLI

```sh
tomato-cli connections
tomato-cli check-config
tomato-cli import /path/to/old/connections.json
tomato-cli export /path/to/export.json
tomato-cli sql <connection-id> --query 'SELECT 1'
tomato-cli sql <connection-id> --file query.sql
tomato-cli json format input.json
tomato-cli convert-image input.svg output.png --width 512
tomato-cli convert-dataset annotations coco_annotations.json
tomato-cli ls /path/to/directory
tomato-cli catalog
```

数据库密码可由 `TOMATO_PASSWORD` 提供。`tomato-cli run` 从标准输入接受一个与界面相同的 Command：

```json
{"command":"sql","params":{"connection":{"id":"demo","name":"SQLite","type":"SQLITE","database":":memory:"},"sql":"SELECT 1 AS example;"}}
```

## 检查和打包

完整测试需要 JDK 17 或以上（JAVA_HOME 或 PATH）：仅用于运行保留的原版 Java ZMODEM 互通夹具，发行程序运行不依赖 Java。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo xtask package
cargo xtask package --profile dev
```

`dist/` 生成 ZIP、tar.gz 和 SHA-256 校验文件，包含桌面程序、CLI 和文档。macOS 额外包含 `.app`。可使用 `cargo xtask package --target <triple>` 指定已安装工具链的目标。Windows 可用 `--installer` 额外生成安装程序，首次运行会下载并校验固定版本的 NSIS。

CI 在 Windows、Linux、macOS 执行格式、Clippy、测试和无界面引擎自检。`Release` 打包流程覆盖 Windows x64、Linux x64/ARM64、macOS Intel/Apple Silicon。推送与 workspace 版本一致的 `v*` 标签，五个平台打包成功后会创建草稿。

在 GitHub 的 **Actions → Publish Release → Run workflow** 输入与 `Cargo.toml` 工作区一致的版本号，例如 `v1.0.0`。升级版本后用 `cargo update --workspace` 同步锁文件，并一起提交 `Cargo.toml` 和 `Cargo.lock`。默认公开发布，选择 `draft` 可先创建草稿，`verify` 可只校验产物。流程优先复用默认分支上同版本且未过期的打包产物。需要时自动打包五个平台，再校验压缩包、Windows 安装程序并生成 `SHA256SUMS.txt`。

当前发行格式包括便携包和 Windows 安装程序。签名、公证、自动更新和 BSD/Web 构建尚未迁移。参考仓库的发布工作流依赖组织共享工作流和签名 secrets，这里改成独立构建与打包。

发布说明的「发布与下载」指向该版本的 GitHub Release 页面。下载包按系统和架构命名，例如 `tomato-1.0.0-linux-x64.tar.gz`，`SHA256SUMS.txt` 使用相同的文件名。

macOS 便携包随附 unixODBC 运行库。解压后可打开 `Tomato.app`，也可在解压目录内运行 `tomato` 和 `tomato-cli`。使用 ODBC 导入仍需有对应数据源的驱动。

详见 [架构与设计模式](docs/architecture.md)、[源码迁移清单](docs/migration.md)、[远程协议与验收说明](docs/remote-protocols.md) 和 [内存测量说明](docs/performance.md)。


## 界面与内存

界面以旧项目 FXML/CSS 和原始图标为依据还原，尚未完成所有页面的视觉与交互对等。具体差距见迁移清单。原版图标许可见 LICENSE-TOMATO-ASSETS。

界面采用事件驱动刷新、连接树可见行绘制、按需图标解码（最多 64 项缓存）。终端历史默认 1,000 行，可从原版 global.json 调整；JSON 历史最多 20 条且总计 256 KiB；数据库默认保留 1,000 行、最多 10,000 行，跨结果集保留数据最多 8 MiB。可用连接字段 queryRowLimit 调整行数。后台运行时使用 2 个工作线程、最多 4 个阻塞线程；wgpu 优先节省内存，最多排队一帧。

使用 packaging/measure-memory.ps1 -Executable target/release/tomato.exe -Output target/memory-release.json 测量隔离空工作区空闲内存。报告区分工作集和私有提交量；实际占用受图形驱动、字体、窗口尺寸和打开的会话影响，不能将结果数据上限当作整个进程内存上限。




Oracle 原生接入的开发验证副本：`F:/data/git/rust/tomato/target/qa-candidate-oracle-448/tomato.exe`。用户树、查询、表数据和设计器已接到真实后台；463 项默认回归、6 项驱动边界测试及共用数据库的真实工作流通过。此副本不是优化发布版，Oracle 真实服务器验收、传输/备份等仍有缺口；完整边界见 [Oracle 接入](docs/database-oracle.md)。
