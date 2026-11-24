use super::*;
use crate::tabs::queries::Document;
fn source(connection: &str, scope: &Scope, name: &str) -> anyhow::Result<String> {
    Ok(format!(
        "named-query:{connection}:{}",
        scope.file(name)?.to_string_lossy()
    ))
}
impl TomatoApp {
    pub(in crate::app) fn open_query_file(
        &mut self,
        connection: &str,
        namespace: &str,
        scope: Scope,
        name: Option<String>,
    ) -> anyhow::Result<()> {
        let identity = identity::decode(namespace)?;
        if let Some(identity) = &identity {
            anyhow::ensure!(
                identity.database == scope.database,
                "查询目录与数据库不一致"
            );
        }
        let sql_namespace = identity
            .as_ref()
            .map_or(namespace, |identity| identity.namespace);
        let source_id = name
            .as_deref()
            .map(|name| source(connection, &scope, name))
            .transpose()?;
        if let Some(existing) = source_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.source_id == id))
        {
            self.active = Some(existing.id.clone());
            return Ok(());
        }
        let host = self
            .sql_hosts
            .get(connection)
            .ok_or_else(|| anyhow::anyhow!("数据库连接已关闭"))?;
        let mut routed = host.connection.clone();
        if matches!(routed.kind(), "MYSQL" | "POSTGRESQL" | "HIVE") {
            routed = tomato_core::database_scope::select(&routed, &scope.database);
        }
        let transaction = self.sql_transaction(&routed, sql_namespace);
        let mut tab = tabs::create_tab(
            // Each editor owns its database scope; metadata vectors remain shared.
            routed,
            self.runtime.as_ref().map(|runtime| runtime.handle()),
        )?;
        let mut document = Document::new(scope.clone(), "");
        document.namespace = namespace.into();
        document.loading = name.is_some();
        document.name = name.clone();
        tab.source_id = source_id.unwrap_or_else(|| format!("new-query:{}", tab.id));
        tab.title = name.clone().unwrap_or_else(|| "未保存查询".into());
        let Panel::Sql { workbench, sql, .. } = &mut tab.panel else {
            anyhow::bail!("此连接不支持 SQL 查询")
        };
        sql.clear();
        workbench.schema = sql_namespace.into();
        let same_database = host.connection.kind() != "POSTGRESQL"
            || host.connection.text("database") == scope.database;
        workbench.schemas = if same_database {
            host.schemas.clone()
        } else {
            std::sync::Arc::new(vec![sql_namespace.into()])
        };
        workbench.tables = if same_database {
            host.tables.get(sql_namespace).cloned().unwrap_or_default()
        } else {
            Default::default()
        };
        workbench.transaction = transaction;
        workbench.document = Some(document);
        workbench.completion.focus_requested = true;
        let id = tab.id.clone();
        let metadata_connection = match &tab.panel {
            Panel::Sql { connection, .. } => connection.clone(),
            _ => unreachable!(),
        };
        if let Some(name) = name {
            self.query_command(
                connection,
                namespace,
                Request::Read { scope, name },
                JobKind::Read {
                    tab: id.clone(),
                    generation: 0,
                },
            )?;
        }
        self.active = Some(id.clone());
        self.tabs.push(tab);
        self.submit(
            &id,
            Command::Database {
                connection: metadata_connection,
                action: tomato_core::workbench::DatabaseAction::Completion {
                    schema: sql_namespace.into(),
                },
            },
            Purpose::Normal,
        );
        Ok(())
    }
    pub(in crate::app) fn request_query_save(&mut self, id: &str) -> anyhow::Result<()> {
        if self.save_cloud_query(id)? {
            return Ok(());
        }
        let (connection, namespace) = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .and_then(|tab| match &tab.panel {
                Panel::Sql {
                    connection,
                    workbench,
                    ..
                } if workbench.table.is_none() => Some((
                    connection.id.clone(),
                    if workbench.selectors.selected {
                        identity::key(connection, &workbench.schema)
                    } else {
                        workbench.schema.clone()
                    },
                )),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("查询编辑器已关闭"))?;
        if namespace.is_empty() {
            anyhow::bail!("请先选择数据库")
        }
        let selected_scope = self.tabs.iter().find(|tab| tab.id == id).and_then(|tab| {
            if let Panel::Sql {
                connection,
                workbench,
                ..
            } = &tab.panel
            {
                workbench.selectors.selected.then(|| Scope {
                    connection: connection.name.clone(),
                    database: crate::tabs::workbench::query_database(connection, &workbench.schema)
                        .into(),
                    path: String::new(),
                })
            } else {
                None
            }
        });
        let root = if let Some(scope) = selected_scope {
            if self.queries.tree(&connection, &namespace).is_none() {
                self.ensure_query_scope(&connection, &namespace)?;
            }
            scope
        } else {
            if self
                .queries
                .tree(&connection, &namespace)
                .is_none_or(|tree| tree.root.is_none())
            {
                self.ensure_query_scope(&connection, &namespace)?;
            }
            let Some(root) = self.query_scope(&connection, &namespace, "") else {
                anyhow::ensure!(
                    self.queries.deferred.len() < 64,
                    "查询初始化操作已排队，请稍后保存"
                );
                self.queries.deferred.push(Action::Save(id.into()));
                return Ok(());
            };
            root
        };
        let tab = self.tabs.iter_mut().find(|tab| tab.id == id).unwrap();
        let Panel::Sql { sql, workbench, .. } = &mut tab.panel else {
            unreachable!()
        };
        let generic = tab.source_id == connection;
        let document = workbench
            .document
            .get_or_insert_with(|| Document::new(root.clone(), sql));
        if generic && document.name.is_none() {
            document.scope = root;
            document.namespace = namespace;
        }
        document.changed(sql);
        if document.loading {
            anyhow::bail!("查询正在加载，请稍后保存")
        }
        if document.saving {
            document.resave = true;
            return Ok(());
        }
        if let Some(name) = document.name.clone() {
            self.save_query_named(id, &name)
        } else {
            let count = self
                .queries
                .tree(&connection, &document.namespace)
                .and_then(|tree| tree.directories.get(&document.scope.path))
                .map_or(0, |directory| directory.entries.len());
            self.queries.dialog =
                Some(Dialog::save(id.into()).with_name(format!("查询{}", count + 1)));
            Ok(())
        }
    }
    pub(in crate::app) fn save_query_named(&mut self, id: &str, name: &str) -> anyhow::Result<()> {
        let name = name.trim_matches(|ch: char| ch <= ' ');
        if name.is_empty() {
            if let Some(Tab {
                panel: Panel::Sql { workbench, .. },
                ..
            }) = self.tabs.iter_mut().find(|tab| tab.id == id)
                && let Some(document) = &mut workbench.document
            {
                document.close_after_save = false;
            }
            return Ok(());
        }
        let tab = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .ok_or_else(|| anyhow::anyhow!("查询编辑器已关闭"))?;
        let Panel::Sql {
            connection,
            sql,
            workbench,
            ..
        } = &mut tab.panel
        else {
            anyhow::bail!("不是 SQL 编辑器")
        };
        let document = workbench
            .document
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("查询文档尚未初始化"))?;
        if document.loading || document.saving {
            return Ok(());
        }
        if document.name.is_some() && !document.loaded {
            anyhow::bail!("原查询未成功读取，保留草稿，请重新打开后再保存")
        }
        let generation = document.generation;
        let (connection, namespace) = (connection.id.clone(), document.namespace.clone());
        let request = Request::Save {
            scope: document.scope.clone(),
            name: name.into(),
            text: sql.clone(),
            expected: document.stamp.clone(),
        };
        self.query_command(
            &connection,
            &namespace,
            request,
            JobKind::Save {
                tab: id.into(),
                generation,
            },
        )?;
        if let Some(Tab {
            panel: Panel::Sql { workbench, .. },
            ..
        }) = self.tabs.iter_mut().find(|tab| tab.id == id)
        {
            workbench.document.as_mut().unwrap().saving = true;
        }
        Ok(())
    }
    pub(super) fn query_document_reply(&mut self, job: Job, result: anyhow::Result<Output>) {
        let (id, generation, saving) = match job.kind {
            JobKind::Read { tab, generation } => (tab, generation, false),
            JobKind::Save { tab, generation } => (tab, generation, true),
            _ => unreachable!(),
        };
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Panel::Sql { sql, workbench, .. } = &mut tab.panel else {
            return;
        };
        let Some(document) = workbench
            .document
            .as_mut()
            .filter(|document| document.generation == generation)
        else {
            return;
        };
        let mut refresh = None;
        let mut close = false;
        let mut resave = false;
        match result {
            Ok(Output::NamedQueries(queries::Output::Document {
                name, text, stamp, ..
            })) if !saving => {
                *sql = text;
                document.loaded(name, stamp);
                workbench.completion.editor.reset();
                workbench.completion.invalidate();
                tab.error = None;
            }
            Ok(Output::NamedQueries(queries::Output::Saved { scope, name, stamp })) if saving => {
                document.saved(scope.clone(), name.clone(), stamp);
                document.changed(sql);
                match source(&job.connection, &scope, &name) {
                    Ok(source) => tab.source_id = source,
                    Err(error) => tab.error = Some(error.to_string()),
                }
                close = document.close_after_save && !document.dirty();
                resave = document.resave;
                document.resave = false;
                document.close_after_save &= close || resave;
                refresh = Some(scope);
                tab.error = None;
            }
            result => {
                document.loading = false;
                document.saving = false;
                document.close_after_save = false;
                document.resave = false;
                tab.error = Some(match result {
                    Err(error) => format!("查询文件操作失败：{error:#}"),
                    _ => "未收到查询文件结果".into(),
                });
            }
        }
        tab.title = document.name.clone().unwrap_or_else(|| "未保存查询".into());
        if let Some(scope) = refresh
            && self
                .query_scope(&job.connection, &job.namespace, "")
                .is_some_and(|root| {
                    root.connection == scope.connection && root.database == scope.database
                })
            && let Err(error) = self.load_query_directory(&job.connection, &job.namespace, scope)
        {
            self.error = Some(error.to_string());
        }
        if resave {
            self.query_action(Action::Save(id.clone()));
        } else if close {
            self.close(&id);
        }
    }
    pub(super) fn query_change_reply(
        &mut self,
        connection: &str,
        namespace: &str,
        path: &str,
        result: anyhow::Result<Output>,
    ) {
        let Ok(storage) = self.query_root() else {
            return;
        };
        match result {
            Ok(Output::NamedQueries(queries::Output::Changed {
                scope,
                renamed,
                deleted,
                errors,
            })) => {
                for tab in &mut self.tabs {
                    let Panel::Sql {
                        connection: c,
                        workbench,
                        ..
                    } = &mut tab.panel
                    else {
                        continue;
                    };
                    if c.id != connection {
                        continue;
                    }
                    let Some(document) = &mut workbench.document else {
                        continue;
                    };
                    let Some(name) = document.name.clone() else {
                        continue;
                    };
                    let Ok(file) = document.scope.file(&name).map(|file| storage.join(file)) else {
                        continue;
                    };
                    if deleted.iter().any(|path| file.starts_with(path)) {
                        document.stamp = None;
                        continue;
                    }
                    if let Some((old, new)) = &renamed
                        && let Ok(suffix) = file.strip_prefix(old)
                    {
                        let updated = new.join(suffix);
                        let Ok(root) = document.scope.root().map(|root| storage.join(root)) else {
                            continue;
                        };
                        if let Ok(relative) = updated.strip_prefix(root)
                            && let Some(file) = relative.file_stem()
                        {
                            document.name = Some(file.to_string_lossy().into_owned());
                            document.scope.path = relative
                                .parent()
                                .unwrap_or_else(|| std::path::Path::new(""))
                                .to_string_lossy()
                                .replace('\\', "/");
                            document.generation += 1;
                            if let Ok(source) = source(
                                connection,
                                &document.scope,
                                document.name.as_deref().unwrap(),
                            ) {
                                tab.source_id = source;
                            }
                            tab.title = document.name.clone().unwrap();
                        }
                    }
                }
                if !errors.is_empty() {
                    self.error = Some(errors.join("\n"));
                }
                if let Some(tree) = self
                    .queries
                    .trees
                    .get_mut(connection)
                    .and_then(|trees| trees.get_mut(namespace))
                {
                    tree.directories.retain(|key, _| {
                        key == path || (!path.is_empty() && !key.starts_with(&format!("{path}/")))
                    });
                }
                if let Err(error) = self.load_query_directory(connection, namespace, scope) {
                    self.error = Some(error.to_string());
                }
            }
            Err(error) => self.error = Some(format!("查询操作失败：{error:#}")),
            _ => self.error = Some("未收到查询操作结果".into()),
        }
    }
}
