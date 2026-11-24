# 1696 后续 Oracle 实际验收（1698）

本记录补充同一份正式优化版 1696 的实际引擎验证，没有修改运行代码，也没有生成或替换另一份 EXE。GUI、CLI、便携包和安装器的 SHA256 再次核对一致；发布来源仍为 `migration-1696-final-v1`。3,107 个冻结文件曾逐字节核对一致；本次仅补充验收文档与清单。

## 已通过的实际流程

使用自建、只监听回环的 Oracle XE21 与 Linux OpenSSH，以及最终 1696 CLI，独立验证 8 个场景：直连和 SSH 的首次断线目录读取、两种路由的原生数据页、SSH 首次连接中断后的单次写入、直连脚本中断停止，以及 SSH 中断后连接恢复成功和失败。服务端独立观察者确认已提交记录只有一条，后续语句没有执行；用户写入不自动重放。预期失败命令的退出码 1 按失败场景进行断言，不被算作成功 SQL。

CLI 已确认中断前结果数量保留；Oracle 原生窗口完整结果缓冲和显式事务丢失不从这些场景推断。MySQL/PostgreSQL 的真实后台与原生窗口流程继续沿用 1696 的独立证据。8 项 Oracle 验收不加到 967 项工作区测试统计中。

证据：[实际恢复验收](../target/oracle-xe-fixture-1698/cli-recovery-validation-v3.json)、[源码只读核对](../target/migration-1698-source-readonly-audit.json)、[最终汇总](../target/migration-1698-validation-complete.json)。

## 仍未通过的项目

Oracle 完整复制、跨连接传输和备份还原尚未通过实际组合验收。正式 CLI 的 `DBMS_METADATA.GET_DDL` 仍超过 300 秒；隔离恢复副本内的 SQLPlus `GET_XML` 与 `GET_DDL` 也未在各自 150 秒内结束。首次本地所有者连接遇到数据库尚未打开，已按实际错误和返回内容判失败，保留原始记录及断言更正，未将仅有结束提示的输出计为成功。上述情况没有被当作客户端修复，也不能据此确定唯一根因。

统计收集未完成。诊断期间仅在专有可写副本里临时关闭两个自动维护客户端，随后在恢复副本内四次启用调用均完成。状态查询读回验证为 `false`；恢复脚本、输出与查询时限记录均保留。三份本次创建的虚拟机全部停止，各自只读父镜像的大小和修改时间不变。没有修改 Windows 功能、Java 项目或共享连接配置。

证据：[DDL 超时](../target/oracle-xe-fixture-1698/ddl-fetch-after-pause.json)、[本地内容核对](../target/oracle-xe-fixture-1698-recovery-v2/local-metadata-content-verified.json)、[维护恢复](../target/oracle-xe-fixture-1698-recovery-v2/maintenance-restored.json)、[资源退出](../target/oracle-xe-fixture-1698-recovery-v2/cleanup.json)。

MIT Kerberos 的 Linux 实环境、物理设备与完整 Java 界面对照仍按迁移清单保留；本机 WSL 可选组件不可用，未安装或启用它。全应用迁移尚未标记为全部验收完成。正式可用交付仍是 [1696](migration-1696.md)。
