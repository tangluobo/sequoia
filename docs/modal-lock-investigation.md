# 主界面无响应与后台弹窗排查（2026-10-24）

## 用户运行实例

本次目标是 `target/qa-candidate-table-sql-1493/tomato.exe`，PID 44508，
本机时间 10:09:50 启动。排查中没有结束或重启它，也没有修改它的进程内存或保存配置。
另一个安装目录的 Tomato 是 JavaFX 程序，其日志不能作为 Rust 实例故障证据。

只读窗口枚举时，Rust 根窗口可见、启用、未最小化、未被 DWM 隐藏，
没有应用原生对话框，也没有系统菜单、移动/缩放循环或鼠标捕获。
鼠标所在点属于该 Rust 根窗口。Windows 等待链没有报告循环；
主线程一次寄存器采样停在 `NtUserMsgWaitForMultipleObjectsEx`，
栈顶是 `MsgWaitForMultipleObjectsEx`。这说明采样时在等窗口消息，
不能据此排除之前或其他线程的阻塞。

控件树中的搜索框和大部分按钮启用，一次 Tab 输入后焦点落在表格区域。
截图工具超时，按控件点击则报告 `coordinate input geometry is unavailable`，
因此没有通过自动鼠标操作确认用户所述的单元格编辑/打开连接失败。
后续截图时工具报告用户按 Escape 停止电脑操作，随后不再发送窗口输入。
这些证据不能证明该运行实例由隐藏弹窗锁住，也不能证明它已经恢复。

## 源码中的独立缺陷

主界面原来检查所有标签中的设计表、对象浏览器、工具等弹窗状态，
只要任意标签存在这样的状态就禁用整个根 UI。
这些弹窗却随所属面板绘制，而面板只绘制当前活动标签。
后台数据库任务失败时会给不活动的对象浏览器或设计表设置 notice，
因而可能形成“根 UI 禁用、弹窗未绘制、无法切回所属标签”的状态。
开发面板隐藏时残留的模态状态有同类问题。

修复应让随面板绘制的弹窗只在其面板实际显示时阻塞根 UI，
保留后台通知，切回标签后显示原来的弹窗；可见弹窗仍保持模态。
独立于面板、每帧在根窗口绘制的对话框（包括查询结果对话框）继续全局阻塞。
这是一项源码缺陷修复，不等同于已确认用户运行实例的本次根因。

修复前的隔离 UI 回归 `background_sql_notices_do_not_lock_the_visible_workspace`
实际失败：前台页面点击设置后 `section` 仍为 0，预期为 4。
夹具不创建数据库会话，也不读写用户的连接文件。
修复后的三个回归均通过：后台对象/设计表通知、隐藏工具编辑器、
后台标签的全局查询筛选对话框。前两项不再锁住未显示所属面板的页面，
访问所属 SQL 标签后原通知能显示并关闭；第三项继续全局模态。

最终 `cargo test --workspace --locked -j 2 -- --test-threads=1` 正常退出：
595 项通过、0 失败、82 项按环境条件忽略。格式检查、
`cargo clippy --workspace --all-targets --locked -- -D warnings` 和
指定隔离配置路径的 CLI self-check 均通过。
首轮和重试构建出现过生成目标文件缺失的 LNK1181/LNK1104 输出，保留在
`target/modal-lock-workspace-tests*.log`；最终完成的日志为
`target/modal-lock-workspace-tests-no-incremental.log`。
该次重试显式设置 `CARGO_INCREMENTAL=0`，但项目原本已经关闭增量编译，
因此不能把最终成功归因为关闭增量缓存。
已直接运行并通过生成的工作区测试程序中的三个回归；之后取消了重复的单包编译，
减少同机内存压力，没有结束用户的应用实例。

常规 release 构建的最终 ThinLTO 优化持续二十多分钟仍未完成，停止的仅是本次自有编译任务。
随后尝试仅对最终目标指定 `-C lto=off`，但与原来的 LTO 依赖不兼容，
出现 LNK2019/LNK1107，未生成候选 EXE。这是本次构建方式选择不当，不作为源码缺陷证据。
没有修改仓库的 release 配置，也不提供旧 EXE 充当修复版。

交付程序采用仓库默认开发配置，取自最终源码工作区测试构建出的
`target/qa-active-200/debug/tomato.exe`；取消了排队中的重复开发构建。
单独复制为 `target/qa-candidate-modal-scope-20261024/tomato-dev.exe`，
157,958,656 字节，SHA256 `EB250A05A1AA364BD3963D3E3162936F2168A5E61E492547E97A95C43F2F5231`。
它只作为开发验证候选，不作为正式 release；不以它的内存值代替 release 性能结果。

空配置启动成功，得到 1200×900 帧并正常自行退出（退出码 0），
证据为同目录 `smoke.json`、`smoke.png` 和 `smoke.stderr.log`。
没有注入系统输入，也没有连接保存主机。
使用 `packaging/measure-memory.ps1` 的独立空配置探针，自动图形后端，
预热 15 秒、采样 10 秒，平均工作集 **155.74 MiB**、平均私有提交 **190.75 MiB**。
这是开发候选的空闲单次测量，不代表活动连接峰值，不与此前 release 样本混作优化结论。
探针只结束其自行创建的进程；原用户 PID 44508 的 10:09:50 启动时间未变。
程序改名为 `tomato-dev.exe`，探针的 `otherInstancesAtStart` 只统计同名进程，
该值为 0 不代表系统没有用户的 `tomato.exe` 实例。

编译期间工作区另有索引下拉源码更新，未覆盖这些修改。
最终使用 `--target-dir target/qa-active-200` 对当前源码再次运行工作区测试和严格 Clippy，
以及格式检查：仍为 595 项通过、0 失败、82 项忽略，全部检查正常退出。
`final-source-manifest.json` 记录 718 个应用/库 Rust 源码及根清单/锁文件的哈希，
复验后逐项一致。此清单不是包含 vendor/资产的完整源码归档。
最终证据为 `target/modal-lock-final-source-tests.log`、
`target/modal-lock-final-source-clippy.log` 和 `target/modal-lock-final-source-fmt.log`。
候选目录的 `validation.json` 汇总测试、开发构建限制、启动及内存结果。
