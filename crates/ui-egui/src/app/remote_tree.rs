//! Cloud/MQ connection tree controller. Stable keys keep folding and tab reuse independent.
use super::*;
use serde_json::{Value, json};
#[derive(Clone, Debug)]
pub(super) enum Kind {
    Product(String),
    Topics,
    Groups,
    Cluster,
    Domain(String),
    Instance,
    Topic(String),
    Messages(String),
    Group(String),
    Broker(String),
}
#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub key: String,
    pub parent: Option<String>,
    pub name: String,
    pub icon: &'static str,
    pub kind: Kind,
    pub data: Value,
}
impl Entry {
    pub fn folder(&self) -> bool {
        matches!(
            self.kind,
            Kind::Product(_) | Kind::Topics | Kind::Groups | Kind::Cluster | Kind::Topic(_)
        )
    }
}
pub(super) struct Properties {
    pub title: String,
    pub rows: Vec<(String, String)>,
}
pub(super) struct Host {
    pub connection: Connection,
    pub generation: String,
    pub ready: bool,
    pub busy: bool,
    pub entries: Vec<Entry>,
}
impl Host {
    fn new(connection: Connection) -> Self {
        Self {
            connection,
            generation: Connection::new("TOOL").id,
            ready: false,
            busy: false,
            entries: Vec::new(),
        }
    }
}
fn key(c: &Connection, parent: Option<&str>, name: &str) -> String {
    format!(
        "remote:{}:{}",
        c.id,
        serde_json::to_string(&(parent, name)).unwrap_or_default()
    )
}
fn entry(
    c: &Connection,
    parent: Option<&str>,
    name: &str,
    icon: &'static str,
    kind: Kind,
    data: Value,
) -> Entry {
    Entry {
        key: key(
            c,
            parent,
            &match &kind {
                Kind::Instance => tomato_services::cloud::text(&data, "instanceId"),
                Kind::Broker(address) if !address.is_empty() => address.clone(),
                Kind::Broker(_) => format!(
                    "{}:{}",
                    tomato_services::cloud::text(&data, "host"),
                    tomato_services::cloud::text(&data, "port")
                ),
                _ => name.into(),
            },
        ),
        parent: parent.map(str::to_owned),
        name: name.into(),
        icon,
        kind,
        data,
    }
}
impl TomatoApp {
    pub(super) fn remote_changed(&mut self, c: Connection, groups: bool) {
        let parent = self
            .remote_hosts
            .get(&c.id)
            .and_then(|h| {
                h.entries.iter().find(|e| {
                    if groups {
                        matches!(e.kind, Kind::Groups)
                    } else {
                        matches!(e.kind, Kind::Topics)
                    }
                })
            })
            .cloned();
        if let Some(parent) = parent {
            self.remote_load(c, Some(parent));
        }
    }
    pub(super) fn remote_open(&mut self, c: Connection, force: bool) {
        let id = c.id.clone();
        let ready = self.remote_hosts.get(&id).is_some_and(|h| h.ready);
        if ready && !force {
            if !self.collapsed.remove(&id) {
                self.collapsed.insert(id);
            }
            return;
        }
        self.remote_hosts
            .entry(id)
            .or_insert_with(|| Host::new(c.clone()));
        self.remote_load(c, None);
    }
    fn remote_load(&mut self, c: Connection, parent: Option<Entry>) {
        let host = self
            .remote_hosts
            .entry(c.id.clone())
            .or_insert_with(|| Host::new(c.clone()));
        if host.busy {
            return;
        }
        let command = match parent.as_ref().map(|p| &p.kind) {
            None if matches!(c.kind(), "ALIYUN" | "TENCENT_CLOUD") => Command::Cloud {
                ddns_path: None,
                connection: c.clone(),
                action: tomato_services::cloud::Action::Verify,
            },
            None | Some(Kind::Topics) => Command::MessageQueue {
                connection: c.clone(),
                action: tomato_services::mq::Action::Topics,
                control: None,
            },
            Some(Kind::Groups) => Command::MessageQueue {
                connection: c.clone(),
                action: tomato_services::mq::Action::Groups,
                control: None,
            },
            Some(Kind::Cluster) => Command::MessageQueue {
                connection: c.clone(),
                action: tomato_services::mq::Action::Cluster,
                control: None,
            },
            Some(Kind::Product(p)) => Command::Cloud {
                ddns_path: None,
                connection: c.clone(),
                action: if matches!(p.as_str(), "ecs" | "cvm") {
                    tomato_services::cloud::Action::Instances {
                        region: String::new(),
                    }
                } else if matches!(p.as_str(), "domain" | "dnspod") {
                    tomato_services::cloud::Action::Domains
                } else {
                    self.collapsed.remove(&parent.as_ref().unwrap().key);
                    return;
                },
            },
            _ => return,
        };
        match self.queue.submit(command) {
            Ok(job) => {
                host.busy = true;
                self.pending.insert(
                    job,
                    Pending {
                        tab_id: String::new(),
                        purpose: Purpose::RemoteTree {
                            connection: c.id,
                            generation: host.generation.clone(),
                            parent,
                        },
                    },
                );
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }
    pub(super) fn remote_reply(
        &mut self,
        id: &str,
        generation: &str,
        parent: Option<&Entry>,
        result: anyhow::Result<Output>,
    ) {
        let Some(host) = self
            .remote_hosts
            .get_mut(id)
            .filter(|h| h.generation == generation)
        else {
            return;
        };
        host.busy = false;
        let output = match result {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(format!("加载失败：{e:#}"));
                return;
            }
        };
        let c = &host.connection;
        let (rows, cloud) = match output {
            Output::Cloud(v) => (v.rows, true),
            Output::MessageQueue(v) => (v.rows, false),
            _ => return,
        };
        if parent.is_none() {
            host.entries.clear();
            host.ready = true;
            if cloud {
                for (code, label) in tomato_services::cloud::products(c.kind()) {
                    let e = entry(
                        c,
                        None,
                        label,
                        "images/connect/tree-icons/cloud-product.svg.png",
                        Kind::Product((*code).into()),
                        Value::Null,
                    );
                    self.collapsed.insert(e.key.clone());
                    host.entries.push(e);
                }
            } else {
                for (label, icon, kind) in [
                    (
                        "主题",
                        "images/connect/tree-icons/topic.svg.png",
                        Kind::Topics,
                    ),
                    (
                        "消费者组",
                        "images/connect/tree-icons/consumer.svg.png",
                        Kind::Groups,
                    ),
                    (
                        "集群信息",
                        "images/connect/tree-icons/cluster.svg.png",
                        Kind::Cluster,
                    ),
                ] {
                    let e = entry(c, None, label, icon, kind, Value::Null);
                    self.collapsed.insert(e.key.clone());
                    host.entries.push(e);
                }
            }
            self.collapsed.remove(id);
            return;
        }
        let parent = parent.unwrap();
        let parent_key = &parent.key;
        let mut removed = std::collections::HashSet::from([parent_key.clone()]);
        loop {
            let count = removed.len();
            for n in &host.entries {
                if n.parent.as_ref().is_some_and(|p| removed.contains(p)) {
                    removed.insert(n.key.clone());
                }
            }
            if removed.len() == count {
                break;
            }
        }
        host.entries
            .retain(|e| e.key == *parent_key || !removed.contains(&e.key));
        for v in rows {
            let text = |k| tomato_services::cloud::text(&v, k);
            let e = match &parent.kind {
                Kind::Product(p) if matches!(p.as_str(), "ecs" | "cvm") => entry(
                    c,
                    Some(parent_key),
                    &format!(
                        "{} ({}, {})",
                        text("instanceName"),
                        text(if c.kind() == "ALIYUN" {
                            "regionId"
                        } else {
                            "region"
                        }),
                        text("status")
                    ),
                    "images/connect/tree-icons/compute.svg.png",
                    Kind::Instance,
                    v,
                ),
                Kind::Product(_) => entry(
                    c,
                    Some(parent_key),
                    &text("domainName"),
                    "images/connect/tree-icons/domain.svg.png",
                    Kind::Domain(text("domainName")),
                    v,
                ),
                Kind::Topics => entry(
                    c,
                    Some(parent_key),
                    &text("topic"),
                    "images/connect/tree-icons/topic.svg.png",
                    Kind::Topic(text("topic")),
                    v,
                ),
                Kind::Groups => entry(
                    c,
                    Some(parent_key),
                    &text("group"),
                    "images/connect/tree-icons/consumer.svg.png",
                    Kind::Group(text("group")),
                    v,
                ),
                Kind::Cluster => entry(
                    c,
                    Some(parent_key),
                    &format!(
                        "{} ({})",
                        text("brokerName"),
                        text(if c.kind() == "KAFKA" {
                            "host"
                        } else {
                            "address"
                        })
                    ),
                    "images/connect/tree-icons/broker.svg.png",
                    Kind::Broker(text("address")),
                    v,
                ),
                _ => continue,
            };
            if let Kind::Topic(name) = &e.kind {
                let child = entry(
                    c,
                    Some(&e.key),
                    "消息",
                    "images/connect/tree-icons/message.svg.png",
                    Kind::Messages(name.clone()),
                    json!({"topic":name}),
                );
                host.entries.push(child);
                self.collapsed.insert(e.key.clone());
            }
            host.entries.push(e);
        }
        self.collapsed.remove(parent_key);
    }
    pub(super) fn remote_entry(&mut self, c: Connection, e: Entry, verb: &str) {
        if matches!(e.kind, Kind::Instance) {
            let ordered = [
                ("publicIp", "公网 IP"),
                ("privateIp", "内网 IP"),
                ("instanceName", "实例名称"),
                ("instanceId", "实例 ID"),
                ("status", "状态"),
                ("region", "地域"),
                ("regionId", "地域"),
                ("zone", "可用区"),
                ("instanceType", "实例规格"),
                ("cpu", "CPU"),
                ("memory", "内存"),
                ("osName", "操作系统"),
                ("createdTime", "创建时间"),
                ("expiredTime", "到期时间"),
            ];
            self.instance_properties = Some(Properties {
                title: e.name.clone(),
                rows: ordered
                    .into_iter()
                    .filter(|(key, _)| e.data.get(*key).is_some())
                    .map(|(key, label)| (label.into(), tomato_services::cloud::text(&e.data, key)))
                    .collect(),
            });
            return;
        }
        if e.folder() && !matches!(e.kind, Kind::Topic(_)) && matches!(verb, "expand" | "refresh") {
            if verb == "expand" && !self.collapsed.contains(&e.key) {
                self.collapsed.insert(e.key);
                return;
            }
            self.remote_load(c, Some(e));
            return;
        }
        if matches!(e.kind, Kind::Topic(_)) && verb == "expand" {
            if !self.collapsed.remove(&e.key) {
                self.collapsed.insert(e.key);
            }
            return;
        }
        let source = format!(
            "{}:{}",
            e.key,
            if matches!(verb, "query" | "send") {
                verb
            } else {
                "open"
            }
        );
        if let Some(t) = self.tabs.iter_mut().find(|t| t.source_id == source) {
            self.active = Some(t.id.clone());
            if let Panel::Cloud { view, .. } = &mut t.panel
                && let Kind::Domain(domain) = &e.kind
            {
                if verb == "refresh" {
                    view.loaded = false;
                    view.followup = view.action();
                }
                if verb == "certificate" && !t.busy {
                    view.certificate(domain, domain, c.text("certificateEmail"));
                }
            }
            return;
        }
        let mut tab = match tabs::create_tab(c.clone(), self.runtime.as_ref().map(|r| r.handle())) {
            Ok(t) => t,
            Err(err) => {
                self.error = Some(format!("{err:#}"));
                return;
            }
        };
        tab.source_id = source;
        match &mut tab.panel {
            Panel::Cloud { view, .. } => {
                if let Some(path) = self.repository.location() {
                    view.ddns_path = path.with_file_name("ddns.json");
                }
                use tabs::cloud::Page;
                match &e.kind {
                    Kind::Domain(domain) => {
                        view.page = Page::Records(domain.clone());
                        if verb == "certificate" {
                            view.certificate(domain, domain, c.text("certificateEmail"));
                        }
                        tab.title = format!("子域名({domain})");
                    }
                    Kind::Instance => {
                        view.page = Page::Properties;
                        view.detail = serde_json::to_string_pretty(&e.data).unwrap_or_default();
                        view.loaded = true;
                        tab.title = format!("属性({})", e.name);
                    }
                    Kind::Product(product) => {
                        view.page = if matches!(product.as_str(), "ecs" | "cvm") {
                            Page::Instances(product.clone())
                        } else {
                            Page::Domains
                        };
                        tab.title = e.name.clone();
                    }
                    _ => return,
                }
            }
            Panel::MessageQueue { view, .. } => {
                use tabs::mq::{Page, QueryPage};
                match &e.kind {
                    Kind::Topics => view.page = Page::Topics,
                    Kind::Groups => view.page = Page::Groups,
                    Kind::Cluster => view.page = Page::Cluster,
                    Kind::Topic(name) | Kind::Messages(name) => {
                        view.topic = name.clone();
                        view.page = if matches!(e.kind, Kind::Messages(_))
                            || matches!(verb, "query" | "send")
                        {
                            Page::Messages
                        } else {
                            Page::Topics
                        };
                        if verb == "send" {
                            view.query = QueryPage::Send;
                        }
                        tab.title = name.clone();
                    }
                    Kind::Group(group) => {
                        view.page = Page::Group;
                        view.group = group.clone();
                        tab.title = format!("消费者组({group})");
                    }
                    Kind::Broker(address) => {
                        if c.kind() == "KAFKA" {
                            view.page = Page::Cluster;
                            view.detail = serde_json::to_string_pretty(&e.data).unwrap_or_default();
                        } else {
                            view.page = Page::Broker;
                            view.address = address.clone();
                        }
                    }
                    _ => return,
                }
                if verb == "new" {
                    view.create = true;
                }
                if verb == "delete" {
                    if let Kind::Topic(topic) = &e.kind {
                        view.confirm = Some(tomato_services::mq::Action::DeleteTopic {
                            topic: topic.clone(),
                        });
                    }
                    if let Kind::Group(group) = &e.kind {
                        view.confirm = Some(tomato_services::mq::Action::DeleteGroup {
                            group: group.clone(),
                        });
                    }
                }
            }
            _ => return,
        }
        self.active = Some(tab.id.clone());
        self.tabs.push(tab);
    }
}
pub(super) fn append<'a>(
    rows: &mut Vec<db_tree::Row<'a>>,
    c: &'a Connection,
    host: &'a Host,
    collapsed: &std::collections::HashSet<String>,
    filter: &str,
    depth: usize,
) {
    fn visit<'a>(
        rows: &mut Vec<db_tree::Row<'a>>,
        c: &'a Connection,
        host: &'a Host,
        parent: Option<&str>,
        collapsed: &std::collections::HashSet<String>,
        filter: &str,
        depth: usize,
    ) {
        for e in host
            .entries
            .iter()
            .filter(|e| e.parent.as_deref() == parent)
        {
            let show = filter.is_empty()
                || e.name.to_lowercase().contains(filter)
                || host.entries.iter().any(|child| {
                    child.parent.as_deref() == Some(&e.key)
                        && child.name.to_lowercase().contains(filter)
                });
            if show {
                rows.push(db_tree::Row {
                    node: db_tree::Node::Remote(c, e),
                    depth,
                });
            }
            if (!collapsed.contains(&e.key) || !filter.is_empty()) && depth < 20 {
                visit(rows, c, host, Some(&e.key), collapsed, filter, depth + 1);
            }
        }
    }
    visit(rows, c, host, None, collapsed, filter, depth + 1);
}

impl TomatoApp {
    pub(super) fn remote_properties(&mut self, ctx: &egui::Context) {
        let Some(properties) = &self.instance_properties else {
            return;
        };
        let mut closed = false;
        let icons = &mut self.icons;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("cloud-instance-properties"),
            egui::ViewportBuilder::default()
                .with_title(&properties.title)
                .with_inner_size([560., 520.])
                .with_resizable(false),
            |ctx, _| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    closed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(12))
                    .show(ctx, |ui| {
                        ui.label(RichText::new("实例属性").strong().size(15.));
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(430.)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Grid::new("cloud-instance-attributes")
                                    .spacing([16., 9.])
                                    .show(ui, |ui| {
                                        for (label, value) in &properties.rows {
                                            ui.label(
                                                RichText::new(format!("{label}："))
                                                    .strong()
                                                    .color(Color32::from_gray(85)),
                                            );
                                            ui.add_sized(
                                                [330., 24.],
                                                egui::Label::new(value).wrap(),
                                            );
                                            let response = if let Some(icon) = icons
                                                .get(ui.ctx(), "images/connect/copy_tables.png")
                                            {
                                                ui.add(
                                                    egui::Button::image(egui::Image::new((
                                                        icon,
                                                        egui::vec2(14., 14.),
                                                    )))
                                                    .frame(false),
                                                )
                                            } else {
                                                ui.button("⧉")
                                            };
                                            if response.on_hover_text("复制").clicked() {
                                                ui.ctx().copy_text(value.clone());
                                            }
                                            ui.end_row();
                                        }
                                    });
                            });
                        ui.separator();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("关闭").clicked() {
                                closed = true;
                            }
                        });
                    });
            },
        );
        if closed {
            self.instance_properties = None;
        }
    }
}
