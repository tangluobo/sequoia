//! Original Java readable text formats, built once per successful refresh.
use serde_json::Value;
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}
fn line(out: &mut String, value: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(value);
}
fn fallback(value: String, empty: &str) -> String {
    if value.is_empty() {
        empty.into()
    } else {
        value
    }
}
pub(super) fn format(root: &Value) -> [String; 4] {
    let mut ports = String::new();
    if let Some(bindings) = root
        .pointer("/HostConfig/PortBindings")
        .and_then(Value::as_object)
    {
        for (port, bindings) in bindings {
            if let Some(bindings) = bindings.as_array() {
                for binding in bindings {
                    if !binding.is_object() {
                        continue;
                    }
                    let ip = text(binding, "HostIp");
                    let host = text(binding, "HostPort");
                    line(
                        &mut ports,
                        &format!(
                            "{}:{} -> {port}",
                            if ip.is_empty() { "0.0.0.0" } else { ip },
                            if host.is_empty() { "?" } else { host }
                        ),
                    );
                }
            }
        }
    }
    let mut env = String::new();
    if let Some(values) = root.pointer("/Config/Env").and_then(Value::as_array) {
        for value in values.iter().filter_map(Value::as_str) {
            line(&mut env, value);
        }
    }
    let mut mounts = String::new();
    if let Some(values) = root.get("Mounts").and_then(Value::as_array) {
        for value in values.iter().filter(|v| v.is_object()) {
            let mode = text(value, "Mode");
            let mode = if mode.is_empty() {
                if value.get("RW").and_then(Value::as_bool) == Some(true) {
                    "rw"
                } else {
                    "ro"
                }
            } else {
                mode
            };
            let kind = text(value, "Type");
            line(
                &mut mounts,
                &format!(
                    "{} -> {} ({mode}{})",
                    text(value, "Source"),
                    text(value, "Destination"),
                    if kind.is_empty() {
                        String::new()
                    } else {
                        format!(", {kind}")
                    }
                ),
            );
        }
    }
    let mut network = String::new();
    for (label, path) in [
        ("网络模式", "/HostConfig/NetworkMode"),
        ("主机名", "/Config/Hostname"),
        ("IP地址", "/NetworkSettings/IPAddress"),
    ] {
        if let Some(value) = root
            .pointer(path)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            line(&mut network, &format!("{label}: {value}"));
        }
    }
    if let Some(values) = root
        .pointer("/NetworkSettings/Networks")
        .and_then(Value::as_object)
    {
        for (name, value) in values {
            let ip = text(value, "IPAddress");
            if !ip.is_empty() {
                line(&mut network, &format!("网络[{name}] IP: {ip}"));
            }
        }
    }
    [
        fallback(ports, "（无端口映射）"),
        fallback(env, "（无）"),
        fallback(mounts, "（无挂载）"),
        fallback(network, "（无网络信息）"),
    ]
}
