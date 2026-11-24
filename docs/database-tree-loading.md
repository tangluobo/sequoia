# 数据库树按需加载

对照原 Java 的 `AbstractDbHandler.connectHost`、`openDatabaseWithFolders` 和
`PostgresDbHandler.openDatabase/handleSchemaDoubleClick`：

- 打开 MySQL/Oracle 连接只提交一次数据库/用户列表请求，不创建查询标签，不加载任何库的表目录。
- 未打开的数据库使用原来的灰色图标，不显示展开箭头。单击只选中；双击或右键“打开”才读取该库的表与视图。
- PostgreSQL 保留数据库→模式→对象层级：打开连接只列库，打开库只列该库的模式，打开模式才加载其表和视图。没有提前枚举其他库或模式。
- 已打开节点的箭头只控制折叠/展开。搜索可以显示已经缓存的子项，但不能给未打开的库凭空增加表、视图、函数等文件夹，也不会触发网络查询。
- “已加载”与“列表非空”分开记录：合法的空库列表不会因再次打开连接而重复读取；右键刷新仍可主动重读。空表清单也沿用已有缓存。
- 原来的作用域路由、连接代次/请求代次校验、关闭释放、32 个打开数据库和 PostgreSQL 256 个模式上限、16 MiB 树缓存限制保留。没有增加全量对象缓存。

旧连接文件不需要转换。本次只修改内存中的目录状态与侧栏事件，没有修改共享配置。

优化 GUI：`target/qa-candidate-database-lazy-669/tomato.exe`。475 项默认回归、19 项隔离后端
回归、12 条真实 MySQL/PostgreSQL UI 流程通过；成品完成三种隔离文件视图的原生启动检查
及六种嵌入图标验证。原生文件视图截图不代替数据库树与 Java 原版的像素对照。

回归入口为 `app::sql_tree::lazy_tests`，使用真实 egui 侧栏事件和不运行的后台 runtime，
捕获请求而不访问保存的主机；覆盖 MySQL、Oracle、PostgreSQL 的 200 库列表、搜索、单击、
双击、空对象列表、默认库配置不变和显式刷新。MySQL/PostgreSQL 的既有真实数据库树回归
仍通过仓库内的独立 loopback fixture 执行。Oracle 的真实服务器验收范围见
[Oracle 接入](database-oracle.md)。本条不代表全应用像素对等或新的发布版内存测量。
