# 与原版共享连接配置

从 0.1.1 起，GUI 和 CLI 默认直接打开用户目录的 `.tomato/connections.json`。
Windows 路径为 `%USERPROFILE%\.tomato\connections.json`；Linux/macOS 为 `~/.tomato/connections.json`。
保留原来的连接 ID、分组 ID、父子关系、SSH 引用、数据库产品、私钥路径、已保存密码及其他未知字段，不经过“导入”时的 ID 重映射。

## 格式与保存

读取支持原版整文件 `TOMATO_ENCRYPTED` 加密数组、早期明文数组及字段密码加密、空文件、旧 Rust 工作区对象。保存统一使用原版的 AES/ECB/PKCS5Padding（AES 块下对应 PKCS7）、Base64 和 `TOMATO_ENCRYPTED` 标记；密钥截断方式与原始 SecurityUtils 相同。保护级别与原版一致，兼容加密并不等同操作系统凭据库。

启动只读取，不自动重写或创建配置、备份。首次实际修改前建立 `connections.json.before-rust.bak`；已有加密文件按字节备份，早期明文格式以兼容加密格式备份，避免新增明文凭据副本。后续保存不会覆盖该首次备份。

保存先验证连接关系，在同目录写临时文件并同步后原子替换。若从加载到保存期间检测到外部修改、删除或新建文件，则拒绝保存并提示重新打开。Java 版不参与锁协议，这种检测不是跨程序事务锁；避免两个版本同时编辑同一文件。

“导出”仍为移除密码、token、secret、access key 的 JSON，且不允许覆盖正在使用的配置。默认连接文件会加密保留已保存凭据，重启不再因为 Rust 的保存过程而丢失密码。

已迁移的全局设置沿用原版 `~/.tomato/global.json`，保留未知字段，并在实际修改时备份和检查外部修改；只打开设置不会保存。JSON 历史沿用原代码实际使用的 `~/.tomata/json_history.json`（目录名中的 tomata 是原版行为）。`development.json` 等其余配置及尚未接入的字段仍以迁移清单为准。Rust 专有连接类型（例如 SQLite）不在旧 Java 枚举中，旧版不会因此自动具备对应支持。

原版 `useSsl` 是远程桌面的通用字段，不能据此强制 Redis 或 PostgreSQL 使用 TLS。Rust 的 Redis 显式 TLS 字段为 `redisTls`；PostgreSQL 默认使用 `sslmode=prefer`，只有显式设置 `databaseTls=true` 才强制 `require`。原字段仍保留供旧程序使用。

## 隔离和检查

- 设置 `TOMATO_DATA_DIR` 后读写该目录内的 `connections.json`，用于测试或独立配置。
- CLI `--config <文件>` 可显式指定旧 Rust 的 workspace.json 等文件；首次编辑后将使用兼容的加密连接数组格式。
- `tomato-cli check-config` 只读解析并报告连接、分组数量，不输出主机、密码或访问远程服务。
- `tomato-cli connections` 输出移除敏感字段后的连接列表。

## 验证

提交的测试夹具由原始 Java SecurityUtils 生成，覆盖 Rust 解密、ID/分组/密码/未知字段读写不变、首次备份、外部修改与删除检测、损坏文件保护、导出覆盖保护。另用原始 Java SecurityUtils 成功解密 Rust 保存的隔离测试文件，验证回写格式。

本机原文件已只读识别为 59 个连接、5 个分组；检查前后 SHA-256 相同。未保存原文件，也未尝试连接其中的服务器。

0.1.8 的连接编辑、批量删除、保留子节点和逐项导入采用候选模型，文件保存成功后才发布到界面；检测到外部修改时保留原模型与草稿。导入重复项默认不选，用户可手动复制，复制会生成新 ID 和唯一名称；未选择的重复分组/跳板复用已有目标，未选择且非重复的引用清空，行为对照原 ConnectionImportPlanner。

`LOCAL_DIRECTORY` 且 `directoryType=S3` 直接沿用原 `endpoint`、`bucket`、`s3Prefix`、`region`、`pathStyleAccess`、`username`、`password`；虚拟树路径不是本地磁盘路径。未勾选“保存密钥”的输入只保存在运行时连接中，关闭应用后释放，不回写旧文件。

Hosts 分组直接使用原 `~/.tomata/hosts_groups.json`，Trae 会话直接使用原 `~/.tomata/trae_sessions.json`，保留各层未知字段。只打开面板不重写文件；首次修改保留备份，检测到外部修改则保留草稿并停止提交。Hosts 的系统文件与配置提交、Trae 的目录切换与配置提交均带失败回滚；这不构成与旧版同时写入时的跨进程事务锁。

Redis 使用原 `redisCluster`、`redisClusterNodes`、`redisDatabase` 和 ACL 用户字段。数据库标签只在运行时派生所选数据库，不为每个标签修改旧连接配置。Cluster 固定 DB0，Cluster+SSH 与原 Java 一样不支持。旧 `sshTunnelHostId` 引用和 `sshTunnelHost/Port/Username/Password/UsePassword/UseKey/PrivateKeyPaths` 内嵌跳板字段继续使用；引用可指向 SSH 或 SFTP 连接，运行时密码不写回旧配置。

对象存储私有剪贴板保留原连接 ID、类型、Bucket 和对象键，跨连接粘贴按 ID 只读查找原加密连接文件；不要求迁移连接配置，也不会为复制任务重写配置。若源连接已经删除则明确失败。未保存的运行时密钥仍只存在当前会话，不能从共享连接文件中恢复。
