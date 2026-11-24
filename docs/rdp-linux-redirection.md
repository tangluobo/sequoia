# Linux RDP 重定向与密码重试

用户指定“小主机桌面”进行实际排查。旧版能完成初始认证，但收到 Enhanced Security Server Redirection PDU（Share Control 类型 0x0A）后，未实现的解码分支报错并终止会话。独立窗口按用户要求隐藏了底部状态栏，失败原因没有显示在认证框内，因此表现为不断重新输入密码。

修复参考只读 Java `rdp4j` 的 `Rdp.processServerRedirection`、`RdpClient` 重连及 `RdstlsClient`，并对照 [Server Redirection Packet](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/df3d59e6-30a8-4a36-bd2d-9d11bcd96c3e) 与 [RDSTLS Connection Sequence](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/ef2979ac-5c21-4cdf-beda-1ded533ff4da)。

客户端先识别主 MCS 通道上的重定向消息，读取服务端给出的会话 ID、路由令牌、目标和一次性凭据，释放旧传输后重新建立连接。带路由令牌时继续连接原入口；没有令牌时按 FQDN、地址、NetBIOS 或地址列表选择目标。重定向最多六次，单个报文及字段不超过 65535 字节，UTF-16、长度、版本和地址数均有校验。信息性 NOREDIRECT 消息不发起重连。

RDSTLS 的加密密码保持字节不变，不转换成字符串；能力交换、认证请求和响应均在 TLS 内进行，并保留允许的 TLS 分支。Client Info 中同样保留原始加密凭据字节和终止符。请求只保存在当前会话；强制重新连接的尺寸变更会回到原入口获取新凭据，避免重放过期的一次性密码。新增状态只在实际发生重定向时分配，普通 Windows 会话不复制额外配置。

连接失败框现在显示具体原因并提供手动重连，Enter 自动重复不会再次提交。未勾选保存的 RDP 临时密码保留在当前标签/独立窗口的会话配置里，供后续重试使用；保存仍经过已有的加密 Repository 和外部修改检测。Connector 配置中的密码、智能卡 PIN 和私钥调试输出脱敏，重定向请求不记录一次性凭据或路由令牌。

实际连接的修复前证据为 `target/rdp-linux-before-968.json`；首次修复成功为 `target/rdp-linux-rdstls-973.json`，约 2103ms 收到 1920×1080 画面且共享配置字节不变。

成品为 `target/qa-candidate-rdp-linux-983/tomato.exe`，默认 release 优化构建，SHA256 `80FFEA7CC540886B3664BD4161B36CD5E5466C4408BC0E6E0A9ECC0D9FA9B4D9`，98,658,816 字节。与冻结的 2,167 个生产源文件核对一致，PE 图标的所有帧均与原资源逐字节相同；未覆盖旧 EXE。494 项默认测试通过、73 项条件测试忽略，另有四项协议向量通过，格式、严格 Clippy、全目标构建和 CLI 自检通过。

同一份成品使用默认旧配置实际连接“小主机桌面”，TAB 和 WINDOW 均显示 1920×1080 的 AlmaLinux 图形登录界面，画面已逐张检查，未停在 Tomato 密码框。关闭标记至正常退出分别为 211ms 和 217ms，四份共享配置及旧程序哈希不变。证据 `target/window-probes/rdp-linux-release-native-985/validation.json`，截图位于同目录 `TAB.png` 和 `WINDOW.png`；开发构建也通过了相同检查。当前 Windows 会话虽标记 Disc，此次默认 Vulkan 窗口仍能正常捕获，因此本轮两种模式的画面和正常退出已经验收。

验收止于远端系统登录界面，没有代用户选择 Linux 账户、完成图形用户登录或注入操作系统输入；不据此宣称物理鼠标流畅度或完整桌面登录通过。内存采用前版 960 与本版的独立配置配对测量，结果见 [性能记录](performance.md)。

普通工作区回归不连接保存的主机。实际主机检查只在用户已指定的连接 ID、类型和明确启用的验收环境变量同时匹配时运行；两个保存主机开关同时开启时不选择主机。协议向量覆盖路由与凭据、二进制 RDSTLS 密码、拒绝响应、缺失字段、错误字符串及 Client Info 原始字节。界面回归覆盖两种模式的临时密码重试、共享配置不变、隐藏状态栏时的错误显示及 Enter 重复事件。
