# 云服务与消息队列迁移（工作区）

S3 的旧端点兼容、SSH 跳板及证书窗口临时凭据/存储刷新已通过最终回归和优化成品窗口检查，详见 [S3 与证书存储](s3.md)。

本轮已完成优化发布构建，可运行 `F:/data/git/rust/tomato/target/qa-release-cloud-mq/release/tomato.exe`，产物身份以同目录 `build-info.json` 为准；`target/release` 的旧程序可能仍由用户运行，不覆盖它。不能将旧版可执行文件视为已经包含本轮功能。全应用的迁移及逐状态视觉验收仍在进行。

## 原 Java 对照

只读参考 `D:/Data/Git/tangluobo/tomata` 的 AliyunService、TencentCloudService、DdnsService、LetsEncryptService、KafkaService、RocketmqService，以及对应连接处理器、数据视图和证书/实例属性窗口。沿用原连接类型、username/password、SSH 引用和内嵌跳板字段，以及原树图标。

原版阿里云列出的部分产品只有树入口，没有数据操作；本轮不将这些入口计为完整云产品客户端。

## 已接通的用例

- 阿里云 RPC HMAC-SHA1、腾讯云 TC3-HMAC-SHA256 请求；地域、实例、域名及记录分页；DNS 创建、修改、删除；实例属性。
- 共享原 `~/.tomato/ddns.json`：保持未知字段、首次备份、并发修改保护。A 记录开关实际持久化；后台获取 IPv4、仅缓存成功更新的地址、失败重试、禁用后停止更新。没有启用记录时不探测公网 IP。
- DDNS IP 修复（367）：独立 HTTP 客户端强制 IPv4、禁用环境/系统 HTTP 代理、不跟随重定向；恢复 Java 的 `https://gitlab.tangluobo.com/ip`，与 4.ipw.cn、ipify、icanhazip 共四个来源并发交叉检查。只接受完整、最多 64 字节的纯 IPv4 响应，过滤内网/回环/共享地址和常见保留网段；HTML 中的 IP、错误响应、超长/截断内容均不能参与投票。至少两个不同来源一致且票数高于其他候选才更新，单来源或平票只提示检测失败并保留原 DNS。每轮最多四个在途请求，最长单请求 8 秒；无 DDNS 条目仍不访问探测接口。代理客户端不会复用于云 API 签名请求。
- `69.63.203.33` 报告的排查边界：原实现从任意响应文本抽取首个 IPv4，并沿用自动代理；当前进程未检测到代理环境变量，Windows 显式代理未启用。因此能确认的是代码缺陷及其复现，不能据此断言该地址当时一定来自代理或某一真实探测站。没有读取或改写用户的 DNS 记录，未对用户的云连接发出请求。新版本启动后按上述规则重新检测并更新已启用的旧配置记录。
- 367 验证：工作区 424 项通过、0 失败、63 项需显式启动的检查被忽略，43 组（含 doctest）。新增解析/截断/交叉确认回归 3 项，原后台真实云 API 夹具增加错误网页不写 DNS 的断言；所有 HTTP/DNS 更新请求只在自建回环夹具中执行，响应内的公网格式 IP 仅为文本，不作为连接目标。格式、严格工作区 Clippy、全部目标构建及 CLI 自检通过。证据：[完整回归](../target/ddns-tests-367.log)、[统计](../target/ddns-test-summary-367.json)、[代理设置只读检查](../target/ddns-proxy-diagnostic-367.json)。
- 367 优化成品：[tomato.exe](../target/qa-release-ddns-367/release/tomato.exe)，同目录 [build-info.json](../target/qa-release-ddns-367/release/build-info.json) 记录完整证据。发布 CLI 自检、旧加密配置只读加载（60 连接/4 分组）、六份图标资源校验和隔离原生启动通过；空闲工作集 180.63 MiB、私有提交 204.49 MiB（15 秒预热 / 10 秒采样，自动后端）。旧 EXE、共享 connections.json 和 ddns.json 哈希保持不变；未使用真实云凭据或查询实际 WAN IP。当前旧窗口须退出后运行新 EXE 才会使用修复。



- ACME DNS-01 订单、TXT 创建和传播检查、RSA 2048 CSR、验证与签发；四家 CA 的选择及 EAB。取消和失败清理本任务创建的 TXT。通用 PEM、Nginx、Apache、Tomcat/IIS PFX、Java JKS，以及 ZIP，实际写入所选 S3/OSS 存储。
- Kafka 原生 TCP 协议：主题创建/删除、统计、集群、消费组详情和积压；带 Key/Headers/Partition 的发送；时间、Offset 和未消费消息浏览。浏览不会提交消费位点。显式路由拒绝触发有限重试；发送超时等结果不确定的失败不自动重发。
- RocketMQ 原生 Remoting：NameServer/多 Broker 路由、主题与消费组管理、集群/运行信息、发送、Offset/时间/Key/ID 查询、延迟消息查询、消费轨迹，以及向在线消费者发起真实重新消费请求。
- GUI 命令总线、后台任务、树的延迟加载、相关树刷新、主题/消息工作台、DNS 编辑和证书窗口均已接入实际服务。

## 资源与兼容边界

协议请求、响应、解压结果、分页、消息和 UI 队列均有上限。目录/消息表仅绘制可见行；每条命令拥有并释放 socket 和 SSH 跳板；关闭标签取消在途消息操作。DDNS 后台任务随应用退出释放，证书取消保留 TXT 清理阶段。生产程序不依赖 Kafka/RocketMQ 的 Java 客户端或本轮测试服务器。

Kafka 依据原版的明文连接与 SSH 跳板工作流实现；SASL/TLS 等新增安全配置未列为本轮原版对等功能。固定 wire API 组合已在 Kafka 3.9.1 验证，其他版本仍需兼容验证。RocketMQ 已在 5.3.1 验证，不将全部旧版本兼容性视为已经证明。

## 验证证据

- `target/cloud-mq-tests-293.log` 和 `target/cloud-mq-workspace-tests-293.log`：389 项默认回归通过、57 项条件夹具默认忽略；后者包含工作区文档测试。`target/cloud-mq-clippy-293.log` 严格检查通过，`target/cloud-mq-fmt-293.log` 格式检查通过。
- 云服务的 7 项默认集成测试涵盖两家云 CRUD、实例/域名分页、原 Java TC3 签名金样、旧 DDNS 备份和未知字段、真实后台更新/失败重试/禁用、ACME JWS 验签/CSR/DNS-01/S3 上传、PFX/JKS 解析、EAB、ZIP，以及取消清理。
- `target/mq-backend-287.log`：2 项真实服务集成通过，使用专有回环 Kafka 3.9.1 和 RocketMQ 5.3.1。包含主题/路由/统计、发送/查询、消费组偏移/积压、未消费消息、消费轨迹及实际消费者重新消费回调；Kafka Gzip/Snappy/LZ4/Zstd 和 RocketMQ Zlib/LZ4/Zstd 压缩中文消息均实际存入 Broker 后由 Rust 客户端读取。
- Kafka 默认 Key 分区算法与当前原 Java 使用的 Kafka 4.3.1 `Utils.murmur2` 金样一致，也与夹具 SDK 3.9.1 一致。金样及来源哈希见 `target/oracle-cloud-mq-285/partition-identity.json`；真实 Broker 验证了 Key 默认分区、无 Key 轮询，以及 RocketMQ 跨队列发送。
- `target/cloud-ui/native-295/validation.json` 和 `target/cloud-native-295.log`：腾讯云、阿里云分别通过树、DNS 实际增改删、DDNS 文件开关、证书表单和 ACME/S3 结果的原生控件流程；包含阿里云域名右键申请证书及旧 certificateEmail 预填。`target/cloud-backend-295.log` 的 7 项云服务回归通过。连续签发时隔离 CA 夹具会清空上一订单的证书结果。
- `target/mq-ui/native-293/validation.json`：Kafka 和 RocketMQ 实际树、创建、发送、时间/Offset/ID 查询、返回列表和删除确认流程通过。验收等待数据加载完成，按可见控件区域定位，并核对删除确认的目标名称。
- `target/window-probes/cloud-mq-dev-293/validation.json`：正常开发版程序的六组窗口截图；`target/oracle-cloud-mq-272` 保存当前原 Java 的主题、查询和发送页面截图及节点几何。它们是对照资料，不是全部 Rust 界面像素一致的证明。
- `target/cloud-mq-selfcheck-293.log`：CLI 自检通过；`target/cloud-mq-legacy-293.log` 只读解析原配置的 59 个连接和 5 个分组，没有连接服务器。共享配置 SHA256 保持 `F7B178E9E4CB3E780AA21F6E62ADD859EFC4A3DC5E520CEC0C8A1EEC2A826624`。

新客户端首次加载失败后等待用户刷新，不逐帧重放网络请求；此行为有实际 UI/任务队列回归。发送与查询的 Key、发送与 Offset 查询的 Partition 分开维护。消息正文输入最多 2 Mi 字符、Headers 最多 64 Ki 字符，发送仍受协议请求字节预算限制；两者撤销缓存各限制 64 KiB，关闭标签时主动释放编辑器和日期选择状态。

没有访问已保存的用户主机，没有用真实云凭据或公开 CA 进行签发。隔离 ACME 验证证明协议与导出链路，不代表证书已获公开信任。发布构建、发布窗口和空闲内存配对测量已完成；全应用迁移及全部视觉细节仍未全部验收。


2026-10-19 通过只读网页查询核实了四个公开 ACME 目录的 `newAccount`、`newNonce`、`newOrder` 端点：[Let's Encrypt](https://acme-v02.api.letsencrypt.org/directory)、[ZeroSSL](https://acme.zerossl.com/v2/DV90)、[Google Trust Services](https://dv.acme-v02.api.pki.goog/directory)、[SSL.com](https://acme.ssl.com/sslcom-dv-rsa)。查询没有注册账户或下单，不构成真实域名签发验证。CA 名称、EAB 表单规则沿用原 Java；实际证书有效期由 CA 决定。

## 2026-10-20 发布验收 296

GUI SHA256 `544C2D0BD307B9320AC4568F9623828D120EACB1D2A85BA3F34228B21BD492B3`，CLI `31CB4AE3C3C8700A2134EDE19A04047298436A1937AC0DE95FB4F7A1C58AFA0A`。发布源码检查点 cloud-mq-293 的 1,430 个文件哈希保持不变。`target/window-probes/cloud-mq-release-296/validation.json` 的六组正常程序窗口均已打开检查；它不代表全应用或全部交互状态逐像素一致。发布 CLI 自检、原 59 连接/5 分组只读检查通过，旧共享配置未改。

内存配对见 `target/window-probes/cloud-mq-final-memory-296/validation.json`：

| 程序 | 后端 | 工作集 MiB | 私有提交 MiB |
|---|---|---:|---:|
| old | auto | 179.98 | 204.23 |
| new | auto | 180.54 | 204.44 |
| old | vulkan | 114.11 | 115.05 |
| new | vulkan | 114.53 | 115.00 |

旧程序为保留的 tools-255 发布成品；新程序为 cloud-mq-293。每个窗口使用独立空配置，预热 15 秒、采样 10 秒，编译和本轮 Broker/ACME 测试服务已停止，用户原有程序保留。工作集与私有提交分别报告；不是 Java/Rust 对比，也不是活动连接峰值保证。
