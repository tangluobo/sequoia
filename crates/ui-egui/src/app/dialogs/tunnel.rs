use super::*;

pub(super) fn normalize(connection: &mut Connection) {
    if connection.kind() == "REDIS" && connection.flag("redisCluster") {
        connection.set("redisDatabase", 0);
    }
}

fn referenced(connection: &Connection) -> bool {
    !connection.text("sshTunnelHostId").is_empty()
        || (connection.text("sshTunnelHost").trim().is_empty()
            && matches!(
                connection.kind(),
                "SSH" | "SFTP" | "S3" | "ALIYUN_OSS" | "REDIS" | "KAFKA" | "ROCKETMQ"
            ))
}

pub(super) fn fields(ui: &mut Ui, connection: &mut Connection, saved: &[Connection]) {
    check(ui, connection, "useSshTunnel", "启用SSH通道");
    ui.add_enabled_ui(connection.flag("useSshTunnel"), |ui| {
        if referenced(connection) {
            ui.label("选择已有SSH主机：");
            let mut id = connection.text("sshTunnelHostId").to_owned();
            let label = saved.iter().find(|c| c.id == id).map(|c| {
                format!("{} ({}:{})", c.name, c.text("host"), c.number("port"))
            });
            let old = id.clone();
            egui::ComboBox::from_id_salt("ssh-tunnel-reference")
                .width(280.0)
                .selected_text(label.unwrap_or_else(|| {
                    if id.is_empty() { "选择已保存的SSH连接".into() } else { format!("找不到SSH连接：{id}") }
                }))
                .show_ui(ui, |ui| {
                    for c in saved.iter().filter(|c| matches!(c.kind(), "SSH" | "SFTP") && c.id != connection.id) {
                        ui.selectable_value(&mut id, c.id.clone(), format!("{} ({}:{})", c.name, c.text("host"), c.number("port")));
                    }
                });
            if id != old { connection.set("sshTunnelHostId", id); }
            ui.label(RichText::new(if connection.kind() == "REDIS" {
                "启用后，将通过所选SSH主机建立安全通道连接Redis；SSH连接信息随引用主机自动更新。支持单机与集群模式，集群节点通过同一SSH会话转发。"
            } else { "启用后，将通过所选SSH主机建立安全通道；SSH连接信息随引用主机自动更新。" }).size(12.0).color(Color32::from_gray(136)));
        } else {
            egui::Grid::new("ssh-tunnel-fields").spacing([10.0,10.0]).show(ui, |ui| {
                for (key,label,secret) in [
                    ("sshTunnelHost","SSH主机：",false), ("sshTunnelPort","SSH端口：",false),
                    ("sshTunnelUsername","SSH用户名：",false), ("sshTunnelPassword","SSH密码：",true)
                ] { form_field(ui, connection, key,label,secret); }
            });
            check(ui,connection,"sshTunnelUsePassword","使用密码认证");
            check(ui,connection,"sshTunnelUseKey","使用密钥认证");
            if connection.flag("sshTunnelUseKey") {
                let mut paths: Vec<String> = connection.options.get("sshTunnelPrivateKeyPaths")
                    .and_then(serde_json::Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
                let mut remove = None;
                for (index,path) in paths.iter().enumerate() {
                    ui.horizontal(|ui| { ui.label(path); if ui.small_button("移除").clicked() { remove=Some(index); } });
                }
                let mut changed = false;
                if let Some(index)=remove { paths.remove(index); changed=true; }
                if ui.button("+ 添加密钥").clicked()
                    && let Some(path)=rfd::FileDialog::new().set_title("选择 SSH 私钥").pick_file() {
                    paths.push(path.to_string_lossy().into_owned()); changed=true;
                }
                if changed { connection.set("sshTunnelPrivateKeyPaths",serde_json::json!(paths)); }
            }
            ui.label(RichText::new("启用后，将通过SSH跳板机建立安全通道连接数据库").size(12.0).color(Color32::from_gray(136)));
        }
    });
}

pub(super) fn redis(ui: &mut Ui, connection: &mut Connection) {
    ui.label("模式：");
    check(ui, connection, "redisCluster", "集群模式（Cluster）");
    ui.end_row();
    if connection.flag("redisCluster") {
        form_field(ui, connection, "redisClusterNodes", "集群节点：", false);
        ui.label("");
        ui.label(
            RichText::new("格式: host1:port1,host2:port2,... 多个节点用逗号分隔")
                .size(10.0)
                .color(Color32::from_gray(153)),
        );
        ui.end_row();
    }
    normalize(connection);
    ui.label("数据库：");
    let enabled = !connection.flag("redisCluster");
    let mut database = connection.number("redisDatabase").to_string();
    if ui
        .add_enabled(
            enabled,
            TextEdit::singleline(&mut database).desired_width(60.),
        )
        .changed()
        && let Ok(database) = database.parse::<u32>()
    {
        connection.set("redisDatabase", database);
    }
    ui.end_row();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_tunnel_fields_and_cluster_database_keep_original_semantics() {
        let mut redis = Connection::new("REDIS");
        redis.set("redisDatabase", 7);
        redis.set("future", serde_json::json!({"nested":true}));
        assert!(referenced(&redis));
        normalize(&mut redis);
        assert_eq!(redis.number("redisDatabase"), 7);
        redis.set("redisCluster", true);
        normalize(&mut redis);
        assert_eq!(redis.number("redisDatabase"), 0);
        assert_eq!(redis.options["future"], serde_json::json!({"nested":true}));
        redis.set("sshTunnelHost", "old-gateway");
        assert!(!referenced(&redis));
        redis.set("sshTunnelHostId", "saved-sftp");
        assert!(referenced(&redis));
        let mongo = Connection::new("MONGODB");
        assert!(!referenced(&mongo));
    }
}
