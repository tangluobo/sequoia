# 旧 RDP 安全层

1672 接入 `useSsl=false` 的原生 Standard Security。TLS/CredSSP 仍走原有传输，不创建旧安全层的缓冲和密码状态。
连接器根据自身状态标记已有 Security Header 的报文，避免把虚拟通道的普通负载误当成头部；适配器处理分段读写、背压、FastPath/MCS、MAC 与 4096 包密钥更新。单个传输缓冲最多 65 KiB，密钥在释放时清零。

40/56/128 位以及 Low、High、无加密使用独立 FreeRDP 3.32.2 的合成 Sample 服务验证，不启动桌面共享服务。夹具只修改绑定地址和服务器配置：绑定 127.0.0.1，并关闭 FreeRDP 对本地连接跳过加密的优化；其协议与密码实现保持原样。观察器检查实际 GCC 方法和级别，后续内容不解密、不记录。每种模式分别从关闭/保留旧 SSL 设置开始，发送超过 4100 个输入包、跨过密钥更新、确认画面再关闭。

仅服务器明确返回安全层不匹配时，按 Java 行为最多重新协商一次；证书、TLS 或认证失败不会触发降级。FIPS 不在原 Java 的实现范围，继续明确拒绝。

在 Windows 上用 Python 3.14、UCRT64 GCC、CMake 和 Ninja 运行：

```powershell
python scripts/qa/build_freerdp.py --cmake <cmake.exe> --ninja <ninja.exe> --toolchain <ucrt64/bin>
cargo test --workspace --locked --test remote_desktop --no-run --message-format=json > target/rdp-standard-artifacts.jsonl
python scripts/qa/freerdp.py --test-exe <上述输出的 executable> --toolchain <ucrt64/bin> --label own-standard-128 --strength 128
```

`--strength` 另有 `56`、`40`、`low`、`high`、`none`。每次使用新的 label。下载校验固定在脚本中，SDK 只存放于 target，不作为软件运行依赖；不读写保存连接或用户剪贴板。Windows Actions 已接入同样的六种模式；本机结果与尚未执行的 GitHub CI 结果分别记录。

最初未关闭 FreeRDP 本地优化的历史运行实际为无加密，相关记录已标明不能作为加密互通证据。有效证据为 `target/rdp-standard-interop/native-standard-*-wire-v2/validation.json`；不得用申请的设置替代实际协商值。

脚本使用 [MSYS2 Action 的安装路径输出](https://github.com/msys2/setup-msys2#outputs) 定位工具，FreeRDP 源码来自[官方发布目录](https://pub.freerdp.com/releases/)。服务器证书与测试文件均由夹具创建，不读取真实桌面。
