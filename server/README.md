# Tomato Cloud

自建的项目协作服务。Go 后端、SQLite 持久化、自带网页管理；Rust 桌面客户端通过版本化 JSON API 与增量通知同步。功能范围和研究依据见 [调研对照](../docs/collaboration-research.md)。

## 本机使用

使用发行包的 `tomato-cloud.exe`，或者在本目录执行 `go build -trimpath -o tomato-cloud.exe ./cmd/tomato-cloud`。`go.mod` 固定工具链与依赖；不需要 Java、Node.js 或外部数据库运行环境。

先创建管理员，密码从标准输入读取，不放进命令行参数：

```powershell
.\tomato-cloud.exe --data .\cloud-data --command create-admin --email admin@example.com
.\tomato-cloud.exe --data .\cloud-data --listen 127.0.0.1:8443
```

浏览器打开 `http://127.0.0.1:8443`，登录后在“用户管理”创建团队账号。Tomato 顶部“云端协作”填写同一服务器地址和账号。没有配置邮件时可由管理员建号并在项目中发送账号内邀请。

客户端可以记住多个服务器与邮箱，但不保存云密码或会话令牌；重启后重新登录，也可先打开离线副本。编辑先保存到本机队列，再后台上传。同步冲突时保留本地和远端内容，选定版本后继续同步。清理离线副本要求先退出且没有待上传修改，不会删除原连接配置。

云端连接选“接入连接树”后，可在原连接设置中填写只属于本机的数据库/SSH 密码和密钥。云端查询在原 SQL 编辑器中打开、执行并通过原保存快捷键保存。表中的数据库记录、查询结果、S3 内容和远程桌面画面不会自动上传到协作服务。

## TLS 与部署

公开监听必须使用 `--tls-cert cert.pem --tls-key key.pem`，或者在受控内网中使用 `--behind-proxy`，由反向代理提供 HTTPS。客户端拒绝非回环 HTTP 和包含密码的 URL，不跳过证书验证。

本目录提供 `Dockerfile`、`compose.yml` 和 `Caddyfile`。Compose 只暴露代理的 80/443 端口，后端使用非 root 用户、只读容器及 `/data` 持久卷。设置 `TOMATO_CLOUD_DOMAIN` 为实际域名；在 `secrets/smtp-password.txt` 创建只供部署用户读取的 SMTP 密码文件（不使用邮件时为空文件），再运行 `docker compose up -d --build`。首次管理员可用 `docker compose run --rm cloud --data /data --command create-admin --email admin@example.com` 创建。不要同时启动多个后端副本写同一个卷。

生产数据目录及备份应只允许服务账号和备份管理员读取。`encryption.key` 是对象及两步验证密钥的 AES-256-GCM 主密钥；不能只备份数据库。这里是服务端加密存储，拥有该密钥的服务器管理员具备解密能力，不声称端到端加密。

## 邮件与账号安全

默认关闭公开注册。配置下列环境变量并启用 `--registration --public-url https://cloud.example.com` 后，注册账号必须先验证邮箱：

| 变量 | 用途 |
|---|---|
| `TOMATO_CLOUD_PUBLIC_URL` | 公开 HTTPS 根地址，用于邮件链接 |
| `TOMATO_CLOUD_SMTP_ADDRESS` | 支持 STARTTLS 的 SMTP 主机和端口，例如 `smtp.example.com:587` |
| `TOMATO_CLOUD_SMTP_USERNAME` | 邮件服务用户名 |
| `TOMATO_CLOUD_SMTP_PASSWORD_FILE` | SMTP 密码文件；优先于直接环境变量 `TOMATO_CLOUD_SMTP_PASSWORD` |
| `TOMATO_CLOUD_SMTP_FROM` | 发件邮箱 |

密码使用带独立盐的 PBKDF2-SHA256（600000 次）保存；登录、邮件和敏感操作有频率限制。会话令牌仅存哈希，默认 7 天到期，可逐设备撤销。账号可启用 TOTP 两步验证并生成 10 个一次性恢复码。密码重置撤销现有会话，保持两步验证；遗失全部验证方式时，管理员可在服务器本机执行 `--command reset-password --email ...`，从标准输入设置新密码，该恢复操作同时重置两步验证。

邮件验证码链接只保存哈希：验证有效期 1 小时，密码恢复 15 分钟，使用后失效。邀请在收件账号中接受或拒绝；管理员创建的账号视为管理员已核验身份。`--local-fixture-registration` 仅供隔离回环测试使用，禁止配合代理或公开监听。

## 备份与恢复

```powershell
.\tomato-cloud.exe --data .\cloud-data --command backup --output .\backup-2026-10-24
```

输出目录必须是新目录。备份使用 SQLite 在线一致性快照，不直接复制正在使用的 WAL 数据库文件。恢复时停止服务，将快照的 `cloud.db` 和 `encryption.key` 一起放入新的私有数据目录，再以该目录启动。错误或缺失的密钥会阻止启动，不会自动生成新密钥覆盖既有数据。

## 开发与验收

```text
go vet ./...
go test -count=1 ./...
go test -race -count=1 ./...
```

Windows 的 race 构建需要可用的 C 编译器。GitHub Actions 在 Windows、Linux 和 macOS 执行服务端校验。桌面集成测试显式指定 `TOMATO_CLOUD_FIXTURE_EXE`，只启动临时回环服务；网页及原生窗口验收脚本分别在 `scripts/qa/collaboration-portal.cjs` 和 `scripts/qa/collaboration-native.py`。

客户端与服务端共享对象类型和版本语义；实现独立于 Navicat 官方账号、商业授权及专有文件编辑器。门户显示工作文件内容，不执行上传的 SQL 或打开其描述的数据库连接。详细限制见调研对照；真实 SMTP 投递、域名和证书部署需使用部署方的服务配置验证。
