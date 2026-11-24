//! Sidebar table actions reuse the actor and dependent-tab refresh used by the object page.
use super::*;
use crate::native_dialog::message::{self, Answer, Button, Kind, Message, Role};
use tomato_core::{
    database_objects::{Mutation, Request, Target},
    workbench::{DatabaseAction, DatabaseOutput},
};
pub(super) struct Dialog {
    id: String,
    connection: Connection,
    request: Request,
    focus: bool,
    notice: Option<String>,
    inline: bool,
}
impl TomatoApp {
    pub(super) fn confirm_tree_objects(&mut self, connection: Connection, request: Request) {
        if request.targets.is_empty() {
            return;
        }
        self.tree_objects = Some(Box::new(Dialog {
            id: Connection::new("TOOL").id,
            connection,
            request,
            focus: true,
            notice: None,
            inline: false,
        }));
    }
    pub(super) fn rename_tree_object(
        &mut self,
        connection: Connection,
        table: tomato_core::workbench::TableInfo,
        name: String,
    ) -> anyhow::Result<()> {
        let schema = table.table.schema.clone();
        let request = Request {
            targets: vec![Target {
                table: table.table,
                view: table.view,
            }],
            mutation: Mutation::Rename { name },
        };
        let draft = Box::new(Dialog {
            id: Connection::new("TOOL").id,
            connection: connection.clone(),
            request: request.clone(),
            focus: false,
            notice: None,
            inline: true,
        });
        let id = self.queue.submit(Command::Database {
            connection,
            action: DatabaseAction::ObjectMutation { schema, request },
        })?;
        self.pending.insert(
            id,
            Pending {
                tab_id: draft.id.clone(),
                purpose: Purpose::TreeObjects(draft),
            },
        );
        Ok(())
    }
    pub(super) fn tree_objects_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.tree_objects.take() else {
            return;
        };
        let (title, content, label, kind) = if let Some(notice) = &draft.notice {
            ("操作失败", notice.clone(), "确定", Kind::Error)
        } else {
            let count = draft.request.targets.len();
            match draft.request.mutation {
                Mutation::Clear => (
                    "清空确认",
                    format!(
                        "确定要清空选中的 {count} 张表的数据吗？\n此操作将删除所有数据（DELETE FROM），不可恢复！"
                    ),
                    "确认清空",
                    Kind::Warning,
                ),
                Mutation::Truncate => (
                    "截断确认",
                    format!(
                        "确定要截断选中的 {count} 张表吗？\n此操作将删除所有数据（TRUNCATE TABLE），不可恢复，且会重置自增列！"
                    ),
                    "确认截断",
                    Kind::Warning,
                ),
                Mutation::Drop => {
                    let tables = draft
                        .request
                        .targets
                        .iter()
                        .filter(|target| !target.view)
                        .count();
                    let views = count - tables;
                    let counts = [(tables, "张表"), (views, "个视图")]
                        .into_iter()
                        .filter(|(n, _)| *n > 0)
                        .map(|(n, unit)| format!("{n} {unit}"))
                        .collect::<Vec<_>>()
                        .join("、");
                    (
                        "确认删除",
                        format!("确定要删除选中的 {counts}吗？此操作不可恢复！"),
                        "确认删除",
                        Kind::Confirmation,
                    )
                }
                Mutation::Rename { .. } => return,
            }
        };
        let buttons = [
            Button {
                label,
                role: if draft.notice.is_some() {
                    Role::Ok
                } else {
                    Role::Other
                },
            },
            Button {
                label: "取消",
                role: Role::Cancel,
            },
        ];
        let answer = message::show(
            ctx,
            Message {
                id: egui::ViewportId::from_hash_of(("tree-objects", &draft.id)),
                owner: ctx.input(|i| i.viewport().inner_rect),
                title,
                header: None,
                content: &content,
                kind,
                buttons: if draft.notice.is_some() {
                    &buttons[..1]
                } else {
                    &buttons
                },
                active: true,
            },
            &mut draft.focus,
            &mut self.icons,
            |_, _| {},
        );
        match answer {
            None => self.tree_objects = Some(draft),
            Some(Answer::Button(0)) if draft.notice.is_none() => {
                let schema = draft.request.targets[0].table.schema.clone();
                let command = Command::Database {
                    connection: draft.connection.clone(),
                    action: DatabaseAction::ObjectMutation {
                        schema,
                        request: draft.request.clone(),
                    },
                };
                match self.queue.submit(command) {
                    Ok(id) => {
                        self.pending.insert(
                            id,
                            Pending {
                                tab_id: draft.id.clone(),
                                purpose: Purpose::TreeObjects(draft),
                            },
                        );
                    }
                    Err(error) => {
                        draft.notice = Some(error.to_string());
                        draft.focus = true;
                        self.tree_objects = Some(draft);
                    }
                }
            }
            Some(_) => {
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
        }
    }
    pub(super) fn tree_objects_reply(
        &mut self,
        mut draft: Box<Dialog>,
        result: anyhow::Result<Output>,
    ) {
        let report = match result {
            Ok(Output::Database(DatabaseOutput::ObjectsChanged(report))) => {
                draft.notice = (!report.failures.is_empty()).then(|| {
                    report
                        .failures
                        .iter()
                        .map(|(table, error)| format!("{}: {error}", table.name))
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                Some(report)
            }
            Ok(_) => {
                draft.notice = Some("表操作任务返回类型错误".into());
                None
            }
            Err(error) => {
                draft.notice = Some(error.to_string());
                None
            }
        };
        let schema = draft.request.targets[0].table.schema.clone();
        if let Some(report) = report {
            self.receive_object_changes(&draft.id, &draft.connection, &draft.request, &report);
        }
        if draft.inline {
            self.reload_tree_names(draft.connection.clone(), schema);
        } else {
            self.refresh_tree_tables(draft.connection.clone(), schema);
        }
        if let Some(notice) = &draft.notice {
            if draft.inline {
                self.status = format!("重命名失败：{notice}");
            } else {
                draft.focus = true;
                self.tree_objects = Some(draft);
            }
        }
    }

    pub(super) fn update_object_tree(
        &mut self,
        connection: &Connection,
        mutation: &Mutation,
        schema: &str,
        succeeded: &std::collections::HashSet<&str>,
    ) {
        use tomato_core::workbench::{TableId, TableInfo};
        let rename = match mutation {
            Mutation::Rename { name } => Some(name),
            Mutation::Drop => None,
            Mutation::Clear | Mutation::Truncate => return,
        };
        if succeeded.is_empty() {
            return;
        }
        let key = |name: &str| {
            db_tree::Row {
                node: db_tree::Node::Table(
                    connection,
                    &TableInfo {
                        table: TableId {
                            schema: schema.into(),
                            name: name.into(),
                        },
                        view: false,
                    },
                ),
                depth: 0,
            }
            .key()
        };
        for old in succeeded {
            let old_key = key(old);
            let new_key = rename.map(|name| key(name));
            if self.selected_connection.as_deref() == Some(old_key.as_str()) {
                self.selected_connection.clone_from(&new_key);
            }
            if self.sql_selection.remove(&old_key)
                && let Some(new_key) = &new_key
            {
                self.sql_selection.insert(new_key.clone());
            }
            if self.sql_anchor.as_deref() == Some(old_key.as_str()) {
                self.sql_anchor = new_key;
            }
        }
        let changed = |item: &TableInfo| {
            item.table.schema == schema && succeeded.contains(item.table.name.as_str())
        };
        if let Some(tables) = self
            .sql_hosts
            .get_mut(&connection.id)
            .and_then(|host| host.tables_for_mut(connection, schema))
            .filter(|tables| tables.iter().any(changed))
        {
            let tables = std::sync::Arc::make_mut(tables);
            if let Some(name) = rename {
                for item in tables.iter_mut().filter(|item| changed(item)) {
                    item.table.name.clone_from(name);
                }
            } else {
                tables.retain(|item| !changed(item));
            }
        }
    }
}
pub(super) fn targets(
    rows: &[db_tree::Row<'_>],
    selected: &std::collections::BTreeSet<String>,
    connection: &Connection,
    fallback: &tomato_core::workbench::TableInfo,
) -> Vec<Target> {
    let mut targets = rows
        .iter()
        .filter_map(|row| match &row.node {
            db_tree::Node::Table(c, table)
                if c.id == connection.id
                    && c.text("database") == connection.text("database")
                    && table.table.schema == fallback.table.schema
                    && selected.contains(&row.key()) =>
            {
                Some(Target {
                    table: table.table.clone(),
                    view: table.view,
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        targets.push(Target {
            table: fallback.table.clone(),
            view: fallback.view,
        });
    }
    targets
}
#[cfg(test)]
mod tests;
