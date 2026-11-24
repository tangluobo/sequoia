# Oracle 原生适配接入

当前状态：Oracle 复制/传输、nb3 备份还原已有原生调用链，真实 Oracle 服务端完整流程仍待验收。Hive 已补齐实际界面路由与原生控件工作流，见 migration-1676.md。下文的未接入描述属于早期记录。

当前为部分迁移，尚未完成真实 Oracle 数据库的往返验收，也不代表整套应用迁移完成。

## 已连接的操作链路

- 原来的 `ORACLE` 连接类型进入 SQL 工作区。使用 Oracle 官方纯 Rust `oracledb` 驱动，固定为 `26.0.0-beta.4`，不调用 JDBC、OCI 或 JVM。上游源码及许可保存在 `vendor/oracledb`，本地补丁见其 `TOMATO_PATCHES.md`。
- 旧配置中的 `database` 继续表示物理 SID；以 `/` 开头表示 service name。树中用户、查询工具栏、查询文件与表结构的 owner 单独传递，切换 owner 不覆盖 SID/service，也不保存临时的 `__tomatoOracleOwner` 作用域。
- 用户列表、表/视图、对象页、字段、主键和注释使用 `ALL_*` 目录读取。用户节点复用 MySQL 的有界平面目录控制器；PostgreSQL 保持自己的数据库→schema 层级。关闭一个 Oracle 用户节点不会断开其他用户共用的物理会话。
- 原始查询保留 NUMBER 精度、NULL、二进制、DATE 的时间部分、时间戳和时区。SQL 脚本识别 Oracle 替代引号与 PL/SQL 单元；过程单元之间使用独占一行的 SQL*Plus `/` 分隔符。
- 打开表使用原表格组件，支持排序、分页、插入、修改、删除和手动事务。参数使用 `:1` 等绑定，日期使用明确的转换格式，无主键行使用 ROWID 子查询限制为一行；LOB 比较使用 `DBMS_LOB.COMPARE`。
- 表设计器读取服务器原始 `DBMS_METADATA.GET_DDL`，接入字段编辑、ALTER SQL 预览、保存、结构重读和相关标签刷新。未修改的默认表达式、字符长度语义、标识列/虚拟列属性保留；虚拟表达式和标识列生成方式不通过普通字段编辑器重写。索引、复合外键、触发器接入原有信息页。创建表仍保留原 Java 生成器忽略 Oracle 默认值、自增和列注释的边界，不能把这三个选项记成已完成的创建能力。
- 删除、清空、截断、表重命名和当前登录用户所属视图的重命名接入对象操作。用户创建/删除对应原 Java 的 `CREATE USER ... IDENTIFIED BY ...` / `DROP USER ... CASCADE`；字符集编辑与原版一样不执行修改。
- Oracle DDL 会隐式提交：存在手动事务时拒绝结构修改；多条 DDL 失败时提示可能部分生效并保留草稿，不声称已经回滚。Oracle 没有 `RELEASE SAVEPOINT`，不会向服务器发送该语句。应用保存点使用兼容 Oracle 12.1 的短名称。

## 资源与配置

每个物理会话拥有一个阻塞驱动线程和容量为 2 的消息通道。结果只在后台读取，界面帧不做网络 I/O。每次抓取 16 行，语句缓存 8 项；列名映射在行之间共享。结果/单元格/结构快照按各自 8 MiB 上限处理，目录最多 10,000 行，查询列最多 4,096 项，JSON 最多 64 层。协议累计响应上限 32 MiB。

关闭会话使用拥有的 socket 中止句柄打断阻塞读取，并唤醒空闲线程。上游没有并发取消查询接口：中途取消结果读取会使该会话失效，需要重新连接，不能承诺取消后仍保留原事务。TCP 与初始认证使用连接描述中的超时，完成连接后的驱动调用设置 30 秒读取超时。

配置格式及加密仓储不变。验证使用临时文件和自有 loopback 服务，不联系已保存的主机。

## 验证与剩余项

当前开发验证副本为 `target/qa-candidate-oracle-448/tomato.exe`，不是优化发布版。`target/oracle-*-446.log` 记录完整工作区格式、严格 Clippy、463 项默认测试（69 项按条件跳过）、全目标构建和 CLI self-check。`target/oracle-driver-tests-447.log` 的六项独立驱动测试验证短/畸形报文、阻塞读取中止、追加响应的总量上限，以及底层 OSON 深度/空间/偏移边界。应用连接工厂还通过自有 TNS socket 验证拒绝畸形握手和释放连接；GitHub Actions 已加入这些驱动测试步骤，未在此声称远程 Actions 已执行。

`target/database-fixtures/probes/oracle-449/validation.json` 记录 MySQL 8.4.11 / PostgreSQL 17.11 的 19 项真实后端回归及原生控件工作流：树、表设计、对象菜单、事务、查询结果写回、跨连接传输及 nb3 备份/还原。它证明共享流程的回归状态，不能用作 Oracle 服务器互通证明。`target/window-probes/oracle-candidate-448/validation.json` 记录该 EXE 的共享设计器 7 页原生窗口捕获：这是 MySQL 布局夹具，不是 Oracle 的正向连接或全像素一致证明。

以下仍需继续：真实 Oracle 各版本和字符集的端到端验证、Oracle 复制表/跨连接传输与备份还原、查询结果可编辑列的可靠来源信息、LOB 写回与复杂类型的完整覆盖。原 Java 函数文件夹没有加载动作；索引/外键/触发器的添加删除按钮也只显示待实现状态，不能把保留这些入口算成原生 DDL 编辑完成。Hive 尚未接入。

参考：[Oracle 官方 Rust 驱动](https://github.com/oracle/rust-oracledb)、[Oracle TO_TIMESTAMP](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/TO_TIMESTAMP.html)、[Oracle RENAME](https://docs.oracle.com/en/database/oracle/oracle-database/18/sqlrf/RENAME.html)。
