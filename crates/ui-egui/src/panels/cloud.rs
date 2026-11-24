use super::remote_table::{button, table, text};
use super::*;
use crate::tabs::cloud::{Page, State};
use tomato_services::cloud::{Action, Record, certificate::renewal, certificate_domain};
pub fn render(ui: &mut Ui, tab: &mut Tab, catalog: &[tomato_core::Connection]) -> Option<Intent> {
    let Panel::Cloud { connection, view } = &mut tab.panel else {
        return None;
    };
    let mut action = None;
    let mut command = None;
    if view.certificate.is_none() {
        action = view.followup.take();
        if view.certificate_refresh && !tab.busy {
            view.certificate_refresh = false;
            if matches!(view.page, Page::Domains | Page::Records(_)) {
                action = view.action();
            }
        }
    }
    if !view.loaded && action.is_none() && view.certificate.is_none() {
        action = view.action();
    }
    ui.spacing_mut().item_spacing.y = 0.;
    ui.add_enabled_ui(!tab.busy && view.certificate.is_none(), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.;
            let title = match &view.page {
                Page::Products => connection.name.clone(),
                Page::Instances(_) => "云服务器实例".into(),
                Page::Domains => "域名管理".into(),
                Page::Records(domain) => format!("子域名（解析记录）- {domain}"),
                Page::Properties => "属性".into(),
            };
            ui.label(RichText::new(title).size(13.).strong());
            if matches!(view.page, Page::Records(_)) {
                if button(ui, "申请证书", false).clicked()
                    && let Page::Records(domain) = &view.page
                {
                    let domain = domain.clone();
                    view.certificate(&domain, &domain, connection.text("certificateEmail"));
                }
                if button(ui, "添加", false).clicked() {
                    view.record = Some(Record {
                        rr: "@".into(),
                        kind: "A".into(),
                        ttl: 600,
                        priority: 1,
                        line: "default".into(),
                        ..Default::default()
                    });
                }
            }
            if matches!(view.page, Page::Instances(_)) {
                ui.label("地域：");
                ui.add_sized(
                    [160., 24.],
                    TextEdit::singleline(&mut view.region).hint_text("全部地域"),
                );
            }
            if button(ui, "刷新", false).clicked() {
                action = view.action();
            }
            ui.label(&view.status);
        });
        ui.separator();
        match &view.page {
            Page::Products => {
                for (code, label) in tomato_services::cloud::products(connection.kind()) {
                    if ui.selectable_label(false, *label).double_clicked() {
                        view.page = if matches!(*code, "ecs" | "cvm") {
                            Page::Instances((*code).into())
                        } else if matches!(*code, "domain" | "dnspod") {
                            Page::Domains
                        } else {
                            Page::Properties
                        };
                        view.loaded = false;
                        view.rows.clear();
                        view.detail = if matches!(*code, "ecs" | "cvm" | "domain" | "dnspod") {
                            String::new()
                        } else {
                            "此云产品在原 Java 中未提供数据操作".into()
                        };
                        break;
                    }
                }
            }
            Page::Properties => {
                ui.add(
                    TextEdit::multiline(&mut view.detail)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            }
            Page::Instances(_) => {
                if let Some((i, event)) = table(
                    ui,
                    &view.rows,
                    &mut view.selected,
                    &[
                        ("实例名称", "instanceName", 180.),
                        ("实例ID", "instanceId", 180.),
                        ("地域", "region", 120.),
                        ("状态", "status", 90.),
                        ("公网IP", "publicIp", 140.),
                        ("内网IP", "privateIp", 140.),
                    ],
                    30.,
                    &[],
                    &["属性"],
                ) && matches!(event.as_str(), "打开" | "属性")
                {
                    view.detail = serde_json::to_string_pretty(&view.rows[i]).unwrap_or_default();
                    view.page = Page::Properties;
                }
            }
            Page::Domains => {
                if let Some((i, event)) = table(
                    ui,
                    &view.rows,
                    &mut view.selected,
                    &[
                        ("域名", "domainName", 300.),
                        ("实例ID", "instanceId", 240.),
                        ("状态", "status", 100.),
                        ("证书状态", "certificateStatus", 145.),
                        ("证书到期时间", "certificateExpiry", 178.),
                        ("操作", "@actions", 90.),
                    ],
                    30.,
                    &["申请证书"],
                    &["打开", "申请证书", "刷新"],
                ) {
                    let domain = text(&view.rows[i], "domainName");
                    match event.as_str() {
                        "打开" if !domain.is_empty() => {
                            view.page = Page::Records(domain);
                            view.loaded = false;
                            view.rows.clear();
                            view.selected = None;
                        }
                        "申请证书" if !domain.is_empty() => {
                            view.certificate(&domain, &domain, connection.text("certificateEmail"));
                        }
                        "刷新" => action = view.action(),
                        _ => {}
                    }
                }
            }
            Page::Records(domain) => {
                let domain = domain.clone();
                if let Some((i, event)) = table(
                    ui,
                    &view.rows,
                    &mut view.selected,
                    &[
                        ("主机记录", "rr", 120.),
                        ("记录类型", "type", 70.),
                        ("记录值", "value", 200.),
                        ("TTL", "ttl", 55.),
                        ("解析线路", "line", 80.),
                        ("状态", "status", 55.),
                        ("DDNS", "ddns", 60.),
                        ("证书状态", "certificateStatus", 145.),
                        ("证书到期时间", "certificateExpiry", 178.),
                        ("操作", "@actions", 210.),
                    ],
                    34.,
                    &["申请证书", "修改", "删除"],
                    &["申请证书", "修改", "删除"],
                ) {
                    let row = &view.rows[i];
                    let record = serde_json::from_value::<Record>(row.clone()).unwrap_or_default();
                    match event.as_str() {
                        "修改" | "打开" => view.record = Some(record),
                        "删除" => view.delete = Some(record),
                        "DDNS" => {
                            action = Some(Action::SetDdns {
                                domain,
                                record,
                                enabled: !row["ddns"].as_bool().unwrap_or(false),
                            })
                        }
                        "申请证书" => {
                            let name = certificate_domain(&record.rr, &domain);
                            view.certificate(&name, &domain, connection.text("certificateEmail"));
                        }
                        _ => {}
                    }
                }
            }
        }
    });
    if tab.busy {
        ui.spinner();
    }
    record_dialog(ui.ctx(), view, tab.busy, &mut action);
    certificate_dialog(
        ui.ctx(),
        connection,
        view,
        catalog,
        tab.busy,
        &mut command,
        &mut tab.error,
    );
    if let Some(record) = &view.delete {
        let record = record.clone();
        let mut remove = false;
        let mut cancel = false;
        egui::Window::new("确认删除")
            .id(ui.id().with("delete-record"))
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(format!(
                    "确定删除解析记录 {} ({})？",
                    record.rr, record.kind
                ));
                ui.horizontal(|ui| {
                    remove = ui
                        .add_enabled(!tab.busy, egui::Button::new("确定"))
                        .clicked();
                    cancel = ui.button("取消").clicked();
                });
            });
        if cancel {
            view.delete = None;
        }
        if remove && let Page::Records(domain) = &view.page {
            action = Some(Action::DeleteRecord {
                domain: domain.clone(),
                id: record.record_id,
            });
        }
    }
    if let Some(command) = command {
        return Some(Intent::command(command));
    }
    if !tab.busy
        && view.certificate.is_none()
        && let Some(action) = action
    {
        // Initialization errors wait for explicit refresh instead of replaying every frame.
        view.loaded = true;
        view.pending = Some(action.clone());
        return Some(Intent::command(Command::Cloud {
            ddns_path: Some(view.ddns_path.clone()),
            connection: connection.clone(),
            action,
        }));
    }
    None
}

fn record_dialog(ctx: &egui::Context, view: &mut State, busy: bool, action: &mut Option<Action>) {
    let Some(record) = &mut view.record else {
        return;
    };
    let mut apply = false;
    let mut cancel = false;
    let mut open = true;
    egui::Window::new(if record.record_id.is_empty() {
        "添加解析记录"
    } else {
        "修改解析记录"
    })
    .id(egui::Id::new("cloud-record-editor"))
    .default_pos(ctx.content_rect().center() - egui::vec2(230., 170.))
    .open(&mut open)
    .collapsible(false)
    .resizable(false)
    .default_width(460.)
    .show(ctx, |ui| {
        ui.add_enabled_ui(!busy, |ui| {
            egui::Grid::new("dns-fields")
                .spacing([10., 10.])
                .show(ui, |ui| {
                    ui.label("主机记录：");
                    ui.text_edit_singleline(&mut record.rr);
                    ui.end_row();
                    ui.label("记录类型：");
                    egui::ComboBox::from_id_salt("dns-type")
                        .selected_text(&record.kind)
                        .show_ui(ui, |ui| {
                            for t in [
                                "A",
                                "AAAA",
                                "CNAME",
                                "MX",
                                "TXT",
                                "NS",
                                "SRV",
                                "CAA",
                                "PTR",
                                "显性URL",
                                "隐性URL",
                            ] {
                                ui.selectable_value(&mut record.kind, t.into(), t);
                            }
                        });
                    ui.end_row();
                    ui.label("记录值：");
                    ui.add_sized([320., 60.], TextEdit::multiline(&mut record.value));
                    ui.end_row();
                    ui.label("TTL：");
                    ui.add(egui::DragValue::new(&mut record.ttl).range(1..=604800));
                    ui.end_row();
                    ui.label("解析线路：");
                    egui::ComboBox::from_id_salt("dns-line")
                        .selected_text(&record.line)
                        .show_ui(ui, |ui| {
                            for t in ["default", "telecom", "unicom", "mobile", "oversea", "hk"] {
                                ui.selectable_value(&mut record.line, t.into(), t);
                            }
                        });
                    ui.end_row();
                    if record.kind == "MX" {
                        ui.label("优先级：");
                        ui.add(egui::DragValue::new(&mut record.priority).range(1..=65535));
                        ui.end_row();
                    }
                });
            ui.horizontal(|ui| {
                apply = button(ui, "确定", true).clicked();
                cancel = button(ui, "取消", false).clicked();
            });
        });
    });
    if apply && let Page::Records(domain) = &view.page {
        *action = Some(Action::SaveRecord {
            domain: domain.clone(),
            record: record.clone(),
        });
    }
    if cancel || !open {
        view.record = None;
    }
}
fn certificate_dialog(
    ctx: &egui::Context,
    connection: &tomato_core::Connection,
    view: &mut State,
    catalog: &[tomato_core::Connection],
    busy: bool,
    command: &mut Option<Command>,
    error: &mut Option<String>,
) {
    let Some(dialog) = &mut view.certificate else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    let mut reload_buckets = false;
    let stores: Vec<_> = catalog
        .iter()
        .filter(|c| matches!(c.kind(), "S3" | "ALIYUN_OSS"))
        .collect();
    let hosts: Vec<_> = catalog
        .iter()
        .filter(|c| matches!(c.kind(), "SSH" | "SFTP"))
        .collect();
    if dialog.selected_storage.is_empty()
        && let Some(c) = stores.first()
    {
        dialog.selected_storage = c.id.clone();
    }
    let r = &mut dialog.request;
    let form_height = (ctx.content_rect().height() - 160.).clamp(96., 620.);
    egui::Window::new("申请 TLS 证书")
        .id(egui::Id::new("certificate-request"))
        .default_pos(ctx.content_rect().center() - egui::vec2(280., 310.))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(560.)
        .show(ctx, |ui| {
            ui.label(format!("为 {} 申请证书（DNS-01 自动验证）", r.domain));
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(form_height)
                .show(ui, |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        egui::Grid::new("certificate-fields")
                            .spacing([10., 10.])
                            .show(ui, |ui| {
                                ui.label("证书颁发机构：");
                                egui::ComboBox::from_id_salt("certificate-ca")
                                    .selected_text(&r.certificate_authority)
                                    .show_ui(ui, |ui| {
                                        for s in [
                                            "Let's Encrypt（90 天）",
                                            "ZeroSSL（90 天，需 EAB）",
                                            "Google Trust Services（90 天，需 EAB）",
                                            "SSL.com（最长 200 天，需 EAB）",
                                        ] {
                                            ui.selectable_value(
                                                &mut r.certificate_authority,
                                                s.into(),
                                                s,
                                            );
                                        }
                                    });
                                ui.end_row();
                                ui.label("邮箱：");
                                ui.text_edit_singleline(&mut r.email);
                                ui.end_row();
                                let eab = !r.certificate_authority.starts_with("Let's Encrypt");
                                ui.label("EAB KID：");
                                ui.add_enabled(eab, TextEdit::singleline(&mut r.eab_kid));
                                ui.end_row();
                                ui.label("EAB HMAC：");
                                ui.add_enabled(
                                    eab,
                                    TextEdit::singleline(&mut r.eab_hmac).password(true),
                                );
                                ui.end_row();
                                ui.label("域名：");
                                ui.add_enabled(
                                    !r.domain.starts_with("*."),
                                    egui::Checkbox::new(
                                        &mut r.wildcard,
                                        format!("同时包含 *.{}", r.domain),
                                    ),
                                );
                                ui.end_row();
                                ui.label("证书续期：");
                                ui.checkbox(&mut dialog.auto_renew, "到期前三天自动续期");
                                ui.end_row();
                                ui.label("自动部署：");
                                if ui.checkbox(&mut dialog.auto_deploy, "签发和续期后通过 SSH 发布").changed()
                                {
                                    dialog.storage_copy = !dialog.auto_deploy;
                                }
                                ui.end_row();
                                if dialog.auto_deploy {
                                    ui.label("SSH 主机：");
                                    egui::ComboBox::from_id_salt("certificate-ssh")
                                        .selected_text(hosts.iter().find(|c| c.id == dialog.deployment.ssh_id).map_or("请选择 SSH 主机", |c| c.name.as_str()))
                                        .show_ui(ui, |ui| {
                                            for c in &hosts {
                                                ui.selectable_value(&mut dialog.deployment.ssh_id, c.id.clone(), &c.name);
                                            }
                                        });
                                    ui.end_row();
                                    ui.label("远程目录：");
                                    ui.add_sized([322., 24.], TextEdit::singleline(&mut dialog.deployment.directory).hint_text("例如 /etc/nginx/certs/example.com"));
                                    ui.end_row();
                                    ui.label("部署后命令：");
                                    ui.add_sized([322., 24.], TextEdit::singleline(&mut dialog.deployment.reload_command).hint_text("留空则仅发布文件"));
                                    ui.end_row();
                                    ui.label("");
                                    ui.add_sized([322., 36.], egui::Label::new("部署路径：current/fullchain.pem 和 current/privkey.pem").wrap());
                                    ui.end_row();
                                }
                                ui.label("对象存储：");
                                ui.checkbox(&mut dialog.storage_copy, "保存一份到 S3/OSS");
                                ui.end_row();
                                if dialog.storage_copy {
                                ui.label("S3/OSS：");
                                egui::ComboBox::from_id_salt("certificate-storage")
                                    .selected_text(
                                        stores
                                            .iter()
                                            .find(|c| c.id == dialog.selected_storage)
                                            .map_or("请选择存储连接", |c| c.name.as_str()),
                                    )
                                    .show_ui(ui, |ui| {
                                        for c in &stores {
                                            ui.selectable_value(
                                                &mut dialog.selected_storage,
                                                c.id.clone(),
                                                &c.name,
                                            );
                                        }
                                    });
                                ui.end_row();
                                ui.label("Bucket：");
                                ui.horizontal(|ui| {
                                    ui.add_sized([210., 24.], TextEdit::singleline(&mut r.bucket));
                                    if dialog.bucket_task.is_some() {
                                        ui.spinner();
                                    }
                                    reload_buckets = ui
                                        .add_enabled(
                                            dialog.bucket_task.is_none()
                                                && !dialog.selected_storage.is_empty(),
                                            egui::Button::new("刷新").small(),
                                        )
                                        .on_hover_text("从存储连接读取 Bucket 列表")
                                        .clicked();
                                    egui::ComboBox::from_id_salt("certificate-bucket")
                                        .selected_text("选择")
                                        .show_ui(ui, |ui| {
                                            for s in &dialog.buckets {
                                                ui.selectable_value(&mut r.bucket, s.clone(), s);
                                            }
                                        });
                                });
                                ui.end_row();
                                ui.label("保存目录：");
                                ui.text_edit_singleline(&mut r.prefix);
                                ui.end_row();
                                ui.label("服务器类型：");
                                ui.vertical(|ui| {
                                    for line in [
                                        ["Nginx", "Apache", "Tomcat (PFX)"],
                                        ["IIS (PFX)", "Java JKS", "通用 PEM"],
                                    ] {
                                        ui.horizontal(|ui| {
                                            for s in line {
                                                let mut checked =
                                                    r.server_types.iter().any(|v| v == s);
                                                if ui.checkbox(&mut checked, s).changed() {
                                                    r.server_types.retain(|v| v != s);
                                                    if checked {
                                                        r.server_types.push(s.into());
                                                    }
                                                }
                                            }
                                        });
                                    }
                                });
                                ui.end_row();
                                ui.label("输出方式：");
                                ui.horizontal(|ui| {
                                    ui.radio_value(&mut r.zip_package, true, "文件（ZIP 压缩包）");
                                    ui.radio_value(&mut r.zip_package, false, "目录");
                                });
                                ui.end_row();
                                ui.label("证书密码：");
                                ui.add(
                                    TextEdit::singleline(&mut r.keystore_password).password(true),
                                );
                                ui.end_row();
                                }
                                ui.label("");
                                ui.checkbox(&mut dialog.tos, "我同意所选证书颁发机构的服务条款");
                                ui.end_row();
                            });
                    });
                    if let Some(control) = &view.control {
                        if let Some(logs) = control.read(|p| p.logs.clone()) {
                            egui::ScrollArea::vertical()
                                .max_height(100.)
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    for l in logs {
                                        ui.label(l);
                                    }
                                });
                        }
                        if busy && ui.button("取消申请").clicked() {
                            control.cancel();
                        }
                    }
                    if dialog.storage_copy && stores.is_empty() {
                        ui.colored_label(
                            Color32::from_rgb(200, 70, 70),
                            "请先创建一个 S3 或阿里云 OSS 连接。",
                        );
                    }
                    if dialog.storage_copy && let Some(error) = &dialog.bucket_error {
                        ui.colored_label(
                            Color32::from_rgb(200, 70, 70),
                            format!("{error}。可手动填写 Bucket。"),
                        );
                    }
                });
            if let Some(error) = &dialog.issue_error {
                ui.add(
                    egui::Label::new(RichText::new(error).color(Color32::from_rgb(200, 70, 70)))
                        .selectable(true)
                        .wrap(),
                );
            } else if let Some(progress) = view.control.as_ref().and_then(|control| {
                control
                    .read(|progress| progress.logs.back().cloned())
                    .flatten()
            }) {
                ui.label(progress);
            }
            ui.separator();
            ui.horizontal(|ui| {
                let reason = if busy {
                    Some("正在处理，请稍候")
                } else if !dialog.storage_copy && !dialog.auto_deploy {
                    Some("请选择自动部署或 S3/OSS 保存")
                } else if dialog.auto_deploy && !hosts.iter().any(|c| c.id == dialog.deployment.ssh_id) {
                    Some("请选择保存的 SSH 主机")
                } else if dialog.auto_deploy && !renewal::valid_directory(&dialog.deployment.directory) {
                    Some("请填写有效的远程绝对目录")
                } else if dialog.storage_copy && !stores.iter().any(|c| c.id == dialog.selected_storage) {
                    Some("请选择 S3 或 OSS 存储连接")
                } else if dialog.storage_copy && dialog.buckets_for != dialog.selected_storage {
                    Some("正在切换存储连接")
                } else if !r.email.contains('@') {
                    Some("请填写证书申请邮箱")
                } else if dialog.storage_copy && r.bucket.trim().is_empty() {
                    Some("请选择或填写 Bucket")
                } else if dialog.storage_copy && r.server_types.is_empty() {
                    Some("请至少选择一种服务器类型")
                } else if eab_required(r)
                    && (r.eab_kid.trim().is_empty() || r.eab_hmac.trim().is_empty())
                {
                    Some("当前证书机构需要 EAB KID 和 EAB HMAC")
                } else if !dialog.tos {
                    Some("请同意证书颁发机构的服务条款")
                } else {
                    None
                };
                let response = ui.add_enabled(reason.is_none(), egui::Button::new("申请"));
                apply = response.clicked();
                cancel = ui.button("取消").clicked();
                if let Some(reason) = reason {
                    ui.label(reason);
                }
            });
        });
    if cancel || !open {
        if let Some(c) = &view.control {
            c.cancel();
        }
        view.certificate = None;
        return;
    }
    if apply {
        dialog.issue_error = None;
        let mut request = r.clone();
        if !dialog.storage_copy {
            request.bucket.clear();
            request.prefix.clear();
            request.keystore_password.clear();
        }
        let notify = ctx.clone();
        let control = tomato_services::backup::Control::new(move || notify.request_repaint());
        view.control = Some(control.clone());
        *error = None;
        *command = Some(Command::Certificate {
            connection: connection.clone(),
            request: Box::new(request),
            renewal: Some((
                view.ddns_path.with_file_name("connections.json"),
                renewal::Options {
                    auto_renew: dialog.auto_renew,
                    deployment: dialog.auto_deploy.then(|| dialog.deployment.clone()),
                },
            )),
            control: Some(control),
        });
        return;
    }
    if !dialog.storage_copy {
        return;
    }
    let Some(storage) = stores.iter().find(|c| c.id == dialog.selected_storage) else {
        return;
    };
    let storage_changed = dialog.buckets_for != storage.id;
    if storage_changed {
        r.bucket = storage.text("bucket").trim().into();
        r.storage = (**storage).clone();
        dialog.buckets_for = storage.id.clone();
        dialog.buckets.clear();
        dialog.bucket_task = None;
        dialog.bucket_error = None;
        dialog.issue_error = None;
    }
    let lookup = !busy
        && (reload_buckets
            || (storage_changed && r.bucket.is_empty() && !storage.text("password").is_empty()));
    if !lookup {
        return;
    }
    let mut storage = (**storage).clone();
    if storage.text("password").is_empty() && r.storage.id == storage.id {
        storage.set("password", r.storage.text("password"));
    }
    dialog.bucket_error = None;
    *command = Some(Command::CloudBuckets {
        connection: storage,
    });
}
fn eab_required(r: &tomato_services::cloud::certificate::Request) -> bool {
    !r.certificate_authority.starts_with("Let's Encrypt")
}
