use super::remote_table::{button, table, text};
use super::*;
use crate::tabs::mq::{Page, QueryPage};
use tomato_services::mq::{Action, Query};
mod info;
#[cfg(test)]
mod tests;
pub fn render(ui: &mut Ui, tab: &mut Tab, icons: &mut crate::icons::Icons) -> Option<Intent> {
    let Panel::MessageQueue { connection, view } = &mut tab.panel else {
        return None;
    };
    let rocket = connection.kind() == "ROCKETMQ";
    let mut action = view.followup.take();
    if !view.loaded && action.is_none() {
        action = view.action();
    }
    ui.spacing_mut().item_spacing.y = 0.;
    ui.add_enabled_ui(!tab.busy, |ui| {
        if matches!(view.page, Page::Topics | Page::Messages)
            && crate::native_tabs::show(
                ui,
                "mq-main-tabs",
                &mut view.page,
                crate::native_tabs::Style::MessageQueue,
                [(Page::Topics, "主题"), (Page::Messages, "消息")],
            )
        {
            view.rows.clear();
            view.loaded = false;
            action = view.action();
        }
        if view.page != Page::Messages {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.;
                if view.page != Page::Messages && button(ui, "刷新", true).clicked() {
                    action = view.action();
                }
                match view.page {
                    Page::Topics => {
                        if view.topic.is_empty() {
                            if button(ui, "创建主题", false).clicked() {
                                view.create = true;
                            }
                            if button(ui, "查看统计", false).clicked()
                                && let Some(i) = view.selected
                            {
                                view.topic = text(&view.rows[i], "topic");
                                action = Some(Action::Stats {
                                    topic: view.topic.clone(),
                                });
                            }
                            if button(ui, "删除主题", false).clicked()
                                && let Some(i) = view.selected
                            {
                                view.confirm = Some(Action::DeleteTopic {
                                    topic: text(&view.rows[i], "topic"),
                                });
                            }
                        } else {
                            ui.label(format!("Topic: {}", view.topic));
                            if button(ui, "返回列表", false).clicked() {
                                view.topic.clear();
                                view.rows.clear();
                                view.selected = None;
                                view.loaded = false;
                                action = Some(Action::Topics);
                            }
                            if rocket && button(ui, "路由信息", false).clicked() {
                                action = Some(Action::Route {
                                    topic: view.topic.clone(),
                                });
                                view.detail_dialog = true;
                            }
                        }
                    }
                    Page::Groups => {
                        if ui
                            .add_enabled_ui(!view.group_selection.rows.is_empty(), |ui| {
                                button(ui, "批量删除", false)
                            })
                            .inner
                            .clicked()
                        {
                            let groups = view
                                .group_selection
                                .rows
                                .iter()
                                .filter_map(|i| view.rows.get(*i))
                                .map(|r| text(r, "group"))
                                .collect();
                            view.confirm = Some(Action::DeleteGroups { groups });
                        }
                        ui.label("支持 Ctrl/Shift 多选");
                    }
                    Page::Group => {
                        ui.label(format!("消费者组: {}", view.group));
                    }
                    Page::Broker => {
                        ui.label(&view.address);
                    }
                    Page::Cluster => {
                        ui.label("集群信息");
                    }
                    Page::Messages => {
                        ui.label(format!("Topic: {}", view.topic));
                    }
                }
                ui.label(&view.status);
            });
            ui.separator();
        }
        let is_messages = view.page == Page::Messages;
        let split = view.page == Page::Group;
        if split {
            ui.set_max_height(ui.available_height());
        }
        let _height = if split {
            (ui.available_height() * 0.48).max(80.)
        } else {
            ui.available_height().max(80.)
        };
        let columns: Vec<(&str, &str, f32)> = match view.page {
            Page::Topics if view.topic.is_empty() => {
                if rocket {
                    vec![("主题名称", "topic", 500.), ("类型", "topicType", 150.)]
                } else {
                    vec![
                        ("主题名称", "topic", 350.),
                        ("分区数", "partitions", 100.),
                        ("副本数", "replicationFactor", 100.),
                        ("内部主题", "internal", 100.),
                    ]
                }
            }
            Page::Topics => {
                if rocket {
                    vec![
                        ("Broker", "brokerName", 180.),
                        ("队列ID", "queueId", 80.),
                        ("最小Offset", "minOffset", 140.),
                        ("最大Offset", "maxOffset", 140.),
                        ("最后更新时间", "lastUpdateTimestamp", 180.),
                    ]
                } else {
                    vec![
                        ("Partition", "partition", 100.),
                        ("最小Offset", "minOffset", 140.),
                        ("最大Offset", "maxOffset", 140.),
                        ("消息数", "messageCount", 140.),
                        ("Leader", "leader", 100.),
                    ]
                }
            }
            Page::Groups => {
                if rocket {
                    vec![("消费者组", "group", 400.), ("积压量", "diffTotal", 200.)]
                } else {
                    vec![("消费者组", "group", 400.), ("积压量", "diff", 200.)]
                }
            }
            Page::Group => vec![
                ("Topic", "topic", 200.),
                (
                    if rocket { "队列ID" } else { "Partition" },
                    if rocket { "queueId" } else { "partition" },
                    90.,
                ),
                ("Broker Offset", "brokerOffset", 150.),
                ("Consumer Offset", "consumerOffset", 150.),
                ("积压量", "diff", 110.),
            ],
            Page::Cluster => {
                if rocket {
                    vec![
                        ("Broker名称", "brokerName", 200.),
                        ("Broker ID", "brokerId", 80.),
                        ("地址", "address", 230.),
                        ("角色", "role", 100.),
                        ("版本", "version", 150.),
                    ]
                } else {
                    vec![
                        ("节点ID", "nodeId", 100.),
                        ("Host", "host", 230.),
                        ("Port", "port", 100.),
                        ("Rack", "rack", 150.),
                        ("Controller", "controller", 130.),
                    ]
                }
            }
            Page::Broker => vec![("名称", "name", 350.), ("值", "value", 450.)],
            Page::Messages if view.query == QueryPage::ConsumeStatus => vec![
                ("消费者组", "group", 300.),
                ("Topic", "topic", 200.),
                ("积压量", "diff", 120.),
            ],
            Page::Messages => {
                if rocket {
                    vec![
                        ("Message ID", "msgId", 320.),
                        ("Tags", "tags", 100.),
                        ("Keys", "keys", 150.),
                        ("队列ID", "queueId", 80.),
                        ("Offset", "queueOffset", 100.),
                        ("存储时间", "storeTimestamp", 180.),
                    ]
                } else {
                    vec![
                        ("Partition", "partition", 100.),
                        ("Offset", "offset", 120.),
                        ("Key", "key", 150.),
                        ("时间", "timestamp", 180.),
                        ("时间类型", "timestampType", 100.),
                    ]
                }
            }
        };
        let menus = if view.page == Page::Topics {
            vec!["打开", "消息查询", "发送消息", "删除主题"]
        } else if is_messages && rocket {
            vec!["消息详情", "消费轨迹", "重新消费"]
        } else {
            vec!["详情", "复制"]
        };
        let mut event = None;
        if is_messages {
            let width = ui.available_width();
            egui::Panel::left(ui.id().with("mq-message-table"))
                .default_size(width * 0.55)
                .frame(egui::Frame::NONE)
                .min_size(180.)
                // Clamp the saved divider position again after window resize.
                // Otherwise the list can leave the detail panel just 80 px wide.
                .max_size((width - (width * 0.5).min(340.)).max(180.))
                .resizable(true)
                .show(ui, |ui| {
                    message_query(ui, view, rocket, &mut action, &mut tab.error);
                    event = table(
                        ui,
                        &view.rows,
                        &mut view.selected,
                        &columns,
                        30.,
                        &[],
                        &menus,
                    );
                });
            egui::Frame::new()
                .fill(Color32::from_gray(244))
                .inner_margin(5)
                .show(ui, |ui| details(ui, view, rocket, &mut action, icons));
        } else if view.page == Page::Groups {
            event = super::remote_table::multi_table(
                ui,
                &view.rows,
                &mut view.selected,
                &columns,
                30.,
                &menus,
                &mut view.group_selection,
            );
        } else {
            event = table(
                ui,
                &view.rows,
                &mut view.selected,
                &columns,
                30.,
                &[],
                &menus,
            );
        }
        if let Some((i, event)) = event {
            let row = &view.rows[i];
            match event.as_str() {
                "打开" if view.page == Page::Topics => {
                    view.topic = text(row, "topic");
                    action = Some(Action::Stats {
                        topic: view.topic.clone(),
                    });
                }
                "打开" if view.page == Page::Groups => {
                    view.group = text(row, "group");
                    view.page = Page::Group;
                    action = view.action();
                }
                "打开" if view.page == Page::Cluster && rocket => {
                    view.address = text(row, "address");
                    view.page = Page::Broker;
                    action = view.action();
                }
                "消息查询" | "发送消息" => {
                    view.topic = text(row, "topic");
                    view.page = Page::Messages;
                    view.query = if event == "发送消息" {
                        QueryPage::Send
                    } else {
                        QueryPage::Time
                    };
                    view.rows.clear();
                    view.loaded = true;
                }
                "删除主题" => {
                    view.confirm = Some(Action::DeleteTopic {
                        topic: text(row, "topic"),
                    })
                }
                "消费轨迹" => {
                    action = Some(Action::Track {
                        topic: view.topic.clone(),
                        id: text(row, "msgId"),
                    });
                    view.detail_dialog = true;
                }
                "重新消费" => {
                    view.confirm = Some(Action::Reconsume {
                        topic: view.topic.clone(),
                        group: view.group.clone(),
                        id: text(row, "msgId"),
                    })
                }
                "复制" => ui
                    .ctx()
                    .copy_text(serde_json::to_string_pretty(row).unwrap_or_default()),
                _ => {
                    if event != "选择" || !is_messages {
                        let metadata = row
                            .as_object()
                            .map(|m| {
                                m.iter()
                                    .filter(|(k, _)| k.as_str() != "body")
                                    .map(|(k, v)| (k.clone(), v.clone()))
                                    .collect::<serde_json::Map<_, _>>()
                            })
                            .unwrap_or_default();
                        view.detail = serde_json::to_string_pretty(&metadata).unwrap_or_default();
                        let body = text(row, "body");
                        view.set_body(body);
                    }
                    if matches!(event.as_str(), "消息详情" | "详情") {
                        view.detail_dialog = true;
                    }
                }
            }
        }
        if split {
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(160.)
                .show(ui, |ui| {
                    ui.add(
                        TextEdit::multiline(&mut view.detail)
                            .code_editor()
                            .interactive(false)
                            .desired_width(f32::INFINITY),
                    );
                });
        }
    });
    if view.create {
        let mut open = true;
        let mut create = false;
        let mut cancel = false;
        egui::Window::new("创建主题")
            .default_pos(ui.ctx().content_rect().center() - egui::vec2(230., 110.))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.add_enabled_ui(!tab.busy, |ui| {
                    egui::Grid::new("mq-create")
                        .spacing([10., 10.])
                        .show(ui, |ui| {
                            ui.label("主题名称：");
                            ui.text_edit_singleline(&mut view.new_topic);
                            ui.end_row();
                            ui.label(if rocket {
                                "队列数："
                            } else {
                                "分区数："
                            });
                            ui.add(egui::DragValue::new(&mut view.partitions).range(1..=1024));
                            ui.end_row();
                            if !rocket {
                                ui.label("副本数：");
                                ui.add(egui::DragValue::new(&mut view.replicas).range(1..=100));
                                ui.end_row();
                            }
                        });
                    ui.horizontal(|ui| {
                        create = button(ui, "确定", true).clicked();
                        cancel = button(ui, "取消", false).clicked();
                    });
                });
            });
        if create {
            action = Some(Action::CreateTopic {
                topic: view.new_topic.clone(),
                partitions: view.partitions,
                replicas: view.replicas,
            });
        }
        if cancel || !open {
            view.create = false;
        }
    }
    if let Some(confirm) = &view.confirm {
        let mut apply = false;
        let mut cancel = false;
        let mut open = true;
        egui::Window::new(if matches!(confirm, Action::DeleteGroups { .. }) {
            "确认批量删除"
        } else {
            "确认操作"
        })
        .id(egui::Id::new("mq-confirm"))
        .open(&mut open)
        .default_pos(ui.ctx().content_rect().center() - egui::vec2(180., 70.))
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            match confirm {
                Action::DeleteTopic { topic } => {
                    ui.label(format!("确定删除主题 {topic}？"));
                }
                Action::DeleteGroup { group } => {
                    ui.label(format!("确定删除消费者组 {group}？"));
                }
                Action::DeleteGroups { groups } => {
                    ui.label(format!("删除 {} 个消费者组", groups.len()));
                    egui::ScrollArea::vertical()
                        .max_height(220.)
                        .show(ui, |ui| {
                            for group in groups {
                                ui.label(group);
                            }
                        });
                    ui.label("删除后不可恢复，确定要删除吗？");
                }
                Action::Reconsume { group, .. } => {
                    ui.label(format!("确定让消费者组 {group} 重新消费此消息？"));
                }
                _ => {}
            }
            ui.horizontal(|ui| {
                apply = ui
                    .add_enabled(!tab.busy, egui::Button::new("确定"))
                    .clicked();
                cancel = ui.button("取消").clicked();
            });
            cancel |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            apply |= !tab.busy
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        });
        if cancel || !open {
            view.confirm = None;
        } else if apply {
            action = view.confirm.take();
        }
    }
    if let Some((title, message)) = &view.batch_notice {
        let mut close = false;
        let mut open = true;
        egui::Window::new(title)
            .id(egui::Id::new("mq-batch-result"))
            .open(&mut open)
            .collapsible(false)
            .default_size([440., 180.])
            .show(ui.ctx(), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(300.)
                    .show(ui, |ui| {
                        ui.label(message);
                    });
                close = ui.button("确定").clicked();
                close |= ui.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                        || i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                });
            });
        if close || !open {
            view.batch_notice = None;
        }
    }
    if view.detail_dialog {
        egui::Window::new("详情")
            .open(&mut view.detail_dialog)
            .default_size([640., 480.])
            .show(ui.ctx(), |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.add(
                        TextEdit::multiline(&mut view.detail)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                });
            });
    }
    if !tab.busy
        && let Some(action) = action
    {
        // Initialization errors wait for explicit refresh instead of replaying every frame.
        view.loaded = true;
        view.pending = Some(action.clone());
        let control = tomato_services::backup::Control::default();
        view.control = Some(control.clone());
        return Some(Intent::command(Command::MessageQueue {
            connection: connection.clone(),
            action,
            control: Some(control),
        }));
    }
    None
}
fn timestamp(s: &str) -> anyhow::Result<i64> {
    if let Ok(n) = s.parse() {
        return Ok(n);
    }
    let date = chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")?;
    Ok(date
        .and_local_timezone(chrono::Local)
        .single()
        .ok_or_else(|| anyhow::anyhow!("本地时间无效或不唯一"))?
        .timestamp_millis())
}
fn make_action(v: &crate::tabs::mq::State, rocket: bool) -> anyhow::Result<Action> {
    anyhow::ensure!(!v.topic.trim().is_empty(), "请选择 Topic");
    if v.query == QueryPage::Send {
        let headers = if rocket {
            Default::default()
        } else {
            if v.headers.trim_start().starts_with('{') {
                serde_json::from_str(&v.headers)?
            } else {
                v.headers
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|line| {
                        let (k, val) = line
                            .split_once('=')
                            .ok_or_else(|| anyhow::anyhow!("Header 应为每行 key=value"))?;
                        Ok((k.trim().to_owned(), val.trim().to_owned()))
                    })
                    .collect::<anyhow::Result<std::collections::BTreeMap<_, _>>>()?
            }
        };
        return Ok(Action::Send {
            topic: v.topic.clone(),
            partition: if v.send_partition.trim().is_empty() {
                None
            } else {
                Some(v.send_partition.parse()?)
            },
            key: v.send_key.clone(),
            body: v.send_body.clone(),
            headers,
            tags: v.tags.clone(),
            delay: v.delay,
        });
    }
    if v.query == QueryPage::ConsumeStatus {
        return Ok(Action::ConsumeStatus {
            topic: v.topic.clone(),
        });
    }
    let query = match v.query {
        QueryPage::Time => Query::Time {
            begin: timestamp(&v.begin)?,
            end: timestamp(&v.end)?,
        },
        QueryPage::Offset => Query::Offset {
            partition: v.partition.parse()?,
            offset: v.offset.parse()?,
            limit: v.limit,
        },
        QueryPage::Key => Query::Key(v.key.clone()),
        QueryPage::Id => Query::Id(v.id.clone()),
        QueryPage::Unconsumed => {
            anyhow::ensure!(!v.group.is_empty(), "请选择消费者组");
            Query::Unconsumed {
                group: v.group.clone(),
                limit: v.limit,
            }
        }
        _ => unreachable!(),
    };
    Ok(Action::Messages {
        topic: v.topic.clone(),
        query,
    })
}

fn details(
    ui: &mut Ui,
    view: &mut crate::tabs::mq::State,
    rocket: bool,
    action: &mut Option<Action>,
    icons: &mut crate::icons::Icons,
) {
    ui.label(RichText::new("基本信息").size(13.).strong());
    egui::ScrollArea::vertical()
        .id_salt("mq-message-info")
        .max_height(200.)
        .show(ui, |ui| {
            info::render(ui, &view.detail, icons);
        });
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = 23.;
        ui.spacing_mut().button_padding = egui::vec2(6., 2.);
        ui.spacing_mut().item_spacing.x = 8.;
        ui.label(RichText::new("Body").strong().size(13.));
        if view.body.is_empty() {
            ui.disable();
        }
        if ui
            .add_enabled(
                view.body_container,
                egui::Button::new(RichText::new("格式化").size(11.)),
            )
            .clicked()
        {
            view.format_body(true);
        }
        if ui
            .add_enabled(
                view.body_container,
                egui::Button::new(RichText::new("压缩").size(11.)),
            )
            .clicked()
        {
            view.format_body(false);
        }
        if ui
            .add(egui::Button::new(RichText::new("复制全部").size(11.)))
            .clicked()
        {
            ui.ctx().copy_text(view.copy_body());
        }
        if ui
            .add_enabled(
                view.body_container,
                egui::Button::new(RichText::new("展开全部").size(11.)),
            )
            .clicked()
        {
            view.format_body(true);
            view.body_view.all(false);
        }
        if ui
            .add_enabled(
                view.body_container,
                egui::Button::new(RichText::new("折叠全部").size(11.)),
            )
            .clicked()
        {
            view.format_body(true);
            view.body_view.all(true);
        }
    });
    let height = (ui.available_height() - 195.).max(35.);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.painter().rect_filled(ui.max_rect(), 0., Color32::WHITE);
            if view.body.is_empty() {
                ui.painter().text(
                    ui.max_rect().left_top() + egui::vec2(24., 4.),
                    egui::Align2::LEFT_TOP,
                    "1",
                    egui::FontId::monospace(12.),
                    Color32::from_gray(153),
                );
            }
            super::json::viewer(
                ui,
                &view.body,
                &mut view.body_view,
                true,
                &mut view.body_reset,
            );
        },
    );
    ui.label(RichText::new("消费状态").strong());
    ui.horizontal(|ui| {
        if ui.button("刷新").clicked() {
            *action = Some(Action::ConsumeStatus {
                topic: view.topic.clone(),
            });
        }
        if ui
            .add_enabled(
                view.consume_selected.is_some(),
                egui::Button::new("查看未消费消息"),
            )
            .clicked()
            && let Some(i) = view.consume_selected
        {
            view.group = text(&view.consume_rows[i], "group");
            *action = Some(Action::Messages {
                topic: view.topic.clone(),
                query: Query::Unconsumed {
                    group: view.group.clone(),
                    limit: 100,
                },
            });
        }
        if rocket
            && ui
                .add_enabled(
                    view.consume_selected.is_some() && view.selected.is_some(),
                    egui::Button::new("重新消费"),
                )
                .clicked()
            && let (Some(g), Some(m)) = (view.consume_selected, view.selected)
        {
            view.confirm = Some(Action::Reconsume {
                topic: view.topic.clone(),
                group: text(&view.consume_rows[g], "group"),
                id: text(&view.rows[m], "msgId"),
            });
        }
    });
    if let Some((i, _)) = table(
        ui,
        &view.consume_rows,
        &mut view.consume_selected,
        &[("消费者组", "group", 260.), ("积压量", "diff", 100.)],
        26.,
        &[],
        &[],
    ) {
        view.group = text(&view.consume_rows[i], "group");
    }
}

fn message_query(
    ui: &mut Ui,
    view: &mut crate::tabs::mq::State,
    rocket: bool,
    action: &mut Option<Action>,
    error: &mut Option<String>,
) {
    crate::native_tabs::show(
        ui,
        "mq-query-tabs",
        &mut view.query,
        crate::native_tabs::Style::MessageQueue,
        [
            (QueryPage::Time, "按时间查询"),
            (QueryPage::Id, "按MsgId查询"),
            (QueryPage::Key, "按Key查询"),
            (QueryPage::Offset, "按Offset查询"),
            (QueryPage::Send, "发送消息"),
            (QueryPage::ConsumeStatus, "消费状态"),
            (QueryPage::Unconsumed, "未消费消息"),
        ]
        .into_iter()
        .filter(|(page, _)| {
            !matches!(page, QueryPage::ConsumeStatus | QueryPage::Unconsumed)
                && ((rocket && *page != QueryPage::Offset)
                    || (!rocket && !matches!(page, QueryPage::Id | QueryPage::Key)))
        }),
    );
    let mut query = false;
    ui.spacing_mut().item_spacing.x = 5.;
    if view.query != QueryPage::Send {
        ui.horizontal_wrapped(|ui| match view.query {
            QueryPage::Time => {
                ui.label("开始:");
                let width = ((ui.available_width() - 105.) / 2.).clamp(95., 190.);
                super::sql::results::temporal::datetime_field(
                    ui,
                    &mut view.begin,
                    &mut view.begin_picker,
                    width,
                    "mq-begin",
                );
                ui.label("结束:");
                super::sql::results::temporal::datetime_field(
                    ui,
                    &mut view.end,
                    &mut view.end_picker,
                    width,
                    "mq-end",
                );
                query = button(ui, "查询", true).clicked();
            }
            QueryPage::Offset => {
                ui.label("Partition：");
                ui.add_sized([60., 24.], TextEdit::singleline(&mut view.partition));
                ui.label("Offset：");
                ui.add_sized([120., 24.], TextEdit::singleline(&mut view.offset));
                ui.label("条数：");
                ui.add(egui::DragValue::new(&mut view.limit).range(1..=1000));
                query = button(ui, "查询", true).clicked();
            }
            QueryPage::Id => {
                ui.label("MsgId:");
                ui.add_sized(
                    [(ui.available_width() - 65.).clamp(90., 350.), 24.],
                    TextEdit::singleline(&mut view.id).hint_text("请输入MsgId"),
                );
                query = button(ui, "查询", true).clicked();
            }
            QueryPage::Key => {
                ui.label("Key：");
                ui.add_sized(
                    [(ui.available_width() - 65.).clamp(90., 350.), 24.],
                    TextEdit::singleline(&mut view.key),
                );
                query = button(ui, "查询", true).clicked();
            }
            QueryPage::Unconsumed => {
                ui.label("消费者组：");
                ui.add_sized(
                    [(ui.available_width() - 65.).clamp(90., 260.), 24.],
                    TextEdit::singleline(&mut view.group),
                );
                ui.label("条数：");
                ui.add(egui::DragValue::new(&mut view.limit).range(1..=1000));
                query = button(ui, "查询", true).clicked();
            }
            QueryPage::ConsumeStatus => {
                query = button(ui, "查询消费状态", true).clicked();
            }
            QueryPage::Send => {}
        });
    }
    if view.query == QueryPage::Time {
        ui.horizontal_wrapped(|ui| {
            ui.label("快捷:");
            for (days, label) in [(1, "近1天"), (3, "近3天"), (7, "近7天"), (30, "近1月")] {
                if ui.button(label).clicked() {
                    let now = chrono::Local::now().date_naive();
                    view.begin = (now - chrono::Duration::days(days))
                        .and_hms_opt(0, 0, 0)
                        .unwrap()
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string();
                    view.end = now
                        .and_hms_opt(23, 59, 59)
                        .unwrap()
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string();
                }
            }
        });
    }
    if view.query == QueryPage::Send {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 5.;
            ui.horizontal_wrapped(|ui| {
                if rocket {
                    ui.label("Tags:");
                    ui.add_sized([140., 24.], TextEdit::singleline(&mut view.tags));
                }
                ui.label("Key:");
                ui.add_sized([200., 24.], TextEdit::singleline(&mut view.send_key));
                if rocket {
                    ui.label("延迟级别:");
                    egui::ComboBox::from_id_salt("rocket-delay")
                        .selected_text(view.delay.to_string())
                        .show_ui(ui, |ui| {
                            for level in 0..=18 {
                                ui.selectable_value(&mut view.delay, level, level.to_string());
                            }
                        });
                } else {
                    ui.label("Partition:");
                    ui.add_sized(
                        [120., 24.],
                        TextEdit::singleline(&mut view.send_partition)
                            .hint_text("分区ID (留空=默认)"),
                    );
                }
                query = button(ui, "发送", true).clicked();
            });
            if !rocket {
                ui.label("Headers:");
                let width = ui.available_width();
                bounded_editor(
                    ui,
                    [width, 50.],
                    &mut view.headers_editor,
                    "mq-headers",
                    TextEdit::multiline(&mut view.headers)
                        .char_limit(64 * 1024)
                        .desired_rows(2)
                        .hint_text("Headers (可选，每行 key=value)"),
                );
            }
            ui.label("Body:");
            let width = ui.available_width();
            bounded_editor(
                ui,
                [width, 100.],
                &mut view.send_body_editor,
                "mq-send-body",
                TextEdit::multiline(&mut view.send_body)
                    .char_limit(2 * 1024 * 1024)
                    .hint_text("消息内容"),
            );
        });
    }
    if query {
        match make_action(view, rocket) {
            Ok(a) => *action = Some(a),
            Err(e) => *error = Some(format!("{e:#}")),
        }
    }
    ui.separator();
}

fn bounded_editor(
    ui: &mut Ui,
    size: [f32; 2],
    owner: &mut crate::tabs::text_state::Editor,
    name: &str,
    edit: TextEdit<'_>,
) -> egui::Response {
    let id = owner.prepare(ui, name);
    let response = ui.add_sized(size, edit.id(id));
    if let Some(state) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), id) {
        state.trim_undo_memory(64 * 1024);
    }
    response
}
