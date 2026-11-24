use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ConnectionKind {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub port: u16,
    pub backend: &'static str,
}

pub const KINDS: &[ConnectionKind] = &[
    kind("SSH", "SSH", "终端", 22, "ssh"),
    kind("RDP", "远程桌面 RDP", "桌面", 3389, "rdp"),
    kind("VNC", "VNC", "桌面", 5900, "vnc"),
    kind("ADB", "Android ADB", "桌面", 5555, "adb"),
    kind("SPICE", "SPICE", "桌面", 5900, "pending"),
    kind("MYSQL", "MySQL / MariaDB", "数据库", 3306, "sql"),
    kind("POSTGRESQL", "PostgreSQL", "数据库", 5432, "sql"),
    kind("SQLITE", "SQLite", "数据库", 0, "sql"),
    kind("ORACLE", "Oracle", "数据库", 1521, "sql"),
    kind("HIVE", "Apache Hive", "数据库", 10000, "sql"),
    kind("REDIS", "Redis", "数据库", 6379, "redis"),
    kind("MONGODB", "MongoDB", "数据库", 27017, "mongo"),
    kind("FTP", "FTP", "文件", 21, "ftp"),
    kind("SFTP", "SFTP", "文件", 22, "sftp"),
    kind("S3", "S3 / MinIO", "文件", 443, "s3"),
    kind("ALIYUN_OSS", "阿里云 OSS", "文件", 443, "oss"),
    kind("ALIYUN", "阿里云", "云服务", 443, "cloud"),
    kind("TENCENT_CLOUD", "腾讯云", "云服务", 443, "cloud"),
    kind("KAFKA", "Kafka", "消息队列", 9092, "kafka"),
    kind("ROCKETMQ", "RocketMQ", "消息队列", 9876, "rocketmq"),
    kind("LOCAL_TERMINAL", "本地终端", "终端", 0, "pty"),
    kind("LOCAL_DIRECTORY", "本地目录", "文件", 0, "files"),
    kind("HTTP_SERVER", "HTTP 文件服务", "服务器", 8080, "http"),
    kind("FTP_SERVER", "FTP 文件服务", "服务器", 2121, "ftp-server"),
    kind("SMB_SERVER", "SMB 文件共享", "服务器", 445, "pending"),
    kind("TOOL", "工具", "工具", 0, "tools"),
];

const fn kind(
    id: &'static str,
    label: &'static str,
    group: &'static str,
    port: u16,
    backend: &'static str,
) -> ConnectionKind {
    ConnectionKind {
        id,
        label,
        group,
        port,
        backend,
    }
}

pub fn find_kind(id: &str) -> Option<&'static ConnectionKind> {
    KINDS.iter().find(|kind| kind.id == id)
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct DatabaseProfile {
    pub id: &'static str,
    pub label: &'static str,
    pub family: &'static str,
    pub port: u16,
}

pub const DATABASE_PROFILES: &[DatabaseProfile] = &[
    profile("mysql", "MySQL", "MYSQL", 3306),
    profile("mariadb", "MariaDB", "MYSQL", 3306),
    profile("tidb", "TiDB", "MYSQL", 4000),
    profile("oceanbase-mysql", "OceanBase (MySQL)", "MYSQL", 2881),
    profile("doris", "Apache Doris", "MYSQL", 9030),
    profile("starrocks", "StarRocks", "MYSQL", 9030),
    profile("goldendb", "GoldenDB", "MYSQL", 3306),
    profile("postgresql", "PostgreSQL", "POSTGRESQL", 5432),
    profile("kingbase", "KingbaseES", "POSTGRESQL", 54321),
    profile("opengauss", "openGauss", "POSTGRESQL", 5432),
    profile("gaussdb", "GaussDB", "POSTGRESQL", 8000),
    profile("highgo", "HighGo", "POSTGRESQL", 5866),
    profile("vastbase", "Vastbase", "POSTGRESQL", 5432),
    profile("cockroachdb", "CockroachDB", "POSTGRESQL", 26257),
    profile("redshift", "Amazon Redshift", "POSTGRESQL", 5439),
    profile("oracle", "Oracle", "ORACLE", 1521),
    profile("hive", "Apache Hive", "HIVE", 10000),
];

const fn profile(
    id: &'static str,
    label: &'static str,
    family: &'static str,
    port: u16,
) -> DatabaseProfile {
    DatabaseProfile {
        id,
        label,
        family,
        port,
    }
}

pub const TOOLS: &[(&str, &str)] = &[
    ("json_tool", "JSON 处理"),
    ("image_format_converter", "图片格式转换"),
    ("dataset_converter", "VOC → COCO"),
    ("desktop_shortcut", "桌面快捷方式"),
    ("hosts_file", "Hosts 文件"),
    ("image_background_remover", "图标制作"),
    ("trae_session", "Trae 会话"),
    ("resource_extractor", "资源提取"),
    ("port_mapping", "TCP 端口映射"),
    ("downloader", "下载器"),
    ("color_transpose_game", "颜色转置"),
    ("music_downloader", "无损音乐下载"),
    ("game_dungeons", "游戏副本管理"),
];

pub fn is_relaxation_tool(id: &str) -> bool {
    matches!(
        id,
        "color_transpose_game" | "music_downloader" | "game_dungeons"
    )
}
