# RDP 窗口模式与局部更新

2026-10-19 生产源码 255 / 成品验证 259–262：本轮最终程序的 TAB、WINDOW 在 Vulkan 通过真实回环帧捕获；FULLSCREEN 在 DX12 通过。当前桌面会话 Vulkan 全屏创建表面失败（`Surface::configure Invalid surface`），同轮旧 225 成品也复现，不能记作三种模式均在 Vulkan 通过。记录见 `target/window-probes/rdp-release-259/validation.json`；保留失败日志，不自动改变用户后端设置。

2026-10-19 生产源码 225 / 成品验证 229：直连 TCP_NODELAY、本地光标形状绘制、相邻纯移动合并和当前帧输入推进已接通；保留按键、按钮和滚轮顺序。优化程序的 TAB/WINDOW/FULLSCREEN 复验通过，见 target/window-probes/tools-release-final-229-rdp/validation.json。无编译负载回环 32 次鼠标移动 p95 16.2178ms，见 target/rdp-input-quiet-229.log；不是用户主机延迟。最终成品和空闲内存记录见 docs/performance.md。

2026-10-19 工作区修改；默认检查见 target/rdp-{fmt,clippy,tests,docs,build,selfcheck}-199.log，352 项默认测试通过，46 项默认忽略。原生窗口验收见 target/window-probes/rdp-197/validation.json，TAB / WINDOW / FULLSCREEN 均已捕获真实本机 RDP 帧；旧 dist 未更新。

原始行为依据：`D:/Data/Git/tangluobo/tomata/src/main/java/com/tangluobo/tomato/module/connect/handler/RdpConnectHandler.java`、`dialog/SessionConfigDialog.java`、`GlobalConfig.java`，以及 `D:/Data/Git/tangluobo/rdp4j/src/main/java/com/tangluobo/rdp4j/RdpPane.java`。

`rdpOpenMode` 继续使用原字段，值 `WINDOW` / `TAB`。连接未指定或 Java blank 时继承 global.json；全局默认 WINDOW。系统设置提供全局选择；RDP 标签右键的“会话配置”提供连接覆盖和“使用全局配置”。保存继续经过原加密配置 Repository 和外部修改检测，只写三个编辑字段，未知字段保持不变。

独立窗口是 egui 原生 viewport，使用原 mstsc 图标、有独立任务栏入口，同连接同模式复用窗口；标签与独立窗口可并存。关闭独立窗口释放该会话。标签进入全屏再退出保留原 Tab 和会话；独立窗口最大化进入全屏，退出恢复窗口。全屏按比例显示并隐藏状态栏和滚动条；初始独立窗口滚动条隐藏，手动缩放后启用。顶部栏沿用原 SVG 路径、五秒隐藏、固定、横向拖动、连接质量提示、最小化、退出、关闭。质量提示与 Java 一样是固定文案，并非网络质量测量。

旧 IronRDP client 每个 GraphicsUpdate 均将整张 DecodedImage 转成新 buffer，服务和 UI 再复制整屏、上传整张纹理。现在在协议解码端保留 dirty rectangle，每 16ms 合并更新；服务保存一张 canvas 和一个待呈现区域，延迟读取保留所有区域；egui 对已有纹理使用 set_partial。解码循环约每 4ms 主动让出执行，单个 PDU 的耗时无法被此切片中断。Windows WinClipboard 仍在 GUI 线程处理，未将其跨线程移动。远端尺寸在分配前限制到 16,777,216 像素。

本机隔离 TLS/CredSSP 测试 `target/rdp-stream-tests-189.log`：5 项通过，1 项窗口捕获需单独运行。1920×1080 的 540 个两行更新产生 11 次界面快照，累计 2,073,600 像素，415.8ms；最大单次 GUI poll 9.0ms；校验每行每个像素及并发鼠标输入，正常断开。此数字来自测试机 debug 协议夹具，不是用户主机帧率或进程 RAM 测量。

`vendor/ironrdp-client/TOMATO-PATCH.md` 记录补丁来源。Cargo.lock 中 gpu-allocator 0.28 的 windows 绑定使用其允许范围内的 0.62.2，与 wgpu-hal 对齐；clipboard-rs 的 0.59 绑定独立保留，不全局替换。构建使用 --locked。

真实用户连接未在自动化中连接；原版与新版窗口全状态逐像素一致及实际主机体验不能由协议夹具单独证明。


补充回归记录：target/rdp-protocol-197.log 的 1080p 横条突发在并行 release 编译负载下产生 18 次快照，累计仍为 2,073,600 像素，约739ms，最大 GUI poll 约9.5ms，并发鼠标输入正常。195 首次 WINDOW 采图停留在连接中，保留失败记录；196 起诊断截图等待首帧而不是固定延时，普通连接流程不变。192/193 的工具页回归发现独立窗口过滤条件误伤工具内容，修正后 195/196 全套默认测试与两轮真实数据库原生流程通过。194 的 FTP loopback 列表超时单独复测及后续完整测试均通过，未改写 FTP 实现或提高其测试时限。


199 窗口生命周期补充：已打开窗口进入全屏时重新启动初始五秒可见计时；退出恢复窗口时重置固定、横向偏移和质量提示状态；全屏/还原/最小化先按顺序排队释放远端按键和鼠标，防止跨视图残留按下状态。现有原生窗口生命周期测试覆盖控制栏截止时间、远端 Ctrl/鼠标释放、固定与偏移重置。仅撤回旧 GUI 编译，CLI 编译保留，记录 target/rdp-release-gui-superseded-198.json；新的 GUI 构建日志为 target/rdp-release-build-198.log。不能把旧构建的退出错误当作新构建成功。


最终 release 验收完成：target/window-probes/rdp-release-final-199/validation.json，含最终 GUI 的三种模式图像和 CLI 检查；内存空闲配对结果详见 docs/performance.md。GUI SHA-256 9484B79888057AC8E9C485F067390FBB8427FAD45613CD295D1212ADA243F646。
