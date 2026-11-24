# RDP 文件剪贴板初始化与恢复（1670，2026-11-01）

复现并修复“连接前剪贴板已经有文件，连接后文件一直不能粘贴”：原文件通告要求通道已 Ready，但文件本身就是本次初始化要发送的首批格式列表，导致等待循环。修复前专门回归报 `initiate_file_copy: clipboard channel is not in Ready state`。现在文本和文件共用初始化报文构造，发送能力、临时目录和实际文件格式列表，再由服务端确认完成握手；继续检查文件流能力、文件名、路径、锁定代次和请求大小。

初始化与 [Microsoft MS-RDPECLIP 初始化顺序](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpeclip/a5cae3c9-170c-4154-992d-9ac8a149cc7e) 对齐，并只读对照原 Java FixedClipChannel 的格式通告与 WindowsVirtualFileClipboard 的原生文件发布。文件已删除、元数据读取失败或文件工作队列繁忙时，通告空列表结束本次初始化并释放当前未锁定列表；剪贴板已改变时不再发布旧读取错误。随后正常复制可以继续，已有锁定快照的取消与失效保护保留。

819 项工作区默认测试通过、0 失败、90 项条件忽略；格式、严格工作区 Clippy、CLI 自检和旧配置只读检查通过。另外 7 项文件剪贴板专项通过；标记忽略的 Windows 子测试由父测试显式启动并执行成功。真实 TCP 协议回归覆盖初始文件通告、分块收发和远端之间转发；另检验会话关闭、重建工厂与迟到响应的资源释放和快照保护。Windows 私有窗口站覆盖原生延迟 CF_HDROP、FileGroupDescriptorW、OLE IStream 接收并逐字节核对 1,048,659 字节、多文件、中文名、空文件、空目录以及失效文件列表之后再次复制。检查不读写用户的系统剪贴板。新增原生父测试已接入 Windows CI，使用 `scripts/qa/rdp-clipboard.ps1` 在独立清单中运行，避免改写根或 vendor 的锁文件；同一 CI 入口已在本机验证，未声称 GitHub 上已经执行。

这些文件收发证据来自原生 Windows 与隔离协议检查。同一份优化 EXE 已实际连接保存的“110”，标签与独立窗口均收到 1920×1080 画面并正常退出；现场 Explorer 文件粘贴仍待完成。标签截图出现过远端 UAC 提示，独立窗口截图显示 Windows 桌面与 Explorer；未操作安全提示。本轮没有 node_repl 桌面操作工具；computer-use 技能要求使用该工具进行 Windows UI 操作，因此没有自动执行 Explorer 粘贴或改用自制桌面控制工具。文本、文件共用相同会话通道，标签和独立窗口继续使用原有适配器。旧连接配置与 mapClipboard 开关直接沿用，Java 源码及原配置保持；本次未改界面或输入路由。保留 1668 下载器、1666 表格批量编辑及更早的功能和工具。

没有增加常驻线程、整文件预下载或文件数据缓存，继续按需读取、每次最多 1 MiB。空闲发布版工作集和私有提交分别测量；活动文件复制的峰值不由空闲数据推断。

发布源码冻结 `rdp-file-copy-1670-final`，EXE SHA-256 `2CE4C6FBE46876A7E32B72E3DC2F31C5ED64BD49E52F3BE7F782879B7A14B5A5`。
