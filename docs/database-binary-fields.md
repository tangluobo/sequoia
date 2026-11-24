# 打开表时二进制十六进制长度错误

MySQL 8.4.11 的隔离数据库复现了用户报告的“二进制字段的十六进制长度无效”。表格原来统一执行 `HEX(column)`，但 MySQL 将 `BIT` 参数视为数值：1、15、256 分别返回 `1`、`F`、`100`。这些结果既不是完整字节，也丢失了字段声明宽度；按二进制解码时因此报错。错误随具体字段值变化，所以不是每次打开都会出现。

表格分页和数据传输现在共享同一个读取投影：MySQL 二进制列使用 `HEX(CAST(column AS BINARY))`。真实数据库验证 `BIT(9)` 的 0、1、15、256 分别保留为 `0000`、`0001`、`000F`、`0100`，`BIT(64)` 保留八个字节。没有对错误字符串补零或放宽十六进制解码，其他二进制字段仍严格保留原始字节，空二进制与 NULL 分开处理。

原 Java 只读参照为 `DatabaseService.java:630` 的分页读取及 `:4517` 的传输类型读取：表格通过 JDBC 获取字段，传输对 BINARY/VARBINARY/BLOB 读取原始字节。此次错误来自 Rust 适配层自行生成的数值 HEX 投影，未改动 Java 工程或表格界面布局。

同一回归还检查了后续保存：BIT 通过验证位宽的无符号整数参数写入，显式固定参数为字符类型后再转换为无符号整数，避免 MySQL 预处理参数推断拒绝合法的 20 位整数。无主键行定位沿用同样的精确整数比较，防止超过 2^53 的相邻 BIT(64) 值经过浮点比较。查询结果原有的十进制 BIT 显示保持兼容，回写不会误当成 ASCII 二进制。普通 BLOB、VARBINARY、BINARY 的写入方式不变。

`crates/services/tests/database_servers/binary_columns.rs` 在独占回环数据库检查分页、文字修改、二进制修改、带/不带主键定位、查询结果回写、非法位宽拒绝、相邻最大整数定位、流式复制及导出 SQL 重新导入。只创建 UUID 命名的测试库，断开所有库作用域会话后删除测试库。读取仍保留现有分页和内存预算，数据传输仍逐行读取、每 256 行提交，没有新增常驻缓存。

`scripts/qa/mysql-binary.ps1` 启动并验证自有 MySQL 进程；可额外传入 EXE，通过正常打开表控制器、后台数据库 actor 和原生窗口读取同一类真实字段，保存帧缓冲及逐字节结果证明。独立空配置仅指向 127.0.0.1，不读取或改写用户保存的数据库连接。

开发验证（2026-10-23）：

- 原始 MySQL 返回值见 `target/database-fixtures/probes/mysql-bit-before-1180/values.tsv`，同时记录错误的数值 HEX 和正确的二进制投影。
- `target/database-fixtures/probes/mysql-binary-regressions-1191` 的四项真实数据库回归全部通过，包括新增的完整二进制读写回归，以及既有查询回写、Connector/J 类型对照和流式传输回归。
- `target/binary-columns-checks-1189.json` 记录格式、严格全目标 Clippy、522 项默认测试、全目标构建及 CLI 自检通过；75 项依环境条件忽略，新二进制回归在上述隔离数据库单独执行。
- `target/database-fixtures/probes/mysql-binary-dev-1198` 从正常打开表控制器加载真实数据，五行、六个二进制列逐字节匹配，NULL 和空数组区分正确。1200×900 原生帧缓冲人工查看无错误弹窗，正常退出。
- 生产检查点 `binary-columns-1192` 冻结 2186 个文件，相对前版修改四个数据库适配文件，另外三个 UI 文件仅增加受隔离配置及回环地址约束的原生验收入口。见 `target/binary-columns-scope-1192.json`。没有修改图标、表格布局或旧配置。

官方语义参照：[MySQL 8.4 字符串函数 HEX](https://dev.mysql.com/doc/refman/8.4/en/string-functions.html)。

最终优化成品为 `target/qa-candidate-binary-columns-1194/tomato.exe`，98,600,960 字节，SHA-256 `5B8B8C46C20D70774C79E566861C9A86CAC14DA004BB6A78438E269B68649E46`。`target/database-fixtures/probes/mysql-binary-release-1195/proof.json` 记录同一 EXE 的正常控制器、真实数据库加载和正常退出。五行数据及六个二进制列逐字节匹配，1200×900 原生 PNG 与开发验收图字节相同，记录在 `target/binary-columns-framebuffer-1201.json`。成品图标、生产源码冻结及八项保护文件校验通过，旧连接、设置、下载历史和前版程序保持不变，四个用户实例保留。

与前版 1172 同批空闲测量：自动后端工作集 179.26 → 172.63 MiB，私有提交 252.91 → 255.18 MiB；Vulkan 工作集 111.55 → 114.55 MiB，私有提交 190.60 → 187.84 MiB。两版使用独立空配置、15 秒预热及十个一秒样本。结果相近且两个指标方向不同，不据此推断活动表格内存峰值下降。记录为 `target/window-probes/binary-columns-memory-1196/validation.json`；总验收记录为 `target/binary-columns-acceptance-1197.json`，成品目录另有 `build-info.json` 和图标校验记录。
