<!-- current-migration-1672 -->

当前状态以 migration.md 和 migration-backlog.json 为准：RDP 已接入原生 Standard Security、剪贴板文件通道、音频、独立窗口和标签等后续能力，早期“未迁移”描述不代表当前代码。实际主机文件粘贴、声音及完整视觉/流畅度验收仍按各自证据区分。
# 当前迁移状态（1672，2026-11-01）

以下早期版本记录保留作为历史，不能把其中旧的“尚未迁移”说明当作当前功能状态。
当前 RDP 已有标签/独立窗口、声音、原生文件剪贴板、SSH 跳板以及旧 Standard Security，文件复制和退出专项已在隔离 Windows/协议环境验证。
Standard Security 用独立 FreeRDP 3.32.2 合成服务逐项检查实际 GCC 协商值：40/56/128 位、Low/High/无加密，共 12 次连接，每次超过 4100 个输入包并跨过 4096 次密钥更新；旧 SSL 配置不写回。TLS/CredSSP 和 SSH 回归通过。
FIPS 沿用原 Java 的不支持边界，不静默换用其他安全层。
ADB 的同设备共享传输、解码及画质设置已接入，正在验证多视图触摸、关闭与清理；高码率和 GPU I420 绘制继续保留。
保存主机 110 的 Explorer 现场双向文件粘贴尚未完成，不由隔离协议结果推断；整体 Java 像素一致也尚未全部验收。
准确的逐项状态见 [迁移清单](migration-backlog.json)，最新发布证据见 [迁移记录](migration.md)。

---

# 原生远程协议

原始连接文件仍由 `ConnectionFileRepository` 读取。协议适配器接收内存中的连接副本，不产生带密码的命令行参数，不修改旧连接字段。

## SSH

`services/terminal/ssh.rs` 使用 russh，`terminal.rs` 作为本地 PTY / SSH 的统一适配器。后台任务持有 TCP、认证和 channel，界面通过有界消息队列输入；尺寸变化采用 watch 通道，只保留最新值。关闭标签中止任务，socket guard 同时 shutdown 底层连接，避免只丢弃 russh handle 后残留网络任务。

支持保存的密码、`privateKeyPaths`、键盘交互认证。首次看到的主机密钥写入连接配置同目录的 `known_hosts`，以后变更拒绝连接。该记录独立于系统 OpenSSH 的 known_hosts。PTY 与 shell 请求均等待服务端确认后才进入 Ready；确认前收到的欢迎信息和提示符仍通过有界输出通道保留，测试服务故意先发送输出再确认 shell。

终端界面直接绘制 vt100 单元格，采用原版 xterm 调色板。Windows egui Copy 事件在没有选区、且 Ctrl+C 时转换成终端中断；Ctrl+Shift+C / 有选区时复制。

0.1.4 增加原生引用式跳板链、SFTP、rz/sz 及管理子页。跳板按原配置 sshTunnelHostId 解析，每层单独认证和核对主机密钥，限制 16 层并拒绝循环。最外层 socket guard 与各层 handle 随会话释放。

管理操作采用类型化 Operation / Reply，shell、SFTP 子系统和 exec 通道共用认证后的 SSH 会话。请求/回复各最多 8 项，最多 4 个后台操作；终端输出独立，管理结果不会进入 VT 解析器。视图按文件、监控、端口、Docker 排列；树中还有独立终端、存储和文件页，容器支持启停、日志、进入 shell、属性、重启策略和资源限制。存储写操作显示实际命令并确认。

SFTP 按需创建，目录/文件 I/O 在后台执行，文件按 64 KiB 流式传输，SFTP 读写并发各最多 4 项；目录分页读取最多 20,000 项，超限关闭远端句柄并返回错误；本地下载先写临时文件，完整接收后替换。目录递归限制深度和条目数，不跟随链接；文本编辑限制 8 MiB。外部编辑缓存最多 16 项，1 秒检查并等待两个稳定采样再回传，关闭会话终止检查。侧栏保持紧凑列表和可关闭的目录跟随；独立页提供图标、列表和多列浏览。

rz/sz 使用 caller-driven ZMODEM 状态机；协议流先于 VT 分流，缓冲有界，不把整个文件放入内存。输入 rz 后预选上传文件，远端握手也能触发选择；支持多文件、上传续传位置、取消及传输后恢复 shell。等待选择缓冲为 64 KiB，选择/传输等待分别有超时，下载先用临时文件。ZMODEM 位置字段为 32 位，单文件大于 4 GiB 使用 SFTP。

本地 vendor/zmodem2 保留 0.7.3 源码和许可证，补充发送编码协商和接收子包长度兼容：对端未声明 CANFC32 时使用 CRC16，否则保持 CRC32。原版 Java 接收器不声明 CRC32，其 CRC32 序列化也与标准字节序不同，不能固定强发 CRC32。原版 Java sender 的元数据使用 US-ASCII，原版互通测试据此采用 ASCII 下载文件名；UTF-8 下载另用独立 CRC32 对端验证。原始 34 个 Java 协议文件保持原样，仅测试 stdio 入口模拟远端启动公告；它们不会进入发行程序。

Windows CF_HDROP 文件剪贴板、拖拽上传、Ctrl/Shift 多选及 F2 内联重命名已接线，仍需实际桌面手势与完整视觉对等验收；原版 TerminalView 本身也没有终端鼠标报告，不能将其列为已经迁移的原版行为。不能将工具入口的存在等同于全部行为完成。

## RDP

采用 [IronRDP](https://github.com/Devolutions/IronRDP) 的 `ironrdp-client` 和 PDU 类型。`services/remote_desktop.rs` 隔离协议；`ui-egui/panels/desktop` 处理图像纹理、坐标变换及输入；`tabs/desktop.rs` 保存界面状态。界面还参照原版 rdp4j 的 `RdpPane.java`（仓库 HEAD `9cbd5cbd7f01e4317ad2e3e6119a70ba388cc94a`，文件 SHA-256 `1024eaac8342be1b4f40eed2c1df5980581785585b3f0690116117d2b8ea35b5`），采用自然像素滚动、最近邻采样和底部状态栏。保留原版 `domain`、`screenWidth`、`screenHeight`、`colorDepth`、`useSsl`、`mapClipboard` 字段。原 24 位色映射到协议引擎的 32 位 RGB 帧；支持 16 位协商。

Windows CLIPRDR 持有窗口句柄，必须留在创建它的 GUI 线程。因此会话 future 由应用 logic 阶段轮询，Tokio handle 提供异步网络与定时器，waker 请求界面重绘；不使用忙轮询或每帧固定刷新。未选中的标签也继续驱动协议。关闭标签直接释放 future、socket、剪贴板窗口与纹理。

图像输出通道容量为 1，共享状态覆盖旧帧，界面通过 take 取得最新帧。协议输入通道容量为 64 项，长文本在界面分批发送；发送队列繁忙时保留余下输入，避免丢失粘贴尾部或 key-up。

当前 Windows 剪贴板仅完成接线，尚未通过真实桌面互通验收；其他平台的上游默认后端为 stub。当前引擎使用 TLS；旧配置 `useSsl=false` 会得到明确的未迁移提示，不能作为已支持无 TLS 服务器。音频、独立窗口、全屏快捷键、自定义远端光标及完整的配置界面仍待迁移。

## 依赖兼容

采用 russh 0.64.1、SSPI 0.23 和相应的加密组件。IronRDP 连接器 0.10 原先固定旧版 CredSSP 依赖，导致与当前 SSH 的依赖冲突；本地仅对连接器做最小兼容补丁，更新两个依赖约束、传播新的长度检查错误，并移除过时的 lint expectation。测试用 acceptor 对同一长度 API 做对应适配。补丁出处及改动见 `vendor/ironrdp-connector/PATCH.md` 和 `vendor/ironrdp-acceptor/PATCH.md`，许可证保留在 vendor 和 `docs/third-party`。没有修改加密算法。旧版 russh 0.55 存在已知分配问题，不作为兼容方案。Rustls provider 显式选择 ring，避免不同协议分别启用 ring / aws-lc 后出现默认 provider 歧义。

用户提供的 RustConn（GPL-3.0）仅用于了解协议组件选型，没有复制其实现。协议库使用各自许可；[sshx](https://github.com/ekzhang/sshx) 的共享终端架构与 [RustCrypto/SSH](https://github.com/RustCrypto/SSH) 的基础组件也作为参考，未引入原版不需要的远程共享服务。

## 隔离验收范围

`tests/ssh.rs` 的临时 russh 服务覆盖密码、私钥、加密私钥提示、缺失密钥继续尝试、键盘交互、多因素认证、免认证、主机密钥变更、shell 拒绝、中文输出、终端尺寸和关闭连接。

`tests/remote_desktop.rs` 及 `support/rdp_fixture.rs` 使用临时证书、固定测试账号和随机回环端口，覆盖 TLS / CredSSP、无压缩画面的逐像素精确值、RemoteFX 每像素的色差容限、扫描码 / Unicode / 鼠标 / 滚轮、握手错误和会话关闭。不读取或连接用户保存的主机。

`rdp_fixture::capture_native_rdp_window` 默认忽略，需要提供 `TOMATO_RDP_CAPTURE_EXE` 和一个尚不存在的 `TOMATO_RDP_CAPTURE_PATH`。它启动独立 GUI 子进程及隔离配置，检查实际窗口截图中存在原生远端帧；只释放自身创建的进程。这项截图检查不能替代 Windows Server、真实剪贴板和原版界面的对照验收。

2026-10-15 的 0.1.3 本机验收已通过上述 8 项 SSH、4 项 RDP 自动测试及额外手动执行的便携包窗口测试。完整工作区为 51 项通过、默认忽略 3 项；其中窗口项已单独执行，MySQL/PostgreSQL 两项留待 CI。格式、严格 Clippy 和最终 release / 包内 CLI 自检通过。当前锁定依赖组合为 russh 0.64.1、SSPI 0.23 与本地 IronRDP 最小兼容补丁。

0.1.4 新增 `tests/ssh_workspace.rs` 的 8 项回环测试：管理 exec 与 shell 分流、SFTP 文件/目录传输和外部编辑自动回传、引用式 SSH 跳板、原版 Java 接收/发送协议互通、CRC32 与 UTF-8 下载、取消后继续使用 shell、20,001 项目录拒绝后继续操作。上传包含续传和全字节值，下载包含空文件；每项协议传输结束后均验证 shell 恢复。原版 Java 接收器的测试文件适配器使用无缓冲写入，原因和空文件跳过限制见夹具 README。

0.1.4 最终源码已通过格式、严格 Clippy 和 68 项工作区测试。隔离 SSH 原生窗口、离屏渲染及原版 JavaFX 控件截图已单独运行；桌面恢复前的 Surface 失败已通过旧包复现，恢复后新版原生窗口成功。完整外观和真实远端 Docker/LVM、桌面文件剪贴板操作仍需逐项验收。

## SSH 原源码与 Rust 对应

| 原代码入口 | Rust 实现 | 已接入行为 |
| --- | --- | --- |
| SSHTerminalPane / TerminalView | services/terminal/ssh + ui-egui/panels/terminal | 认证、PTY、尺寸、VT、选择/复制/粘贴、重连、ZMODEM、右下角工具切换 |
| SFTPFileBrowser / SFTPClient | ssh/tools/sftp + tools/editor + terminal/tools/files | 列表/图标/多列、目录传输、多选、重命名、删除、外部编辑与自动回传、Windows 文件剪贴板和拖拽入口 |
| DockerPanel | ssh/tools/management + terminal/tools/table / inspect | 启停/重启/删除、日志、容器终端、属性、重启策略、CPU/内存限制 |
| PortPanel | ssh/tools/management + terminal/tools/table | TCP/UDP 筛选、ss/netstat 解析、防火墙开放标记、结束选中 PID |
| MonitorPanel | tabs/ssh + terminal/tools/monitor | CPU/内存/网络/磁盘采样、60 点历史、平滑、面板高度拖动 |
| 存储树及属性动作 | ssh/tools/management + terminal/tools/storage / dialogs | lsblk/LVM 树、PV 初始化、VG 创建/扩展、LV 扩展、格式化与挂载，执行前展示实际命令 |
| SSH 连接树 | app/ssh_tree + app/connections | 终端、容器、服务、端口、存储、文件子项；独立终端、工具标签去重、容器右键操作 |

路径均相对于 crates/services/src 或 crates/ui-egui/src。网络与文件 I/O 在 services 的 actor 和适配器中，绘制函数只提交类型化 Operation；结果按请求 ID 回到所属标签。

尚未达到原版全部行为一致：SSH/SFTP 表头排序与完整菜单/字体细节、链接文件的传输语义、所有标签关闭后的纯后台容器刷新仍有差异。目录递归目前明确不跟随符号链接；本地下载原子落盘，远端覆盖上传不保证原子替换。管理命令的隔离测试不能替代真实 Docker/systemd/LVM 环境验证。


## 原生 VNC

VNC 按原 VncClient/VncPane 及 RFC 6143 接入同一 DesktopSession，已实现 RFB 3.3/3.7/3.8、共享桌面、None/VNC Password、32-bit little-endian true colour、Raw、Hextile 和 DesktopSize。密码字符转 ISO-8859-1、前 8 字节逐位翻转后进行 DES/ECB；两组测试向量由原 Java 实现独立生成，包括非 Latin-1 替换。3.7 None 按协议直接进入 ClientInit，避免等待不存在的 SecurityResult。

输入按 X11 keysym 编码，处理 UTF-16 代理对和滚轮按下/释放；剪贴板遵循旧 mapClipboard 开关和原 Latin-1 语义，在 Ctrl+V 前先发送文本。背景任务跨输入事件保留正在读取的矩形 future，避免 select 取消 read_exact 导致分段数据流错位。关闭标签同时丢弃带 abort guard 的 worker，未执行 GUI poll 也能释放 socket。

8 项回归已通过，覆盖版本协商、原版 DES 向量、Raw/Hextile/尺寸变化、分段读取时输入、剪贴板、错误密码/缺失密码、越界拒绝及连接建立中的关闭。原版与新版窗口对照使用被忽略的 vnc_original_and_native_window_capture 和同一随机回环服务端；本机已单独运行通过，并检查 640×400 合成桌面的原版与 Rust 原生窗口截图。声音暂使用窗口注意提示，SSH 跳板尚未接入；不能据协议夹具宣称全部 VNC 交互已经完全一致。

0.1.5 进一步按原版补齐同一 TCP 连接内的密码等待/提交/取消；不会因弹窗等待截止而重连。VNC 可打印键匹配 OS 文本与物理键生命周期，保留大小写/键盘布局、按下/释放和长按，抑制重复的客户端 typematic；IME 提交继续走 Unicode 路径，失焦释放已按下的键和鼠标按钮，切换标签或模块也会发送释放。剪贴板恢复原版 16 MiB Latin-1 上限，用 64 KiB 缓冲流式编码，并保留事务直到 Ctrl+V 释放；传输中的后续粘贴明确返回忙碌，不静默覆盖先前内容。读写 actor 分离并各自由 RAII 中止，大剪贴板传输不会阻止读取画面。长按/Ctrl 状态转换、单连接认证、失焦释放和 16 MiB 背压场景均已通过回归。声音、原生系统弹窗与真实服务端兼容性仍需验收。

## ADB 连续视频（2026-10-20）

默认走原版 scrcpy 3.3.4 的 H.264 视频及二进制直接触摸通道，使用旧配置中的 adb 可执行路径和网络端点。原版 server JAR 在构建时嵌入并校验 SHA-256；每个连接分配唯一设备端路径、socket 和 tcp:0 转发。后台 OpenH264 解码参考帧后发布最新 I420，GUI 用三张持久 R8 纹理在 GPU 转色；无 GPU 接收方时保留 RGBA 兼容输出；鼠标按住时实时传输 MOVE。连接时视频服务不兼容会报告原因并回退 PNG，也可以在工具栏主动切换稳定截图。

压缩队列、参数集、视频尺寸和参考帧分配有界；过载等下一关键帧恢复，避免重放积压画面。后台标签停止 RGBA 转换，继续维护视频参考链，重新切入取新帧。关闭等待自己的转发 / 临时包清理，不操作用户 adb 全局服务或其他转发。实现、许可证及验证边界见 [ADB 视频](adb-video.md)。跨窗口共享视频会话仍未迁移；不得把当前默认视频通道标为整个 ADB 模块完全对等。
