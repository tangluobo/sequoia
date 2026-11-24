# 表设计器：索引

索引页使用固定 30px 行高的可编辑网格，列为行选择器、名称、字段、类型、方法、注释；不再额外显示“唯一”布尔列。`UNIQUE` 是索引类型。

- 名称和注释双击编辑，也可选择索引后按 F2 修改名称；Enter 提交、Escape 恢复。编辑框不改变行高。
- 字段单元格正下方左对齐展开组合索引多选下拉，勾选字段、调整顺序；MySQL 普通/唯一索引可设置前缀长度，支持的方法可设置 ASC/DESC。选择即时反映到草稿；点击外部收起并保留选择，Escape/取消恢复打开前的字段。确认会校验字段组合。
- 类型、方法直接在单元格下方左对齐展开，不接受任意 SQL 文本。MySQL 提供 NORMAL/UNIQUE/FULLTEXT/SPATIAL；PostgreSQL 提供 NORMAL/UNIQUE 和 btree/hash/gist/spgist/gin/brin；Oracle 使用其自己的方法集合。选项组合、字段数量、存在性和长度在生成 SQL 时校验，数据库仍决定存储引擎和字段类型是否支持该索引。
- 行选择器支持 Ctrl/Shift 多选及拖动，删除作用于所选索引。新增、修改和删除均进入同一表设计草稿，SQL 预览反映实际 DDL，保存通过后台数据库 actor 执行，回读结构并刷新相关标签。
- 仅名称改变时使用 RENAME，避免重建。MySQL 的字段和索引修改合并为一次 ALTER TABLE；PostgreSQL 使用事务或已有事务的保存点。Oracle DDL 的隐式提交限制仍然适用。
- 保存比较服务器字段及索引快照，检测外部修改后拒绝覆盖并保留草稿。索引有未保存变更时禁用索引页刷新，避免丢失修改；整表刷新沿用既有刷新行为。
- 主键继续由字段页管理。约束支撑的唯一索引允许重命名；结构及删除应通过约束 SQL 处理。已有表达式、条件、INCLUDE、自定义操作符类/方法、解析器、不可见性、分区等特殊属性保留原定义，允许安全重命名；网格不将其悄悄简化为普通索引。

内存：最多 1,024 个索引，元数据及草稿有大小限制，网格只绘制可见行，整个索引页复用一个文本编辑器并释放撤销状态。组合字段选择器按需建立，关闭即释放。没有修改旧配置格式或连接文件。

参考：[Navicat 索引类型和方法下拉](https://www.navicat.com/en/company/aboutus/blog/1765-the-impact-of-database-indexes-on-write-operations.html)、[MySQL 8.4 CREATE INDEX](https://dev.mysql.com/doc/refman/8.4/en/create-index.html)、[PostgreSQL 17 CREATE INDEX](https://www.postgresql.org/docs/17/sql-createindex.html)。

验证记录见本轮发布目录 validation。Oracle 无可用隔离服务器时只报告 SQL 生成验证，不宣称实际服务器验收；特殊索引结构的图形编辑并未实现。


1542 成品验证：595 项默认回归通过、82 项按条件忽略；22 项 MySQL 8.4.11 / PostgreSQL 17.11 隔离真实数据库测试以及两种完整设计器界面流程通过。六项索引、十项选择与拖选、七项字段输入状态及静态基线共 24 个原生窗口通过并正常退出，行高保持 30px；开发版与成品的字段下拉截图逐像素相同。这些原生输入来自应用自有夹具，不是人工系统鼠标验收；不据此宣称任意 Java/Navicat 画面完全相同。严格 clippy、格式、全目标构建、CLI 自检、图标及内嵌适配器检查通过。隔离数据库启动默认超时的一轮已正常清理，延长启动等待后的完整重跑通过；早期失败和最终通过的记录均保留。

源码冻结 `designer-index-1542`，2,289 个生产源文件未变化。旧配置的并发更新已保留，原 Java 工程和旧发布文件没有被本次覆盖；发布包不包含用户配置。EXE 的 SHA-256 为 `0BCCF4ECE258793CA50BE3E1EDE2AA7D41CDD3DF67358BFBB3958353DC0F5B38`。空闲内存配对数据见 performance.md；验证原始记录随便携包 validation/designer-index 提供。
