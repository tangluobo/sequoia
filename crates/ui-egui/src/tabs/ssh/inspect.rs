//! One cached presentation of inspect data; raw JSON is released after conversion.
mod details;
use serde_json::Value;
use tomato_services::terminal::ssh::tools::management::docker::{Limits, memory_text};
pub struct Inspector {
    pub id: String,
    pub name: String,
    pub info: [String; 6],
    pub details: [String; 4],
    pub policy: String,
    pub retries: u32,
    pub retry_input: String,
    pub memory: String,
    pub cpus: String,
    pub shares: String,
    pub limits: Limits,
}
impl Inspector {
    pub fn new(id: String, value: Value) -> Self {
        let number = |path: &str| {
            value
                .pointer(path)
                .and_then(Value::as_u64)
                .unwrap_or_default()
        };
        let string = |path: &str| {
            value
                .pointer(path)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let mut policy = string("/HostConfig/RestartPolicy/Name");
        if !["no", "on-failure", "unless-stopped", "always"].contains(&policy.as_str()) {
            policy = "no".into();
        }
        let name = string("/Name");
        let status = string("/State/Status");
        let mut command = string("/Config/Path");
        if let Some(args) = value.pointer("/Config/Args").and_then(Value::as_array) {
            for arg in args.iter().filter_map(Value::as_str) {
                if !command.is_empty() {
                    command.push(' ');
                }
                command.push_str(arg);
            }
        }
        let limits = Limits::read(&value);
        let info = [
            name.clone(),
            string("/Id"),
            string("/Config/Image"),
            if status.is_empty() {
                string("/Status")
            } else {
                status
            },
            string("/Created"),
            command,
        ];
        let nano = number("/HostConfig/NanoCpus");
        let shares = number("/HostConfig/CpuShares");
        Self {
            id,
            name: name.trim_start_matches('/').into(),
            info,
            details: details::format(&value),
            policy,
            retries: number("/HostConfig/RestartPolicy/MaximumRetryCount").min(100) as u32,
            retry_input: number("/HostConfig/RestartPolicy/MaximumRetryCount")
                .min(100)
                .to_string(),
            memory: memory_text(limits.memory),
            cpus: if nano == 0 {
                String::new()
            } else {
                (nano as f64 / 1_000_000_000.).to_string()
            },
            shares: if shares == 0 {
                String::new()
            } else {
                shares.to_string()
            },
            limits,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_container_sections_and_unlimited_fields_are_preserved() {
        let value = serde_json::json!({"Name":"/web","Id":"abc","Config":{"Image":"alpine","Path":"sh","Args":["-c","echo 中文"],"Env":["A=B","C=D"],"Hostname":"web"},"HostConfig":{"PortBindings":{"80/tcp":[{"HostIp":"","HostPort":"8080"}]},"Memory":536870912,"MemorySwap":1073741824,"NetworkMode":"bridge"},"Mounts":[{"Source":"/data","Destination":"/app","RW":true,"Type":"bind"}],"NetworkSettings":{"Networks":{"bridge":{"IPAddress":"172.17.0.2"}}}});
        let inspector = Inspector::new("web".into(), value);
        assert_eq!(inspector.name, "web");
        assert_eq!(inspector.info[5], "sh -c echo 中文");
        assert_eq!(inspector.memory, "512m");
        assert!(inspector.cpus.is_empty() && inspector.shares.is_empty());
        assert_eq!(inspector.policy, "no");
        assert_eq!(inspector.details[0], "0.0.0.0:8080 -> 80/tcp");
        assert_eq!(inspector.details[1], "A=B\nC=D");
        assert_eq!(inspector.details[2], "/data -> /app (rw, bind)");
        assert!(inspector.details[3].contains("网络[bridge] IP: 172.17.0.2"));
        let empty = Inspector::new("empty".into(), Value::Null);
        assert!(empty.memory.is_empty());
        assert_eq!(
            empty.details,
            ["（无端口映射）", "（无）", "（无挂载）", "（无网络信息）"]
        );
    }
}
