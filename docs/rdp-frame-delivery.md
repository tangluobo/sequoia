# RDP 完整帧发布与 AVC420（2026-10-22）

本次针对持续卡顿检查收包、解码与画面发布。此前客户端只声明 V10 AVC_DISABLED/V8，尽管仓库已有 OpenH264 用于 ADB，RDP 实际没有启用 H.264。EGFX 合成器提交完整帧后，客户端还等候传统位图的 4 ms 合并计时器；Windows 调度粒度与 GUI 显示节拍可能叠加等待。

## 实际实现

- RDP 客户端的 `graphics-h264` 特性连接真实的 OpenH264 EGFX 解码器。成品配对发现密集文字场景退化，因此改为显式启用；默认继续原 Java 的 V10 AVC_DISABLED/V8 无损协商，也不创建该解码器。隔离测试通过只读连接选项 `rdpAvc420` 验证实际 AVC420 链路，没有修改用户配置。启用且初始化成功后声明 V8.1 AVC420 与 V8 后备，不声明尚未实现的 AVC444；初始化失败仍提供无损编码。
- AVC420 保留解码参考帧，按协议中的变化矩形合成；颜色使用全范围 BT.709 与运行时 SIMD 转换。限制压缩包为 16 MiB、画面为 16,777,216 像素，不建立额外视频历史队列。
- 合成器已有完整可显示输出时，在写出协议响应后立即发布，包含 ClearCodec/Progressive 路径。传统分片位图/RemoteFX 继续合并，未增加提前 ACK，也不暴露尚未提交的部分帧。保留既有第一次有效 Progressive 预览。
- Progressive 的相邻瓦片在同一表面、顺序连续、精确并集不含空隙时合成矩形，一次输出完整区域。1920×1080 的 510 块瓦片可合为一个缓冲；保留稀疏区域间隙及跨表面绘制顺序，元数据处理保持摊还线性。
- Progressive 颜色重建原本对每个像素做 64 位乘法，以防极端 i16 输入溢出。现在把系数按 65536 拆分，用数学等价且中间结果均不溢出的 32 位公式保留原有舍入和截断，允许编译器向量化；不降低色深、分辨率或颜色精度。完整 Cb 域与亮度/Cr 边界组合共 1,638,400 组同独立 64 位参考公式一致，图形库 255 项回归通过（`target/rdp-codec-tests-progressive-color-715/`）。
- 解压工作缓冲在命令之间保留至多 4 MiB，避免每个 StartFrame/bitmap/EndFrame 都把大缓冲收缩到 16 KiB 再分配。完成的 DVC 分片直接移交 Vec 所有权，移除 drain/collect 的再次分配复制；原长度检查与容量保护保留。缓存策略涉及内存与吞吐权衡，不能直接视作运行内存下降。
- 修复回归中发现的 CPAL/WASAPI 原生崩溃：IMMDeviceEnumerator 从进程全局缓存改为按 COM 线程持有，先于该线程 COM guard 销毁。连续连接不再引用首个已退出线程的 COM 对象。没有关闭声音或跳过并发测试来规避问题。

没有改变旧连接配置、桌面分辨率、窗口设置、缩放或图标。AVC420 为协商后才使用的有损编码；是否启用取决于服务器，不能据新增解码器推定每台主机都在使用它，也不能声称 AVC420 与无损桌面逐像素一致。

## 协议证据与边界

`target/rdp-protocol-cpal-fixed-697.log`：11 项真实 TLS/CredSSP、图形、输入、断线回归通过，1 项依赖桌面的捕获默认忽略。新增测试验证 13 帧 AVC420，包含依赖参考帧、ROI 外背景保留、全范围 BT.709 色样和混合 ClearCodec 更新；色样实际 RGBA 为 `[214,73,29,255]`。这不是编码画质或真实网络流畅度验收。

`target/rdp-audio-lifecycle-697.log`：16 次连续线程退出及 16 个并发线程的真实默认音频设备枚举通过，不播放或录制声音。旧故障地址、崩溃二进制哈希与普通系统/反汇编证据保存在 `target/rdp-cpal-before-695/`；这些不是 REA 工具返回的 Evidence ID。

经本会话既有明确授权，`target/rdp-saved-110-avc-685.log` 对保存连接“110”进行只读连接，收到 1920×1080 图像，配置前后哈希相同。这台主机本次仍选择 ClearCodec/Progressive，而非 AVC420；静态收图不能证明其交互延迟已达标。

原生回环夹具的 adaptive 模式根据真实能力选择 AVC420 或 ClearCodec，以相同亮度场景预编码 16 帧循环，避免实时源编码吞吐限制稳态测量。源日志包含实际编码、发送数量和 ACK 背压间隔。GUI 指标为纹理提交次数，不能等同物理显示帧率或真实网络端到端延迟。开发版测量不能替代优化成品配对验收。

目前桌面会话显示 Disc，全屏创建 GPU 表面出现 Invalid surface；已请求重新连接。TAB/WINDOW 开发版真实 NVIDIA/Vulkan 动态画面捕获通过（`target/window-probes/rdp-avc-dev-motion-698/validation.json`），不据此声明全屏、真实主机流畅度或运行内存改善。

阶段 699 完整工作区默认回归 477 项通过、70 项条件测试忽略、48 组结果，格式、严格 Clippy 与 CLI 自检通过，记录于 `target/rdp-default-validation-702.json`。加入瓦片合并后再次构建和回归，完成后追加当前结果。

## 被拒绝的默认 AVC420 成品（700）

优化成品 `target/qa-candidate-rdp-frames-700/tomato.exe` 的同源 1080p 密集文字场景，在无编译负载的相同 Vulkan/GPU 环境下测得：

| 模式 | 旧 669 / ClearCodec 提交/秒 | 700 / AVC420 提交/秒 |
|---|---:|---:|
| TAB | 23.88 | 15.65 |
| WINDOW | 26.20 | 16.98 |

新版本软件解码退化，未作为最终交付。ABBA 活动样本中工作集约 208.18 → 224.78 MiB，私有提交 311.25 → 326.87 MiB，也未改善。`target/window-probes/rdp-frames-700-{adaptive-old,adaptive-new,memory-adaptive}/` 保留实际编码、源背压和测量证据。直接调用相同优化库的独立分段测量（`target/rdp-codec-profile-707.log`）中，H.264 解码约 29.95 ms/帧、颜色转换约 1.60 ms/帧、EGFX 完整管线约 39.51 ms/帧；后者不包含 TLS、服务层或 GUI。

这组结果用于阻止默认开启退化路径，不能推广为所有 H.264 视频或其他机器的结论。最终成品将继续按相同协议、相同场景比较无损默认路径，并增加整屏 Progressive 瓦片对照。

## Progressive 颜色转换热点（716）

独立优化循环比较了原 64 位公式和拆分后的 32 位公式，524,288 组随机全范围像素完全一致；在有并发编译负载的初次测量中每 64×64 瓦片约 179 / 27 微秒（`target/rdp-color-profile-713.log`）。编译器汇编中原循环含标量乘法、没有 XMM 操作，新循环生成打包乘法与 XMM 操作（`target/rdp-color-profile-713.asm`）。这些是独立循环证据，不能等同整个 RDP 会话加速 6 倍。

为优先纳入这个主要计算热点的修正，中间版 710 编译在最终可执行文件阶段取消，未交付；记录于 `target/rdp-superseded-710.json`。运行源码冻结为 `target/checkpoints/rdp-color-716/`，优化成品与完整应用回归正在验证。瓦片预算回归 66 项通过，原失败测试已改用确实不可合成矩形的区域，保留原预算和拒绝条件（`target/rdp-egfx-unit-710.log`）；12 项原生协议回归通过，包含 510 瓦片整屏 Progressive 的连续变化、无损默认协商与显式 AVC420（`target/rdp-protocol-711.log`，颜色热点修正前的阶段证据）。

## 成品 718 未改善整屏吞吐，继续定位到 RLGR

716 优化构建完成后，`target/qa-candidate-rdp-frames-718/tomato.exe` 的复杂文字源测量显示，局部颜色循环的收益并未转换成会话吞吐：

| 场景 | 模式 | 旧 669 提交/秒 | 718 提交/秒 |
|---|---|---:|---:|
| ClearCodec 密集文字 | TAB | 26.63 | 26.84 |
| Progressive 密集文字 / 510 瓦片 | TAB | 7.37 | 7.26 |
| Progressive 密集文字 / 510 瓦片 | WINDOW | 7.33 | 7.85 |

证据在 `target/window-probes/rdp-frames-718-*`，均为相同源和实际 Vulkan/NVIDIA GPU；不是远程 110 的 FPS。Progressive 四轮活动采样中发送进程约 0.16–0.18 核，客户端 RDP 线程约占满一核，排除把发送端饱和误当客户端退化。此阶段不作为解决流畅度问题的最终交付。

直接测量实际优化库（`target/rdp-progressive-profile-723.log`）得到每瓦片 RLGR 亮度解码约 139.6 微秒、三个分量解码约 154.2 微秒、完整图像重建约 26.1 微秒。主要耗时是熵解码，而非独立颜色循环。原型使用借用的 64 位 MSB 位缓存、每次补充四字节，取消每个熵符号的通用位切片操作；同一优化样本约 137.9 → 51.7 微秒（`target/rdp-progressive-profile-725.log`），不等同整个应用加速 2.7 倍。

新读取器已接入 RLGR1/3 实际调用，编码器与网络字节格式不变。2,000 组完整/截断输入与原解码器逐项比较，输出和错误结果相同；另覆盖未对齐读、0/32 位字段、长连续位与短尾。257 项图形库回归通过（`target/rdp-codec-tests-rlgr-reader-727/`）。运行源码冻结为 `target/checkpoints/rdp-rlgr-728/`，完整应用与优化成品重新验证中。

718 的一轮 ClearCodec 内存采样在等待夹具退出时超时，保留于 `target/window-probes/rdp-frames-718-memory-clearcodec/`，不计为通过。外层原 20 秒等待短于内层 GUI 的 30 秒期限，已改为 45 秒，让内层验证/资源释放先报告结果；没有忽略非零退出或修改应用关闭逻辑。之后 Progressive 四轮完整采样与正常退出通过，但不能代替该失败场景的复测。


## RLGR 优化成品配对（731）

运行源码冻结 `target/checkpoints/rdp-rlgr-728/`。`target/qa-candidate-rdp-rlgr-731/tomato.exe` 是 release / ThinLTO / 16 codegen units 成品，SHA256 `8DEAB6BAE904618E7A64137A677273A19E53CA3C1062FD8B6F8F583556374829`。所有测量使用相同源码的原生 TLS/CredSSP 回环夹具、同一 NVIDIA/Vulkan GPU，无并行编译，旧版为 669。

| 场景 | 模式 | 旧提交/秒 | 新提交/秒 | 最大更新间隔旧 / 新 ms |
|---|---|---:|---:|---:|
| ClearCodec 密集文字 | TAB | 26.40 | 27.12 | 50.9 / 48.4 |
| ClearCodec 密集文字 | WINDOW | 27.08 | 28.97 | 49.6 / 49.5 |
| ClearCodec 整屏色块 | TAB | 36.54 | 41.09 | 51.0 / 37.0 |
| ClearCodec 整屏色块 | WINDOW | 37.08 | 40.71 | 53.6 / 52.5 |
| Progressive 密集文字 / 510 瓦片 | TAB | 7.03 | 10.29 | 165.7 / 136.4 |
| Progressive 密集文字 / 510 瓦片 | WINDOW | 6.88 | 11.75 | 471.0 / 108.4 |

证据位于 `target/window-probes/rdp-rlgr-731-*`，实际编码以源日志记录为准，默认 adaptive 仍使用 ClearCodec。Progressive 整屏复杂文字吞吐确有改善，但仍只有约 10–12 次提交/秒，不能称为这个重负载场景已经流畅。这里统计 GUI 纹理提交，不是物理显示刷新率，也不是“110”真实网络交互延迟。单轮最大间隔受调度影响，不把一个异常峰值推广为所有场景的降延迟幅度。

活动内存与 CPU 使用 WINDOW 模式，等待首次 EGFX 帧 ACK，预热 2 秒，再采样约 4 秒；按旧/新/新/旧四轮取同版本均值：

| 编码场景 | 工作集旧 / 新 MiB | 私有提交旧 / 新 MiB | 客户端 CPU 核旧 / 新 | 源 CPU 核旧 / 新 |
|---|---:|---:|---:|---:|
| adaptive | 208.21 / 207.83 | 311.20 / 310.90 | 1.35 / 1.37 | 0.44 / 0.45 |
| clearcodec | 202.10 / 201.05 | 313.35 / 318.70 | 1.39 / 1.41 | 0.03 / 0.04 |
| progressive | 240.89 / 238.84 | 347.85 / 347.68 | 1.11 / 1.11 | 0.18 / 0.24 |

源发送进程单独统计，不混入客户端占用。缓冲保留和编解码工作区会影响内存；以上为实际活动样本，不能声称所有连接内存都降低。Progressive 原四轮通过；adaptive 首次旧版采样尚未结束已退出，ClearCodec 首次新版完成截图后未按时退出，两组失败不计为通过。两组均以新标签 `-retry-735` 完整重跑四轮，上表使用通过的重跑样本；原失败、之后复测及之前 718 的失败证据均保留。这不证明首次退出超时的根因已经修复，也不承诺每次退出稳定。

静态 ClearCodec 320×256 远程画面，在 TAB 和 WINDOW 两种模式与旧版逐像素相同（`target/rdp-static-parity-732.json`）；TAB 远程区域起点 `(304,90)`，WINDOW 起点 `(0,0)`。这不是整个 Java UI 的逐像素验收。原 EXE 的六种尺寸图标逐字节一致（`target/rdp-icons-732.json`）。未通过降分辨率、色深或有损默认编码换取这些收益。

478 项工作区默认测试通过，70 项条件测试按既定规则忽略；格式、严格工作区/all-targets Clippy、CLI 自检通过（`target/rdp-default-validation-732.json`）。图形库 257 项、EGFX 66 项及原生协议 12 项回归通过。共享旧配置和受保护旧 EXE 哈希未变（`target/rdp-protected-732.json`）。

Windows 桌面仍显示 Disc，实际全屏 GPU 表面和已保存“110”的交互流畅度没有完成验收；文件粘贴和远端声音听感也未据此宣称通过。便携包包含当前修改与许可证，不捆绑旧 CLI；源归档为 `target/checkpoints/rdp-delivery-733/sources.zip`。全应用迁移和原版所有细节对等仍未完成。


## 桌面恢复连接后的补充验收（740）

2026-10-22 桌面恢复为 Active / rdp-tcp#7 后，使用同一 731 成品继续验证；没有重新编译或改变运行代码。ClearCodec 与 Progressive 的 TAB、WINDOW、FULLSCREEN 均实际创建 NVIDIA GeForce 940MX / Vulkan 表面并捕获连续更新；全屏分别约 35.23 / 10.68 次纹理提交/秒，本次未重现 Invalid surface。截图检查确认 1920×1080 远程区域确实显示，不能视作真实主机交互流畅度验收。

ClearCodec 第一次 WINDOW 轮次完成截图后未在 30 秒内退出，此轮失败保留在 `target/window-probes/rdp-active-737-clearcodec/`。之后启用现有捕获跟踪，连续三个独立窗口轮次均完成截图并正常退出（`rdp-active-739-window-1` 至 `-3`），包含 eframe LoopExiting / window closed 日志；不能据复测通过声称首次超时根因已修复。此前单次 WCT 记录也不足以确立退出卡住的根因，未据此改动 COM 或渲染释放代码。

另运行 12 项真实 TLS/CredSSP 协议回归（1 项原生捕获按默认规则忽略）。鼠标在无画面更新场景的 32 次输入中，中位约 15.19 ms、p95 16.02 ms；这是回环协议事件测量，不是 Windows UI 注入、图形负载下鼠标延迟或“110”真实网络延迟。运行时 Computer Use 的 node_repl 工具未提供，未执行或声称完成系统 UI 鼠标键盘人工等价验收。

汇总证据为 `target/rdp-active-acceptance-740.json`，包含 8 次成功原生捕获、图像哈希、实际后台及保留失败。全屏回环验收缺口已补上；复杂 Progressive 的吞吐、偶发退出超时、真实主机交互、Explorer 文件复制与声音听感仍有验收或优化缺口。旧配置继续直接读取，运行中的用户实例未终止。
