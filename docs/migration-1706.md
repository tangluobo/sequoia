# 1706 Hosts 授权和弹窗交互

对照 `HostsFilePane.writeHostsFile`，Linux/macOS 写系统 Hosts 遇到权限不足时会打开 sudo 密码窗口。确认后在后台重试。密码仅供本次请求，编辑历史和请求数据中的密码分别清理与隔离。输错可重试，关闭窗口或按 Esc 可取消。重试前检查系统文件和分组配置版本，配置提交失败会尝试还原系统 Hosts。分组格式沿用旧 `.tomata/hosts_groups.json`，首次保存保留备份。

Hosts 分组和 Redis 键操作的弹窗接入主窗口交互限制。弹窗显示时，背景导航和 Hosts 编辑器快捷键不会触发。其他模块仍能独立处理自己的弹窗。

本版在 Windows 验证弹窗、状态交接、事务和真实子进程控制。隔离 Ubuntu 已实测 sudo 授权命令能写入受保护的测试文件，错误密码会被拒绝；完整 Rust 服务在 Linux 上的端到端验收仍待完成。 macOS 的系统授权实测仍待完成。

工作区 996 项测试通过，0 项失败，104 项条件忽略。Windows 成品窗口、原 Java Hosts 对保存结果的回读、Redis 五种值类型、正常退出和旧加密配置兼容通过。安装器解包校验由发布流程执行。

空配置空闲内存使用 1704 和 1706 成品配对抽样。工作集与私有提交分别记录在 `target/window-probes/migration-1706-memory-v1/validation.json`。全部应用的 Java 视觉对等、Oracle 完整复制及归档、设备场景仍按 [迁移清单](migration-backlog.json) 记录。
