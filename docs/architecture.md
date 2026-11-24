# 架构与设计模式

依赖方向：`apps → ui-egui → services → core`，CLI 直接依赖 services/core。core 不依赖 UI、Tokio、数据库、系统客户端或 GPU。

参考 WordCraft `4062db0ac2928ab58e3794f8017a81032cd6036f` 的分层方式与原生 UI 技术栈；没有复制它的文字处理引擎。参考工程见 https://github.com/storytold/wordcraft 。

## 职责分配

| 模块 | 职责 | 变化边界 |
| --- | --- | --- |
| `crates/core/config.rs` | 连接模型、验证、旧配置兼容和去重 | 与界面、协议无关 |
| `crates/core/repository.rs` | WorkspaceRepository 存取端口与 ConnectionFileRepository | 首次备份、外部修改检测、导出保护；可替换存储 |
| `crates/core/legacy.rs` | 与 Java 相同的连接文件编解码 | 旧加密文件、旧字段加密、连接数组与 Rust 旧工作区兼容 |
| `crates/core/tree.rs` | 基于父子索引的连接树遍历 | 迭代式遍历，防止循环引用 |
| `crates/services/commands.rs` | Command、Output、CommandHandler、Engine | GUI / CLI 的统一用例接口 |
| `crates/services/dialect.rs` | SQL 方言 Strategy | 引用标识符、表列表、分页 SQL |
| `crates/services/database.rs` | 会话与数据库适配 | SQLx 数据传输与类型转换 |
| `crates/core/workbench.rs` | 表元数据、类型化单元格、行变更与分页请求 | 区分 DEFAULT、NULL、文本与二进制 |
| `crates/services/database/workbench.rs` | 表目录、分页读取、批量写入与事务 | 驱动适配、保存点、失败回滚 |
| `crates/services/database/plan.rs` | 参数化行写入计划与 INSERT 导出 | 方言引用、原值冲突检测、无主键单行定位 |
| `crates/services/terminal.rs`、`terminal/` | 本地 PTY / 原生 SSH 统一适配器 | 有界输入输出、认证状态、PTY 确认、主机密钥、连接释放；外部客户端单独隔离 |
| `crates/services/remote_desktop.rs`、`remote_desktop/vnc/` | 原生 RDP/VNC 协议适配器 | 旧字段映射、认证、最新帧与有界输入；RDP 在 GUI 线程轮询，VNC 在后台解码并由 RAII 中止 |
| `crates/services/network.rs` | Redis、HTTP 服务、转发、下载 | 网络资源与生命周期 |
| `crates/services/files.rs` | 文件列表、写入与 CSV 导出 | 文件系统操作 |
| `crates/services/tools.rs` | 图片、VOC、快捷方式用例 | 独立于界面，可直接测试 |
| `crates/services/hosts.rs`、`trae.rs` / `trae/history.rs` | 直接兼容旧分组/会话文件的仓储与目录日志 | 外部修改检测、提交/回滚；历史读取和目录变更不进入绘制代码 |
| `crates/services/redis_data.rs`、`mongo.rs` | 类型化数据编辑、Cluster 路由与集合会话 | 页面/值的保留预算、写入验证、连接池与关闭释放 |
| `crates/services/terminal/ssh/tunnel.rs` | 可复用的 SSH TCP 转发作用域 | 沿用原认证与主机密钥策略、引用/内嵌配置兼容、16 通道上限和 socket 释放 |
| `crates/services/extractor/` | 签名注册表、长度策略、PE 树与重建、流式导出 | 64 KiB 扫描窗口；协议解析和有效载荷不放入工具视图 |
| `crates/ui-egui/app/` | 工作区控制器、连接管理、对话框 | 将界面意图转换成业务命令 |
| `crates/ui-egui/panels/` | 各工具 UI | 生成意图，不实现协议 |
| `crates/ui-egui/panels/terminal/`、`panels/desktop/` | 终端和远程桌面输入、绘制 | ANSI 单元格、按键映射、IME、焦点与剪贴板事件 |
| `crates/ui-egui/tabs.rs` | 会话状态与会话 Factory | 根据连接种类创建视图状态 |
| `crates/ui-egui/tabs/workbench.rs` | 表页及暂存编辑状态 | 删除/更新/插入计划、刷新失败后禁止重复写入 |
| `crates/ui-egui/panels/sql/table/` | 表格、单元格编辑、复制导出、图标工具栏 | 可见行绘制，与协议代码分离 |
| `crates/ui-egui/tasks.rs` | 后台 Actor 与完成事件 | 无阻塞界面、按会话排序 |
| `apps/tomato` | eframe 桌面启动与运行时装配 | Composition Root |
| `apps/tomato-cli` | CLI 参数和输入输出 | 同一个引擎的第二个前端 |
| `xtask` | 构建、归档与校验 | 所有平台共用打包逻辑 |

## 使用的模式

1. **Repository + Dependency Inversion**：调用者面对 `WorkspaceRepository`，不绑定 JSON 解析。配置损坏时禁止覆盖原文件；临时文件写入成功再替换。
2. **Command**：业务操作建模为带参数的可序列化 enum。`CommandHandler` 使同一调用端可以接真实或测试引擎。
3. **Facade**：`Engine` 提供统一入口，封装 SQL、Redis、文件和工具适配细节。
4. **Strategy**：`SqlDialect` 分离 MySQL、PostgreSQL、SQLite 的 SQL 语法变化；数据库产品复用协议家族。
5. **Adapter**：SQLx、PTY、Redis 和文件系统通过各自模块接到统一命令和结果类型。
6. **Factory**：`create_tab` 集中构造不同连接的会话，避免界面各处判断协议与初始化资源。
7. **Actor + Observer**：每个数据库会话拥有独立消息队列和 Engine，串行执行以维持事务；不同会话、文件任务并行执行，通过 Completion 事件通知界面重绘。
8. **Composite 语义 + 索引遍历**：连接和分组用统一的父子关系，`ConnectionTree` 一次索引后遍历子树，删除和展示复用树结构。
9. **RAII / Scope Guard**：终端拥有子进程，服务拥有 Tokio task；关闭标签或退出应用时 Drop 负责杀进程/中止任务，避免服务遗留。
10. **State 建模**：`Panel` enum 明确表示 SQL、终端、文件、编辑器、服务、工具和待迁移会话，编译器检查分支。小型固定状态集不额外引入装箱状态对象。
11. **DTO + 防腐层**：连接 `options` 展平保存未知 Java 字段；Legacy Codec 沿用 Java 加密格式，以便直接共享原连接文件。运行时不依赖 Java；独立 Java 生成的测试夹具用于互通验证。
12. **Composition Root**：桌面和 CLI 各自组装运行时、引擎、配置仓库；不使用全局服务定位器。

不为了列举模式而增加空抽象。固定协议分支使用 enum，可变化的 SQL 语法使用 trait；新的协议实现完成后再注册为可用。连接种类目录不代表协议功能已经实现。

对象复制采用可注入 Clipboard/Connections 仓储、不可变 Arc 进度快照和有作用域的任务 Owner。视图只提交 PasteObjects；Storage 选择服务端复制或临时文件流式转移策略。上传写入器统一管理普通上传与跨连接复制的 close/abort，Drop 清理带独立完成通知；应用退出先停止文件 actor，再等待任务和全局清理，最后关闭运行时。测试注入内存剪贴板，实际 GUI 命令链不接触用户系统剪贴板。

SQL 补全的候选规则放在 core/sql_completion，数据库目录采集放在独立后台模块，标签只持有 Arc 目录、候选和窗口状态。后台请求使用独立 Purpose/代次，不覆盖前台 SQL 的 pending 或 busy 状态；回复同时检查代次与 schema。候选接受和 Markdown 特殊编辑共用 egui 的可选事件扩展，在正常文字/IME 事件序列内处理，使用原撤销历史；未启用扩展的控件维持默认行为。

0.1.8 的 ImportPlan 将重复分析、选择与引用重映射放在核心层，UI 只持有选择草稿，Repository 成功后统一发布候选 Store。S3 树通过 ObjectTree 命令复用 Storage 适配器和 LocalTree 的变更结果，保留同连接串行次序；虚拟路径与本地路径在服务边界区分。远程编辑器共享 Arc 连接和运行时凭据，未保存的密钥不写入配置。egui 撤销补丁只提供显式预算与移交历史的接口，文档编辑器按预算裁剪，不修改上游默认行为。

## 并发和失败行为

任务提交分配稳定任务 ID，完成时按标签 ID 路由，不依赖当前选中标签。关闭标签时移除其待处理结果；修改文件保存时记录提交快照，保存期间新增编辑仍保持未保存状态。连接关闭提交 Disconnect，并关闭数据库 Actor 的入口。

查询标签和表标签按连接 ID 路由到同一个数据库 Actor，最后一个使用该连接的标签关闭后才断开。表保存重新读取结构，用参数绑定生成行写入计划；更新同时比较完整原始行，冲突或约束失败撤销本批修改。已有事务使用保存点，不在保存表数据时提前提交。写入成功但刷新失败单独报告，清除已提交的暂存修改并禁止继续编辑，避免用户重试产生重复插入。事务提交/回滚同步同连接所有表标签，并要求刷新后继续编辑。

同连接只有一个标签持有连接树的 schema / 表元数据。关闭所有者时转交仍打开的表标签；重新打开查询标签时移回元数据并继承事务状态，不重新建连接或清空活动 schema。

SSH 通过 Tokio task 驱动，有界通道和输出通知连接界面；关闭标签同时中止任务并 shutdown socket。RDP 的 Windows 剪贴板持有线程绑定窗口，因此会话 future 保留在 GUI 线程，网络就绪经 waker 请求重绘；未选中的标签同样轮询会话，空闲时不固定频率刷新。图像仅保留最新帧，协议实现和输入绘制分别位于 services 与 UI。详见 `remote-protocols.md`。

SQLite 的表分页、生成列、默认值、NULL/文本区别、二进制、无主键重复行更新、冲突/约束整批回滚、SQL 回读、TSV 暂存及跨标签会话生命周期有本地测试。GitHub Actions 启动专用 MySQL/PostgreSQL 服务进行类型与事务集成测试；本地也已通过 scripts/qa/isolated-databases.ps1 的隔离回环夹具验证，记录和版本边界见 migration.md。测试不使用旧配置中的服务器。

错误由 `anyhow::Result` 传播到界面状态或 CLI 非零退出码；不把网络错误转换成模拟成功数据。配置验证检查重复 ID、父目录缺失与父子环。导入先在副本上构造和验证，成功后才替换内存状态。

核心测试覆盖旧 AES 配置、目录与 SSH 引用重映射、循环配置拒绝、敏感字段移除、SQLite 多语句/事务、SQL 标识符引用、HTTP 文件服务、TCP 转发、SVG 与 VOC 转换。外部服务器协议与系统客户端需要单独集成验证，不能由这些测试推断已完整兼容。


VNC 协议按认证/会话、帧解码和输入编码拆分，复用 DesktopSession 生命周期和绘制端，避免把 socket/DES/Hextile 写进视图。服务器管理按控制器、基本配置、表格、弹窗拆分；文件协议以共同 Operation 和有界 FileSession actor 接入 UI。模式用于隔离协议和存储变化，不额外增加每个节点的装箱对象。

本地连接树的控制器位于 app/local_tree.rs，视图只提交展开或刷新意图；ListVisibleFiles 命令由共享 CommandHandler 在后台执行。目录回复按连接、路径和代次路由，刷新或改配后丢弃旧代次回复。树行遍历借用缓存的 Entry，不复制每项路径和名称；列表缓存与可见行绘制分别限制保留数据和每帧工作。终端的 CursorBlink 只保存单调时钟起点与暂停相位，绘制函数接收可见状态；调度和 ANSI 单元格绘制不依赖协议实现。

Markdown 编辑、工具栏动作、查找窗口、源码高亮和预览分别位于 panels/editor 的独立模块；后台 CommonMark/图片处理在 services/markdown.rs，UTF-8 搜索及 Java 风格替换引用在 core/search.rs。搜索缓存随文档版本失效，关闭编辑标签显式释放 egui 的文档与查找框撤销状态。app/tree_drag.rs 区分连接和文件的拖动载荷：连接重排规则在 core/tree.rs，先持久化候选模型再发布；真实文件移动由 LocalTree 命令在后台执行，不把文件 I/O 写入树绘制函数。eframe 即时子窗口的截图/剪贴板事件修补单独保存在 vendor，说明见 third-party/eframe-PATCH.md。

图片读取采用共同 ReadImage 命令，SFTP 复用当前认证会话，S3/OSS 复用现有 Operator 和桶路由。services/files/images 分离有界读取、栅格/SVG 解码与逐帧 GIF 播放；UI 的缩略图缓存、预览状态、窗口绘制分别管理，后台回复匹配请求 UUID，切图/刷新/关闭取消旧请求。预览的下载和删除复用真实文件命令，回调持有发起时的目标路径，避免响应时选中项变化导致误删。画廊列表使用 Arc 共享，排序或重新列目录后失效。原版图片窗口的测量器见 core/tests/java/ImagePreviewWindowOracle.java；截图只读取自身 Scene，不抓取桌面。

SQL 会话按连接 ID 与数据库名缓存，同连接的 SSH 转发继续共享。左侧 SQL 树的元数据归 app/sql_tree.rs 管理，查询标签借用 Arc 元数据但保留独立数据库范围；关闭树对应标签不再把其状态移入另一个查询。

原命名查询由 core/queries.rs 定义意图与路径，services/queries.rs 提供文件仓储，app/query_tree 的目录控制器、文档保存和原生弹窗分别处理状态与交互。任务按数据根目录串行，读写 SQL 不进入连接配置仓储。文档以内容指纹跟踪修改，不常驻第二份 SQL 正文；保存回复基于已写入内容更新基线，期间的新编辑继续标记未保存。关闭前保存等待成功，错误保留草稿。目录缓存单独按最近访问淘汰，关闭连接的旧代次不再发布目录或解析结果。

数据库原生结果的协议读取、JDBC 兼容数字格式和 NULL 载荷模型分别位于 services/database/raw.rs、raw/format.rs 和 result.rs。SQLx 的局部安全借用扩展让 Any 会话继续负责连接生命周期，而原生 MySQL/PostgreSQL 文本解码保留驱动通用层无法表达的值；所有查询、表操作和事务仍使用同一物理会话。结果模型以位图区分 SQL NULL、空文本和字面量 NULL，JSON 序列化和表格显示统一通过 value/is_null 读取。

结果表绘制在 panels/sql/results.rs，选择、记录索引、排序和受限复制在 tabs/query_results.rs；二者只借用查询值，不为显示重建数据集。数据库返回新结果时同时丢弃旧选择与列宽状态；排序按原始行索引保留选中的记录。Java 原控件金样记录实际的升序、降序、清除排序与重复值/NULL/UTF-16 排序，而不是用 Rust 实现自算预期。

脚本拆分/来源提取在 core/sql_script.rs，顺序执行、统计、自动 EXPLAIN 与状态查询在 services/database/script.rs，标签呈现在 panels/sql/script.rs。旧的原始 Sql 命令继续服务元数据和内部查询；用户运行通过 SqlScript 用例逐条执行并保留错误。所有命令仍进入同一连接 actor，不因 UI 标签或目标 namespace 新建另一套会话。

core/query_edit.rs 单独决定来源映射与可编辑性；services/database/query_rows.rs 读取真实表结构、列注释并在保存时复查，借用原查询连接、拒绝混入手动事务，参数化写入并逐行检查恰好影响一条记录。表页面保留完整原值并发检查，查询结果使用原版主键/整行匹配规则，二者通过共同 SQL 计划及事务执行器隔离差异。tabs/query_results/editing.rs 管理稀疏修改和物理行身份，panels/sql/query_edit.rs 管理底栏动作；协议代码不进入 egui 绘制函数。


结果表的视图投影只保存物理行/列身份和显示索引，筛选、排序、隐藏和拖动列不会重建单元格载荷。暂存编辑与回写命令始终使用物理身份；异步回复还受结果代数约束。筛选比较独立于控件，十进制比较借用数字字符流，原 Java 的条件解析与组合变换由实际控件金样验证。日期选择也拆为值模型、编辑状态和原生弹层；应用统一在根窗口管理弹层，避免嵌套原生视口。编辑预算在移动正文前验证，失败保留原草稿；关闭或替换编辑器显式释放 egui 文本历史。

AI SQL 继续迁移：核心层分开保存原版协议文本、文档插入策略和 global.json 字段/密钥兼容；服务适配器只返回 SQL 文本，没有数据库执行依赖。界面控制器单独持有快照与任务代数，模型回复由 AI 专属 Purpose 分派，不借用普通查询 busy 状态；其他查询不会清除未完成的 AI 请求。生成期间继续编辑时，按原方法改在当前光标后插入，并只选中生成 SQL。设置/需求草稿由独立原生子窗口持有，关闭释放文本状态，配置仍通过既有原子保存与修订冲突检测落盘。表结构上下文复用后台补全元数据，不读取查询数据行；排序后最多 80 张表、48,000 个 UTF-16 单位。完整 UI/后台和原生窗口验证以对应后续检查点为准。

nb3 迁移拆分为核心请求/元数据/目录规则/方言字面量、服务归档索引/记录读取/分片写入/定义流/文件仓库/进度观察器，以及数据库 actor 上的备份策略。文件操作与数据库操作使用独立的类型化 Command，GUI 与 CLI 共用引擎。MySQL 导出借用驱动原始单元格，生成列不进入 INSERT 字段，二进制和特殊字符使用有效 SQL 表达式；还原先校验整个文件及所有压缩校验和，再执行对象重建。已有手动事务拒绝进入备份流程；快照/锁表退出与文件提交分离，失败先释放数据库状态，临时文件随后自动清理。备份窗口草稿、原版复选框联动和实际归档 I/O 分开维护；窗口/目录完整迁移状态以 migration.md 对应检查点为准。


表结构标签与数据标签使用独立、包含实际数据库和命名空间的身份键。打开结构只发送 Definition 命令；数据页通过 Page 命令按需读取，打开/刷新结构不覆盖另一标签的暂存行。SQL 补全根据实际正文、光标及元数据代次失效，字体布局缓存重新分配不重置候选选择。查询命名弹窗拥有独立 Editor，确定、取消及窗口关闭后释放其 egui 撤销状态。

设计器的生产入口已经接通：`app/table_design` 控制打开、SQL 预览、保存及相关标签刷新，`panels/sql/designer` 处理网格输入，`tabs/table_design` 保存草稿状态；实际数据库操作经共享 CommandHandler 和数据库 actor 执行。1676 已对 Hive 原生控件执行一次 DDL 保存、数据页刷新和查询流程；具体服务端和视觉验收边界仍见 migration.md，不能只用模型测试代替完成判定。剪贴板采用原中文列标题的 JSON 格式，有界流式解码/编码；普通行的布尔标志以紧凑状态保存。
