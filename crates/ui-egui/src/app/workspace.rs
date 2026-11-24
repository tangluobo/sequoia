use super::*;

impl TomatoApp {
    pub fn logic(&mut self, ctx: &egui::Context) {
        if self.closing() {
            if ctx.viewport_id() == egui::ViewportId::ROOT {
                self.poll_shutdown(ctx);
            }
            return;
        }
        self.close_requested_rdp_windows(ctx);
        if ctx.viewport_id() != egui::ViewportId::ROOT {
            if let Some(tab) = self
                .tabs
                .iter_mut()
                .find(|tab| rdp_windows::viewport(&tab.id) == ctx.viewport_id())
            {
                Self::poll_desktop(tab, ctx, false);
            }
            return;
        }
        if self.section != 0
            && let Some(commit) = tree_rename::finish(&mut self.tree_rename, ctx, true)
        {
            self.commit_tree_name(commit);
        }
        self.backups.retain_connections(
            &self
                .store
                .connections
                .iter()
                .map(|connection| connection.id.as_str())
                .collect(),
        );
        self.prune_local_trees();
        self.prune_mongo_hosts();
        self.prune_redis_hosts();
        self.poll_ssh_tree(ctx);
        self.remote_hosts
            .retain(|id, _| self.store.connections.iter().any(|c| c.id == *id));
        self.ensure_query_trees();
        if let Some(ddns) = &self._ddns
            && let Some(notice) = ddns.take_notice()
        {
            self.status = notice;
        }
        let mut certificates_changed = false;
        if let Some(notice) = self
            .certificate_renewals
            .as_ref()
            .and_then(|runner| runner.take_notice())
        {
            self.status = notice;
            certificates_changed = true;
        }
        let mut followups = Vec::new();
        let mut remote_changes = Vec::new();
        let mut local_refreshes = Vec::new();
        let mut file_refreshes = Vec::new();
        let mut file_changes = Vec::new();
        let mut object_refreshes = Vec::new();
        let mut certificate_stores = Vec::new();
        let mut close_saved = Vec::new();
        let mut transactions = Vec::new();
        let mut designer_changes = Vec::new();
        let mut browser_changes = Vec::new();
        let mut browser_catalogs = Vec::new();
        for tab in &mut self.tabs {
            if self.active.as_deref() != Some(tab.id.as_str())
                && let Panel::Sql { workbench, .. } = &mut tab.panel
                && let Some(draft) = &mut workbench.designer
            {
                draft.commit_edit(ctx);
            }
            if matches!(&tab.panel, Panel::NativeDesktop { view, .. } if view.window.is_none()) {
                Self::poll_desktop(
                    tab,
                    ctx,
                    self.section == 0 && self.active.as_deref() == Some(tab.id.as_str()),
                );
            }
            if let Panel::Terminal {
                connection,
                session,
                parser,
                view,
                ..
            } = &mut tab.panel
            {
                if self.section != 0 || self.active.as_deref() != Some(tab.id.as_str()) {
                    view.deactivate();
                }
                if let Some(workspace) = &mut view.ssh {
                    workspace.poll(
                        session,
                        ctx,
                        self.section == 0 && self.active.as_deref() == Some(tab.id.as_str()),
                    );
                    while let Some(change) = workspace.file_changes.pop_front() {
                        file_changes.push((tab.id.clone(), change));
                    }
                    crate::panels::copy_dialogs(ctx, workspace);
                    if ctx.data(|data| data.get_temp::<bool>(egui::Id::new("tomato-image-fixture")))
                        == Some(true)
                        && !ctx
                            .data(|data| {
                                data.get_temp::<bool>(egui::Id::new("tomato-image-fixture-opened"))
                            })
                            .unwrap_or_default()
                        && workspace
                            .panes
                            .get(&tomato_services::terminal::ssh::tools::Section::Files)
                            .is_some_and(|pane| {
                                pane.entries.iter().any(|entry| entry.name == "中文.png")
                            })
                    {
                        workspace.open_image("/中文.png");
                        ctx.data_mut(|data| {
                            data.insert_temp(egui::Id::new("tomato-image-fixture-opened"), true)
                        });
                    }
                    crate::panels::image_dialogs(ctx, workspace);
                    if let Some(inspector) = &workspace.inspector
                        && !inspector.name.is_empty()
                        && tab.title.strip_suffix(" 属性") != Some(inspector.name.as_str())
                    {
                        tab.title = format!("{} 属性", inspector.name);
                    }
                    if let Some(host) = self.ssh_hosts.get_mut(&connection.id)
                        && let Some(pane) = workspace
                            .panes
                            .get(&tomato_services::terminal::ssh::tools::Section::Docker)
                        && pane.updated > host.updated
                        && let Some(table) = &pane.table
                    {
                        host.containers = table
                            .rows
                            .iter()
                            .filter_map(|row| {
                                Some(ssh_tree::Container {
                                    id: row.first()?.clone(),
                                    name: row.get(2)?.clone(),
                                    running: row.get(3)?.to_ascii_lowercase().starts_with("up"),
                                })
                            })
                            .collect();
                        host.updated = pane.updated;
                    }
                }
                if session.status() == tomato_services::terminal::Status::Ready
                    && let Some(command) = view.initial_command.take()
                    && let Err(error) = session.write(&format!("{command}\r"))
                {
                    tab.error = Some(error.to_string());
                }
                let mut consumed = 0;
                while consumed < 64 * 1024
                    && let Some(data) = session.try_read()
                {
                    consumed += data.len();
                    let visible = parser.screen().visible_row_id();
                    let actions = view.control.process(&data, parser);
                    if view.alternate != parser.screen().alternate_screen() {
                        view.alternate = parser.screen().alternate_screen();
                        view.clear_selection();
                        view.paused_output = false;
                    }
                    if view.paused_output && !view.alternate {
                        let live_top = parser.screen().first_retained_row()
                            + parser.screen().history_len() as u64;
                        parser.set_scrollback(live_top.saturating_sub(visible) as usize);
                    }
                    if view.selection.as_ref().is_some_and(|selection| {
                        selection.anchor.0.max(selection.end.0)
                            < parser.screen().first_retained_row()
                    }) {
                        view.selection = None;
                        view.selection_anchor = None;
                    }
                    for reply in actions.replies {
                        if let Err(error) = session.write(&reply) {
                            tab.error = Some(error.to_string());
                        }
                    }
                    if let Some(path) = actions.directory
                        && let Some(w) = &mut view.ssh
                        && w.follow_terminal
                        && w.standalone.is_none()
                        && w.visible
                            .contains(&tomato_services::terminal::ssh::tools::Section::Files)
                    {
                        w.enqueue(
                            tomato_services::terminal::ssh::tools::Section::Files,
                            tomato_services::terminal::ssh::tools::Operation::List(path),
                        );
                    }
                }
                if consumed >= 64 * 1024 {
                    ctx.request_repaint();
                }
            }
        }
        let completions: Vec<_> = self.queue.poll().collect();
        let resume_browser_fields = !completions.is_empty();
        for mut completed in completions {
            if let Err(error) = &completed.result
                && let Some(failure) =
                    error.downcast_ref::<tomato_services::database::ConnectionFailure>()
                && failure.sessions_invalidated()
            {
                for tab in &mut self.tabs {
                    if let Panel::Sql {
                        connection,
                        workbench,
                        ..
                    } = &mut tab.panel
                        && connection.id == failure.connection_id
                    {
                        workbench.transaction = false;
                        if failure.transaction_lost {
                            workbench.session_lost = true;
                        }
                    }
                }
            }
            completed.result = match completed.result {
                Err(error) => {
                    match error.downcast::<tomato_services::database::script::Interrupted>() {
                        Ok(interrupted) => Ok(Output::Script(interrupted.script)),
                        Err(error) => Err(error),
                    }
                }
                other => other,
            };
            let Some(pending) = self.pending.remove(&completed.id) else {
                continue;
            };
            if let Purpose::ConnectionExport = pending.purpose {
                self.connection_export_reply(completed.id, completed.result);
                continue;
            }
            if let Purpose::MarkdownHtml = pending.purpose {
                self.markdown_html_reply(completed.id, completed.result);
                continue;
            }
            if matches!(
                pending.purpose,
                Purpose::ConnectionImportRead | Purpose::ConnectionImportCommit
            ) {
                self.connection_import_reply(completed.id, completed.result);
                continue;
            }
            if let Purpose::BackupTree(job) = pending.purpose {
                self.backup_tree_reply(job, completed.result);
                continue;
            }
            if let Purpose::TreeObjects(draft) = pending.purpose {
                self.tree_objects_reply(draft, completed.result);
                continue;
            }
            if let Purpose::ObjectMutation(job) = pending.purpose {
                if let Some(report) = self.object_mutation_reply(&pending.tab_id, completed.result)
                {
                    browser_changes.push((pending.tab_id, job, report));
                }
                continue;
            }
            if let Purpose::DatabaseAdmin(id) = pending.purpose {
                self.database_admin_reply(&id, completed.result);
                continue;
            }
            if let Purpose::SqlDisconnect(connection) = pending.purpose {
                match completed.result {
                    Ok(_) => {
                        self.sql_hosts.remove(&connection);
                        for tab in &mut self.tabs {
                            if let Panel::Sql {
                                connection: c,
                                workbench,
                                ..
                            } = &mut tab.panel
                                && c.id == connection
                            {
                                workbench.transaction = false;
                                workbench.status = "连接已关闭".into();
                            }
                        }
                    }
                    Err(error) => self.error = Some(format!("关闭数据库连接失败: {error}")),
                }
                continue;
            }
            if let Purpose::TreeTables {
                connection,
                schema,
                epoch,
                expected_tables,
            } = pending.purpose
            {
                if let Some(host) = self.sql_hosts.get_mut(&connection.id).filter(|h| {
                    h.epoch == epoch
                        && expected_tables.as_ref().is_none_or(|expected| {
                            h.tables_for(&connection, &schema)
                                .is_some_and(|current| std::sync::Arc::ptr_eq(expected, current))
                        })
                }) {
                    match completed.result {
                        Ok(Output::Database(tomato_core::workbench::DatabaseOutput::Tables(
                            tables,
                        ))) => {
                            host.replace_tables(&connection, schema, std::sync::Arc::new(tables))
                        }
                        Err(error) => self.error = Some(error.to_string()),
                        _ => self.error = Some("树目录返回类型错误".into()),
                    }
                }
                continue;
            }
            if let Purpose::TableTransfer(id, generation, job) = pending.purpose {
                self.table_transfer_reply(&id, generation, job, completed.result);
                continue;
            }
            if let Purpose::TablePaste(refresh) = pending.purpose {
                self.table_paste_result(*refresh, completed.result);
                continue;
            }
            if let Purpose::DirectTree(job) = pending.purpose {
                self.direct_tree_reply(job, completed.result);
                continue;
            }
            if let Purpose::PgTree(job) = pending.purpose {
                self.pg_tree_reply(job, completed.result);
                continue;
            }
            if let Purpose::Backup(id, job) = pending.purpose {
                self.backup_reply(&id, job, completed.result);
                continue;
            }
            if let Purpose::NamedQuery(job) = pending.purpose {
                self.query_reply(job, completed.result);
                continue;
            }
            if let Purpose::LocalTree {
                connection,
                path,
                generation,
            } = &pending.purpose
            {
                self.local_tree_reply(connection, path, *generation, completed.result);
                continue;
            }
            if let Purpose::RemoteTree {
                connection,
                generation,
                parent,
            } = &pending.purpose
            {
                self.remote_reply(connection, generation, parent.as_ref(), completed.result);
                continue;
            }
            if let Purpose::RedisTree {
                connection,
                generation,
            } = &pending.purpose
            {
                self.redis_reply(connection, generation, completed.result);
                continue;
            }
            if let Purpose::MongoTree {
                connection,
                generation,
                target,
                dialog,
            } = &pending.purpose
            {
                self.mongo_reply(
                    connection,
                    generation,
                    target.as_ref(),
                    dialog.as_deref(),
                    completed.result,
                );
                continue;
            }
            if let Purpose::ObjectFields(stamp) = &pending.purpose {
                self.browser_fields_reply(&pending.tab_id, stamp, completed.result);
                continue;
            }
            if let Purpose::ObjectHeaders(stamp) = &pending.purpose
                && !self.accept_browser_header(&pending.tab_id, stamp)
            {
                continue;
            }
            let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == pending.tab_id) else {
                if let Purpose::LocalMutation { connection } = pending.purpose {
                    self.local_mutation_reply(connection, completed.result);
                }
                continue;
            };
            if let Purpose::DesignSql(generation) = pending.purpose {
                if let Panel::Sql { workbench, .. } = &mut tab.panel
                    && let Some(draft) = &mut workbench.designer
                    && draft.sql_generation == generation
                    && matches!(
                        draft.sql_definition,
                        crate::tabs::table_design::SqlDefinition::Loading
                    )
                {
                    match completed.result {
                        Ok(Output::Database(
                            tomato_core::workbench::DatabaseOutput::DesignSql { table, sql },
                        )) => {
                            draft.receive_sql(&table, sql);
                        }
                        Ok(_) => draft.fail_sql("服务器未返回对象定义".into()),
                        Err(error) => {
                            let failure = error
                                .downcast_ref::<tomato_services::database::ConnectionFailure>();
                            let text =
                                failure.map_or_else(|| format!("{error:#}"), ToString::to_string);
                            draft.status = text.clone();
                            draft.fail_sql(text.clone());
                            if failure.is_some_and(|failure| !failure.is_restored()) {
                                draft.notice = Some(crate::tabs::table_design::Notice {
                                    title: "SQL预览".into(),
                                    header: Some("获取对象定义失败".into()),
                                    text,
                                    error: true,
                                    focus: true,
                                });
                            }
                        }
                    }
                }
                continue;
            }
            if matches!(pending.purpose, Purpose::CertificateBuckets) {
                if let Panel::Cloud { view, .. } = &mut tab.panel
                    && let Some(dialog) = &mut view.certificate
                    && dialog.bucket_task == Some(completed.id)
                {
                    dialog.bucket_task = None;
                    match completed.result {
                        Ok(Output::CloudBuckets(buckets)) => {
                            dialog.buckets = buckets;
                            if dialog.request.bucket.trim().is_empty() {
                                dialog.request.bucket =
                                    dialog.buckets.first().cloned().unwrap_or_default();
                            }
                        }
                        Ok(_) => dialog.bucket_error = Some("存储未返回 Bucket 列表".into()),
                        Err(error) => {
                            dialog.bucket_error = Some(format!("读取 Bucket 失败：{error:#}"))
                        }
                    }
                }
                continue;
            }
            if let Purpose::Markdown(revision) = pending.purpose {
                if let Panel::Editor { markdown, .. } = &mut tab.panel {
                    markdown.pending = false;
                    if revision == markdown.revision {
                        match completed.result {
                            Ok(Output::Markdown(document)) => {
                                markdown.retain_image_sources(&document);
                                markdown.document = document;
                                markdown.parsed = Some(revision);
                                markdown.heights.clear();
                                markdown.table_layouts.clear();
                            }
                            Err(error) => {
                                tab.error = Some(format!("预览失败：{error:#}"));
                                markdown.parsed = Some(revision);
                            }
                            _ => {}
                        }
                    }
                }
                continue;
            }
            if let Purpose::MarkdownImage(key, generation) = pending.purpose {
                if let Panel::Editor { markdown, .. } = &mut tab.panel {
                    if generation != markdown.resource_generation
                        || !markdown.images.contains_key(&key)
                    {
                        continue;
                    }
                    match completed.result {
                        Ok(Output::MarkdownImage(picture)) => markdown.image(ctx, key, picture),
                        Err(error) => {
                            markdown.image_failed(key, error.to_string());
                        }
                        _ => {}
                    }
                }
                continue;
            }
            if let Purpose::SqlDatabases {
                connection: expected,
                generation,
                preferred,
            } = &pending.purpose
            {
                let mut selected = None;
                if let Panel::Sql {
                    connection,
                    workbench,
                    ..
                } = &mut tab.panel
                    && connection.id == *expected
                    && workbench.selectors.catalog_generation == *generation
                    && let Ok(Output::Database(tomato_core::workbench::DatabaseOutput::Databases(
                        databases,
                    ))) = completed.result
                {
                    selected = preferred
                        .as_ref()
                        .filter(|name| databases.contains(name))
                        .or_else(|| {
                            if connection.kind() == "ORACLE" {
                                databases.iter().find(|name| {
                                    name.eq_ignore_ascii_case(connection.text("username"))
                                })
                            } else {
                                None
                            }
                        })
                        .or_else(|| databases.first())
                        .cloned();
                    workbench.selectors.databases = std::sync::Arc::new(databases);
                }
                if let Some(selected) = selected {
                    self.choose_sql_database(&pending.tab_id, &selected);
                }
                continue;
            }
            if let Purpose::SqlCompletion(generation) = pending.purpose {
                if let Panel::Sql { workbench, .. } = &mut tab.panel
                    && workbench.completion.generation == generation
                {
                    match completed.result {
                        Ok(Output::Database(output)) => workbench.receive(output),
                        Err(error) => {
                            workbench.completion.error = format!("补全信息读取失败：{error:#}");
                        }
                        _ => {}
                    }
                }
                continue;
            }
            if let Purpose::AiSql(generation) = pending.purpose {
                if let Panel::Sql {
                    sql,
                    workbench,
                    connection,
                    ..
                } = &mut tab.panel
                    && workbench.ai.generation == generation
                    && let Some(insertion) = workbench.ai.insertion.take()
                {
                    let outcome = match completed.result {
                        Ok(Output::AiSql(generated)) => insertion.apply(
                            sql,
                            workbench.completion.selection.primary.index.into(),
                            &generated,
                        ),
                        Err(error) => Err(error),
                        _ => Err(anyhow::anyhow!("模型响应类型错误")),
                    };
                    match outcome {
                        Ok((text, selection)) => {
                            workbench.completion.editor.replace(ctx, sql, text);
                            workbench.completion.editor.select(ctx, selection.clone());
                            workbench.completion.selection = egui::text::CCursorRange::two(
                                egui::text::CCursor::new(selection.start),
                                egui::text::CCursor::new(selection.end),
                            );
                            workbench.completion.invalidate();
                            let document = workbench.document.get_or_insert_with(|| {
                                let mut document = crate::tabs::queries::Document::new(
                                    tomato_core::queries::Scope {
                                        connection: connection.name.clone(),
                                        database: crate::tabs::workbench::query_database(
                                            connection,
                                            &workbench.schema,
                                        )
                                        .into(),
                                        path: String::new(),
                                    },
                                    &insertion.snapshot,
                                );
                                document.namespace = workbench.schema.clone();
                                document
                            });
                            document.changed(sql);
                        }
                        Err(error) => tab.error = Some(format!("AI 生成失败: {error}")),
                    }
                }
                continue;
            }
            if let Panel::Sql { workbench, .. } = &mut tab.panel {
                workbench.execution = None;
            }
            tab.busy = self.pending.values().any(|task| {
                task.tab_id == tab.id && matches!(task.purpose, Purpose::QuerySave { .. })
            });
            if let (Purpose::QuerySave { result, generation }, Panel::Sql { workbench, .. }) =
                (&pending.purpose, &mut tab.panel)
                && *generation == workbench.results_generation
                && let Some(state) = workbench.query_results.get_mut(*result)
            {
                state.saving = false;
            }
            match completed.result {
                Err(error) => {
                    let connection_failure =
                        error.downcast_ref::<tomato_services::database::ConnectionFailure>();
                    if matches!(pending.purpose, Purpose::Certificate)
                        && let Panel::Cloud { view, .. } = &mut tab.panel
                    {
                        view.control = None;
                        certificates_changed = true;
                        if let Some(dialog) = &mut view.certificate {
                            dialog.issue_error = Some(format!("申请证书失败：{error:#}"));
                            continue;
                        }
                    }
                    if let Panel::Sql { workbench, .. } = &mut tab.panel
                        && let Some(browser) = &mut workbench.browser
                    {
                        browser.notice = match connection_failure {
                            Some(failure) => (!failure.is_restored()).then(|| failure.to_string()),
                            None => Some(format!("{error:#}")),
                        };
                    }
                    if let Panel::Sql { workbench, .. } = &mut tab.panel
                        && let Some(draft) = &mut workbench.designer
                    {
                        draft.busy = false;
                        draft.saving = false;
                        draft.status = connection_failure.map_or_else(
                            || format!("保存或加载失败: {error:#}"),
                            ToString::to_string,
                        );
                        draft.notice = connection_failure
                            .is_none_or(|failure| !failure.is_restored())
                            .then(|| crate::tabs::table_design::Notice {
                                title: "表结构".into(),
                                header: Some("操作失败".into()),
                                text: connection_failure
                                    .map_or_else(|| format!("{error:#}"), ToString::to_string),
                                error: true,
                                focus: true,
                            });
                    }
                    if let Panel::Sql { workbench, .. } = &mut tab.panel {
                        for state in &mut workbench.query_results {
                            if let Some(editing) = &mut state.editing {
                                editing.deleted.clear();
                                editing.confirm_delete = false;
                            }
                        }
                    }
                    if let Panel::Tool(tool) = &mut tab.panel
                        && let Some(view) = &mut tool.extractor
                    {
                        view.status = format!("运行失败: {error:#}");
                    }
                    if let Panel::Tool(tool) = &mut tab.panel
                        && let Some(view) = &mut tool.trae
                    {
                        view.pending_selection = None;
                        if view.task_status == "加载中..." && !view.pending_tasks {
                            view.task_status = "加载失败，请重新选择会话".into();
                        }
                    }
                    if let Panel::Mongo { view, .. } = &mut tab.panel {
                        view.pending.clear();
                        view.status = "操作失败".into();
                    }
                    if matches!(pending.purpose, Purpose::Save(_))
                        && let Panel::Editor { markdown, .. } = &mut tab.panel
                    {
                        markdown.close_after_save = false;
                    }
                    if matches!(pending.purpose, Purpose::ToolConversion)
                        && let Panel::Tool(tool) = &mut tab.panel
                    {
                        let state = tool.converter.get_or_insert_with(Default::default);
                        if error.is::<tomato_services::tools::conversion::NoImages>() {
                            state.fail_input(&error.to_string());
                        } else {
                            state.fail(&format!("{error:#}"));
                        }
                        tab.error = None;
                    } else if let (Purpose::Json(feedback), Panel::Tool(tool)) =
                        (&pending.purpose, &mut tab.panel)
                    {
                        let message = if feedback.format_error {
                            format!("JSON 格式错误: {error:#}")
                        } else {
                            format!("{error:#}")
                        };
                        tool.json_status.error(message.clone());
                        if feedback.format_error {
                            tool.set_output(message);
                        }
                        tab.error = None;
                    } else if let Panel::Sql { workbench, .. } = &mut tab.panel
                        && let Some(failure) = connection_failure
                    {
                        workbench.pending_page = None;
                        workbench.failure = (!failure.is_restored()
                            && workbench.browser.is_none()
                            && workbench.designer.is_none())
                        .then(|| crate::tabs::workbench::Failure {
                            text: failure.to_string(),
                            focus: true,
                        });
                        tab.error = None;
                        self.status = failure.to_string();
                    } else if let Panel::Sql { workbench, .. } = &mut tab.panel
                        && workbench.browser.is_none()
                        && workbench.designer.is_none()
                    {
                        workbench.pending_page = None;
                        workbench.failure = Some(crate::tabs::workbench::Failure {
                            text: format!("{error:#}"),
                            focus: true,
                        });
                        tab.error = None;
                    } else if let Panel::Tool(tool) = &mut tab.panel
                        && let Some(view) = &mut tool.hosts
                    {
                        if view.snapshot.is_some() {
                            view.fail(ctx, &error);
                            tab.error = None;
                        } else {
                            tab.error = Some(format!("{error:#}"));
                        }
                    } else if let Panel::Redis { view, .. } = &mut tab.panel {
                        view.fail(format!("{error:#}"));
                        tab.error = None;
                    } else {
                        tab.error = Some(format!("{error:#}"));
                    }
                }
                Ok(output) => {
                    tab.error = None;
                    if let (Purpose::Json(feedback), Panel::Tool(tool)) =
                        (&pending.purpose, &mut tab.panel)
                        && let Some(message) = feedback.success
                    {
                        tool.json_status.success(message);
                    }
                    match (output, &mut tab.panel, pending.purpose) {
                        (Output::JsonHistory(result), Panel::Tool(tool), _) => {
                            if let Some(message) = result.notice {
                                tool.json_status.error(message);
                            }
                            tool.history = result.items;
                            if let Some(input) = result.input {
                                tool.input = input;
                                tool.json_reset_input_cursor = true;
                            }
                            if let Some(formatted) = result.formatted {
                                tool.set_json_tree(formatted);
                            }
                            if let Some(clipboard) = result.clipboard {
                                ctx.copy_text(clipboard);
                            }
                        }
                        (
                            Output::Database(output),
                            Panel::Sql {
                                connection,
                                workbench,
                                ..
                            },
                            _,
                        ) => {
                            if let tomato_core::workbench::DatabaseOutput::Objects(catalog)
                            | tomato_core::workbench::DatabaseOutput::ObjectHeaders(catalog) =
                                &output
                            {
                                browser_catalogs.push((
                                    connection.clone(),
                                    workbench.schema.clone(),
                                    catalog
                                        .objects
                                        .iter()
                                        .map(|o| tomato_core::workbench::TableInfo {
                                            table: o.table.clone(),
                                            view: o.view,
                                        })
                                        .collect::<Vec<_>>(),
                                ));
                            }
                            if let tomato_core::workbench::DatabaseOutput::TableCreated(table) =
                                &output
                                && let Some(draft) = &mut workbench.designer
                            {
                                draft.created = true;
                                followups.push((
                                    tab.id.clone(),
                                    Command::Database {
                                        connection: connection.clone(),
                                        action: tomato_core::workbench::DatabaseAction::DesignStructure {
                                            table: table.clone(),
                                        },
                                    },
                                ));
                            }
                            if let tomato_core::workbench::DatabaseOutput::Design(snapshot) =
                                &output
                            {
                                if workbench.designer.as_ref().is_some_and(|d| d.saving) {
                                    designer_changes.push((
                                        tab.id.clone(),
                                        connection.clone(),
                                        snapshot.definition.table.clone(),
                                    ));
                                }
                                if workbench
                                    .designer
                                    .as_ref()
                                    .is_some_and(|d| d.metadata.is_none())
                                {
                                    followups.push((tab.id.clone(),Command::Database{connection:connection.clone(),action:tomato_core::workbench::DatabaseAction::DesignMetadata}));
                                }
                                workbench.table = Some(snapshot.definition.table.clone());
                                tab.title = format!(
                                    "{}@{}({}:{})-{}结构",
                                    snapshot.definition.table.name,
                                    crate::tabs::workbench::query_database(
                                        connection,
                                        &snapshot.definition.table.schema
                                    ),
                                    connection.text("host"),
                                    connection.number("port"),
                                    if snapshot.view { "视图" } else { "表" }
                                );
                                tab.source_id = format!(
                                    "dbtable:structure:{}:{}",
                                    connection.id,
                                    serde_json::to_string(&(
                                        connection.text("database"),
                                        &snapshot.definition.table
                                    ))
                                    .unwrap_or_default()
                                );
                            }
                            if let tomato_core::workbench::DatabaseOutput::Transaction(active) =
                                &output
                            {
                                transactions.push((
                                    connection.id.clone(),
                                    crate::tabs::workbench::session_database(
                                        connection,
                                        &workbench.schema,
                                    )
                                    .to_owned(),
                                    *active,
                                ));
                            }
                            if let tomato_core::workbench::DatabaseOutput::Schemas(schemas) =
                                &output
                            {
                                let preferred = match connection.kind() {
                                    "MYSQL" => connection.text("database"),
                                    "HIVE" => tomato_core::database_scope::hive_name(
                                        connection.text("database"),
                                    ),
                                    "POSTGRESQL" if !workbench.schema.is_empty() => {
                                        workbench.schema.as_str()
                                    }
                                    "POSTGRESQL" => "public",
                                    "ORACLE" => connection.text("username"),
                                    _ if !workbench.schema.is_empty() => workbench.schema.as_str(),
                                    _ => "main",
                                };
                                workbench.schema = schemas
                                    .iter()
                                    .find(|schema| {
                                        schema.as_str() == preferred
                                            || connection.kind() == "ORACLE"
                                                && schema.eq_ignore_ascii_case(preferred)
                                    })
                                    .or_else(|| schemas.first())
                                    .cloned()
                                    .unwrap_or_default();
                                if !workbench.schema.is_empty() {
                                    followups.push((
                                        tab.id.clone(),
                                        Command::Database {
                                            connection: connection.clone(),
                                            action:
                                                tomato_core::workbench::DatabaseAction::SelectSchema {
                                                    schema: workbench.schema.clone(),
                                                },
                                        },
                                    ));
                                }
                            }
                            if matches!(&output, tomato_core::workbench::DatabaseOutput::Tables(_))
                                && workbench.table.is_none()
                                && !workbench.schema.is_empty()
                            {
                                followups.push((
                                    tab.id.clone(),
                                    Command::Database {
                                        connection: connection.clone(),
                                        action:
                                            tomato_core::workbench::DatabaseAction::Completion {
                                                schema: workbench.schema.clone(),
                                            },
                                    },
                                ));
                            }
                            let tree_tables = matches!(
                                &output,
                                tomato_core::workbench::DatabaseOutput::Tables(_)
                            );
                            if matches!(
                                &output,
                                tomato_core::workbench::DatabaseOutput::Objects(_)
                                    | tomato_core::workbench::DatabaseOutput::ObjectHeaders(_)
                            ) && let Some(browser) = &mut workbench.browser
                            {
                                browser.rename_editor.release(ctx);
                                browser.rename_editor.reset();
                            }
                            workbench.receive(output);
                            if tab.source_id == connection.id
                                && self.sql_hosts.get(&connection.id).is_none_or(|host| {
                                    host.connection.text("database") == connection.text("database")
                                })
                            {
                                let host = self
                                    .sql_hosts
                                    .entry(connection.id.clone())
                                    .or_insert_with(|| sql_tree::Host::new(connection.clone()));
                                host.schemas = workbench.schemas.clone();
                                host.selected = workbench.schema.clone();
                                if tree_tables {
                                    if !host.tables.contains_key(&workbench.schema)
                                        && matches!(
                                            connection.kind(),
                                            "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"
                                        )
                                    {
                                        for folder in
                                            [db_tree::Folder::Tables, db_tree::Folder::Views]
                                        {
                                            self.collapsed
                                                .insert(folder.key(connection, &workbench.schema));
                                        }
                                    }
                                    host.tables
                                        .insert(workbench.schema.clone(), workbench.tables.clone());
                                }
                            }
                        }
                        (
                            Output::Script(mut script),
                            Panel::Sql {
                                results,
                                selected,
                                workbench,
                                ..
                            },
                            purpose,
                        ) => {
                            workbench.result_context = if let Purpose::SqlRequest(context) = purpose
                            {
                                Some(context)
                            } else {
                                None
                            };
                            *results = std::mem::take(&mut script.results);
                            workbench.results_generation =
                                workbench.results_generation.wrapping_add(1);
                            *selected = 0;
                            workbench.script_view = if matches!(
                                script.mode,
                                tomato_services::database::script::Mode::Explain
                            ) {
                                usize::MAX
                            } else if script.interruption.is_some()
                                || script.fail_count > 0
                                || script.row_results().next().is_none()
                            {
                                0
                            } else {
                                1
                            };
                            for state in &workbench.query_results {
                                state.release(&self.context);
                            }
                            workbench.query_results.clear();
                            workbench
                                .query_results
                                .resize_with(results.len(), Default::default);
                            for statement in &mut script.statements {
                                if let Some(index) = statement.result {
                                    workbench.query_results[index].source_table =
                                        statement.source_table.clone();
                                    if let Some(editing) = statement.editing.take() {
                                        workbench.query_results[index].editing = Some(Box::new(
                                            crate::tabs::query_results::editing::Editing::new(
                                                editing,
                                            ),
                                        ));
                                    }
                                    if let Some(error) = statement.edit_error.take() {
                                        workbench.query_results[index].edit_status =
                                            Some(if error.contains("超过") {
                                                error
                                            } else {
                                                "只读结果 · 无法读取源表结构".into()
                                            });
                                    }
                                }
                            }
                            if let Some(interruption) = &script.interruption {
                                self.status = interruption.connection.as_ref().map_or_else(
                                    || "脚本已停止，已完成的查询结果已保留".into(),
                                    ToString::to_string,
                                );
                                if !interruption
                                    .connection
                                    .as_ref()
                                    .is_some_and(|failure| failure.is_restored())
                                {
                                    workbench.failure = Some(crate::tabs::workbench::Failure {
                                        text: interruption.message.clone(),
                                        focus: true,
                                    });
                                }
                            }
                            workbench.script = Some(script);
                            workbench.script_info = None;
                        }
                        (
                            Output::SavedQueryRows {
                                result,
                                affected,
                                deleting,
                            },
                            Panel::Sql {
                                results, workbench, ..
                            },
                            Purpose::QuerySave {
                                result: expected,
                                generation,
                            },
                        ) if expected == result && generation == workbench.results_generation => {
                            if let (Some(rows), Some(state)) = (
                                results.get_mut(result),
                                workbench.query_results.get_mut(result),
                            ) {
                                state.committed(rows, deleting);
                                workbench.status = format!("已保存 {affected} 行修改");
                            }
                        }
                        (
                            Output::Queries(value),
                            Panel::Sql {
                                results,
                                selected,
                                workbench,
                                ..
                            },
                            purpose,
                        ) => {
                            workbench.result_context = if let Purpose::SqlRequest(context) = purpose
                            {
                                Some(context)
                            } else {
                                None
                            };
                            *results = value;
                            workbench.results_generation =
                                workbench.results_generation.wrapping_add(1);
                            *selected = 0;
                            for state in &workbench.query_results {
                                state.release(&self.context);
                            }
                            workbench.query_results.clear();
                            workbench.script = None;
                            workbench.script_info = None;
                        }
                        (Output::StagedRows(rows), Panel::Sql { workbench, .. }, _) => {
                            workbench.status =
                                format!("已暂存 {} 行导入数据，点击保存后写入数据库", rows.len());
                            workbench.inserted.extend(rows);
                            workbench.search.clear();
                        }
                        (
                            Output::Text(value),
                            Panel::Sql { sql, workbench, .. },
                            Purpose::SqlRead,
                        ) => {
                            *sql = value;
                            workbench.completion.editor.reset();
                            workbench.completion.selection = Default::default();
                            workbench.completion.invalidate();
                        }
                        (
                            Output::Text(value),
                            Panel::Editor {
                                text,
                                saved,
                                markdown,
                                ..
                            },
                            Purpose::Read,
                        ) => {
                            *text = value.clone();
                            *saved = value;
                            markdown.changed(ctx.input(|i| i.time));
                        }
                        (
                            Output::Text(_),
                            Panel::Editor {
                                saved,
                                path,
                                text,
                                markdown,
                            },
                            Purpose::Save(snapshot),
                        ) => {
                            if markdown.close_after_save && *text == snapshot {
                                close_saved.push(tab.id.clone());
                            }
                            markdown.close_after_save = false;
                            *saved = snapshot;
                            if let Some(owner) = &markdown.file_owner {
                                file_refreshes.push(owner.clone());
                            } else if let Some(parent) = path.parent() {
                                if let Some(connection) = &markdown.remote {
                                    object_refreshes.push((connection.clone(), parent.to_owned()));
                                } else {
                                    local_refreshes.push(parent.to_owned());
                                }
                            }
                            self.status = "文件已保存".into();
                        }
                        (Output::Resources(value), Panel::Tool(tool), _) => {
                            tool.extractor
                                .get_or_insert_with(Default::default)
                                .accept(value);
                        }
                        (Output::Music(value), Panel::Tool(tool), _) => {
                            if let Some(music) = &mut tool.music {
                                music.accept(value);
                            }
                        }
                        (Output::GameDungeons(value), Panel::Tool(tool), _) => {
                            if let Some(state) = &mut tool.game_dungeons {
                                state.accept(value);
                            }
                        }
                        (Output::Mongo(value), Panel::Mongo { view, .. }, _) => view.accept(value),
                        (Output::Cloud(value), Panel::Cloud { view, .. }, _) => view.accept(value),
                        (
                            Output::MessageQueue(value),
                            Panel::MessageQueue { connection, view },
                            _,
                        ) => {
                            if let Some(action) = &view.pending
                                && matches!(
                                    action,
                                    tomato_services::mq::Action::CreateTopic { .. }
                                        | tomato_services::mq::Action::DeleteTopic { .. }
                                        | tomato_services::mq::Action::DeleteGroup { .. }
                                )
                            {
                                remote_changes.push((
                                    connection.clone(),
                                    matches!(
                                        action,
                                        tomato_services::mq::Action::DeleteGroup { .. }
                                    ),
                                ));
                            }
                            view.accept(value)
                        }
                        (Output::Certificate(value), Panel::Cloud { view, .. }, _) => {
                            if let Some(dialog) = &view.certificate
                                && dialog.storage_copy
                            {
                                certificate_stores.push(dialog.selected_storage.clone());
                            }
                            view.status = value;
                            view.certificate = None;
                            view.control = None;
                            certificates_changed = true;
                        }
                        (Output::Trae(value), Panel::Tool(tool), _) => {
                            tool.trae
                                .get_or_insert_with(Default::default)
                                .accept(value, ctx.input(|i| i.time));
                        }
                        (Output::Hosts(value), Panel::Tool(tool), _) => {
                            let view = tool.hosts.get_or_insert_with(Default::default);
                            if let Some(prompt) = &view.authorization {
                                prompt.editor.release(ctx);
                            }
                            view.accept(value);
                        }
                        (
                            Output::Conversion(report),
                            Panel::Tool(tool),
                            Purpose::ToolConversion,
                        ) => {
                            tool.converter
                                .get_or_insert_with(Default::default)
                                .accept(report);
                        }
                        (Output::Text(value), Panel::Tool(tool), _) => tool.set_output(value),
                        (Output::RedisData(value), Panel::Redis { view, .. }, _) => {
                            view.accept(value, ctx.input(|i| i.time))
                        }
                        (Output::Text(value), Panel::Redis { view, .. }, _) => view.status = value,
                        (Output::Text(value), _, _) => self.status = value,
                        _ => {}
                    }
                }
            }
        }
        for tab in &mut self.tabs {
            if certificates_changed && let Panel::Cloud { view, .. } = &mut tab.panel {
                view.certificate_refresh = true;
            }
            if certificate_stores.contains(&tab.source_id)
                && let Panel::Terminal { view, .. } = &mut tab.panel
                && let Some(workspace) = &mut view.ssh
            {
                for pane in workspace.panes.values_mut() {
                    pane.refresh = true;
                }
            }
        }
        for id in close_saved {
            self.close(&id);
        }
        for (owner, change) in file_changes {
            self.file_document_change(&owner, change);
        }
        for owner in file_refreshes {
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == owner)
                && let Panel::Terminal { view, .. } = &mut tab.panel
                && let Some(workspace) = &mut view.ssh
            {
                workspace
                    .panes
                    .entry(tomato_services::terminal::ssh::tools::Section::Files)
                    .or_default()
                    .refresh = true;
            }
        }
        for (connection, parent) in object_refreshes {
            self.local_tree_open(&connection, Some(parent), true);
        }
        for parent in local_refreshes {
            let connections: Vec<_> = self
                .store
                .connections
                .iter()
                .filter(|connection| {
                    self.local_trees
                        .get(&connection.id)
                        .is_some_and(|host| host.directories.contains_key(&parent))
                })
                .cloned()
                .collect();
            for connection in connections {
                self.local_tree_open(&connection, Some(parent.clone()), true);
            }
        }
        for (id, database, active) in transactions {
            if let Some(host) = self.sql_hosts.get_mut(&id) {
                if active {
                    host.transactions.insert(database.clone());
                } else {
                    host.transactions.remove(&database);
                }
            }
            for tab in &mut self.tabs {
                if let Panel::Sql {
                    connection,
                    workbench,
                    ..
                } = &mut tab.panel
                    && connection.id == id
                    && crate::tabs::workbench::session_database(connection, &workbench.schema)
                        == database
                {
                    workbench.transaction = active;
                    // Commit/rollback can invalidate any cached page in this session.
                    if !active && workbench.page.is_some() {
                        workbench.needs_refresh = true;
                        workbench.status = "事务已结束，请刷新表数据".into();
                    }
                }
            }
        }
        for (id, connection, table) in designer_changes {
            self.refresh_design_dependents(&id, &connection, &table);
        }
        for (connection, schema, tables) in browser_catalogs {
            if let Some(host) = self.sql_hosts.get_mut(&connection.id) {
                host.replace_tables(&connection, schema, std::sync::Arc::new(tables));
            }
        }
        for (id, job, report) in browser_changes {
            self.receive_object_changes(&id, &job.connection, &job.request, &report);
        }
        for (connection, groups) in remote_changes {
            self.remote_changed(connection, groups);
        }
        for (id, command) in followups {
            self.submit(&id, command, Purpose::Normal);
        }
        if resume_browser_fields {
            let ids = self.tabs.iter().filter_map(|tab| {
                let Panel::Sql { workbench, .. } = &tab.panel else { return None; };
                workbench.browser.as_ref().filter(|b| !b.header_pending
                    && b.fields_loading == crate::tabs::database_objects::window::FieldsLoading::NotRequested
                    && !b.catalog.objects.is_empty()).map(|_| tab.id.clone())
            }).collect::<Vec<_>>();
            for id in ids {
                self.load_browser_fields(&id);
            }
        }
        for (id, result) in self.server_receiver.try_iter() {
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                tab.busy = false;
                match result {
                    Ok(server) => {
                        if let Panel::Server(view) = &mut tab.panel {
                            view.log(format!(
                                "监听地址：{}://{}",
                                view.config.kind.scheme(),
                                server.address
                            ));
                            view.log("==== 服务已启动 ====");
                            view.running = Some(server);
                        }
                    }
                    Err(error) => tab.error = Some(format!("{error:#}")),
                }
            }
        }
        self.continue_file_owner_close();
        self.continue_tab_closes();
        if ctx.input(|i| i.viewport().close_requested()) {
            let ids: Vec<_> = self
                .tabs
                .iter()
                .filter_map(|tab| {
                    matches!(&tab.panel, Panel::Server(view) if view.changed)
                        .then_some(tab.id.clone())
                })
                .collect();
            for id in ids {
                if !self.flush_server_config(&id) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    return;
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quit_approved
            && (self.tabs.iter().any(Tab::dirty)
                || self.development.as_ref().is_some_and(|state| state.dirty()))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirmation = Some(Confirmation::Quit);
            return;
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.begin_shutdown();
            ctx.request_repaint();
        }
    }

    pub(super) fn open_connection(&mut self, connection: Connection) {
        let connection = if connection.kind() == "SSH" {
            self.cached_ssh_connection(connection)
        } else {
            connection
        };
        if connection.kind() == "RDP" {
            self.open_rdp(connection);
            return;
        }
        if connection.kind() == "REDIS" {
            if !self.redis_hosts.contains_key(&connection.id)
                && connection
                    .options
                    .get("password")
                    .is_none_or(serde_json::Value::is_null)
            {
                self.credential = Some(credentials::Prompt::new(connection));
                return;
            }
            self.redis_open(connection, false);
            return;
        }
        if connection.kind() == "MONGODB" {
            if !self.mongo_hosts.contains_key(&connection.id)
                && !connection.text("username").trim().is_empty()
                && connection
                    .options
                    .get("password")
                    .is_none_or(serde_json::Value::is_null)
            {
                self.credential = Some(credentials::Prompt::new(connection));
                return;
            }
            self.mongo_open(connection, false);
            return;
        }
        if connection.kind() == "LOCAL_DIRECTORY" {
            self.local_tree_open(&connection, None, false);
            return;
        }
        let connection = if matches!(
            connection.kind(),
            "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"
        ) {
            self.live_sql_connection(connection)
        } else {
            connection
        };
        if matches!(
            connection.kind(),
            "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"
        ) && self.sql_password_missing(&connection)
        {
            self.credential = Some(credentials::Prompt::new(connection));
            return;
        }
        if matches!(connection.kind(), "MYSQL" | "ORACLE" | "HIVE") {
            self.open_direct_host(connection, false);
            return;
        }
        if connection.kind() == "POSTGRESQL" {
            self.open_pg_host(connection, false);
            return;
        }
        if !matches!(connection.kind(), "SSH" | "LOCAL_TERMINAL")
            && let Some(tab) = self.tabs.iter().find(|tab| tab.source_id == connection.id)
        {
            if matches!(&tab.panel, Panel::NativeDesktop { view, .. } if view.window.is_some()) {
                let viewport = rdp_windows::viewport(&tab.id);
                self.context
                    .send_viewport_cmd_to(viewport, egui::ViewportCommand::Minimized(false));
                self.context
                    .send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
            } else {
                self.active = Some(tab.id.clone());
            }
            return;
        }
        let file_auth = matches!(
            connection.kind(),
            "S3" | "ALIYUN_OSS" | "ALIYUN" | "TENCENT_CLOUD"
        ) || connection.kind() == "FTP"
            && connection.options.get("usePassword") != Some(&serde_json::Value::Bool(false))
            || connection.kind() == "LOCAL_DIRECTORY" && connection.text("directoryType") == "S3";
        if file_auth && connection.text("password").is_empty() {
            self.credential = Some(credentials::Prompt::new(connection));
            return;
        }
        if matches!(
            connection.kind(),
            "ALIYUN" | "TENCENT_CLOUD" | "KAFKA" | "ROCKETMQ"
        ) {
            self.remote_open(connection, false);
            return;
        }
        self.collapsed.remove(&connection.id);
        if connection.kind() == "POSTGRESQL" {
            self.open_pg_host(connection, false);
            return;
        }
        match tabs::create_tab(connection, self.runtime.as_ref().map(|r| r.handle())) {
            Ok(mut tab) => {
                if let Panel::Sql {
                    connection,
                    workbench,
                    ..
                } = &mut tab.panel
                {
                    if let Some(host) = self.sql_hosts.get(&connection.id) {
                        workbench.schemas = host.schemas.clone();
                        workbench.schema = host.selected.clone();
                        workbench.tables =
                            host.tables.get(&host.selected).cloned().unwrap_or_default();
                    } else {
                        self.sql_hosts.insert(
                            connection.id.clone(),
                            sql_tree::Host::new(connection.clone()),
                        );
                    }
                    workbench.transaction = self.sql_transaction(connection, &workbench.schema);
                }
                terminal_config::apply(&mut tab, &self.settings);
                if let Panel::Terminal { session, .. } = &tab.panel {
                    let repaint = self.context.clone();
                    session.on_output(move || repaint.request_repaint());
                }
                let detach_adb = matches!(&tab.panel, Panel::NativeDesktop { connection, .. }
                    if connection.kind() == "ADB" && connection.flag("fullscreen"));
                if let Panel::Tool(tool) = &mut tab.panel {
                    if tool.kind == "json_tool" {
                        tool.history_path = self.json_history_path();
                    }
                    self.prepare_downloader(tool);
                    self.prepare_game_dungeons(tool);
                }
                let first = match &tab.panel {
                    Panel::Tool(tool) if tool.kind == "json_tool" => Some(Command::JsonHistory {
                        path: tool.history_path.clone(),
                        action: tomato_services::json_history::Action::Load,
                    }),
                    Panel::Sql {
                        connection,
                        workbench,
                        ..
                    } if workbench.schemas.is_empty() => Some(Command::Database {
                        connection: connection.clone(),
                        action: tomato_core::workbench::DatabaseAction::Schemas,
                    }),
                    Panel::Sql {
                        connection,
                        workbench,
                        ..
                    } if workbench.table.is_none() && !workbench.schema.is_empty() => {
                        Some(Command::Database {
                            connection: connection.clone(),
                            action: tomato_core::workbench::DatabaseAction::Completion {
                                schema: workbench.schema.clone(),
                            },
                        })
                    }
                    _ => None,
                };
                let id = tab.id.clone();
                if !detach_adb {
                    self.active = Some(id.clone());
                }
                self.tabs.push(tab);
                if detach_adb {
                    self.toggle_adb_window(&id);
                } else {
                    self.bind_rdp_repaint(&id);
                }
                if let Some(command) = first {
                    self.submit(&id, command, Purpose::Normal);
                }
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }
    pub(super) fn open_tool(&mut self, kind: &str, label: &str) {
        let source_id = format!("tool:{kind}");
        if let Some(tab) = self.tabs.iter().find(|tab| tab.source_id == source_id) {
            self.active = Some(tab.id.clone());
        } else {
            let id = Connection::new("TOOL").id;
            let mut state = ToolState::new(kind);
            state.history_path = self.json_history_path();
            if kind == "port_mapping" {
                let known = self
                    .repository
                    .location()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(Store::default_path)
                    .with_file_name("known_hosts");
                state.port_mapping = Some(Box::new(crate::tabs::port_mapping::State::new(
                    tomato_services::terminal::ssh::forwarding::legacy_path(),
                    self.runtime.as_ref().map(|r| r.handle().clone()),
                    known,
                )));
            }
            self.prepare_downloader(&mut state);
            self.prepare_game_dungeons(&mut state);
            let history_path = state.history_path.clone();
            self.tabs.push(Tab {
                id: id.clone(),
                source_id,
                title: label.into(),
                panel: Panel::Tool(state),
                busy: false,
                error: None,
            });
            self.active = Some(id.clone());
            if kind == "json_tool" {
                self.submit(
                    &id,
                    Command::JsonHistory {
                        path: history_path,
                        action: tomato_services::json_history::Action::Load,
                    },
                    Purpose::Normal,
                );
            }
        }
    }
    fn json_history_path(&self) -> PathBuf {
        self.repository
            .location()
            .filter(|path| {
                *path != Store::default_path() || std::env::var_os("TOMATO_DATA_DIR").is_some()
            })
            .map(|path| path.with_file_name("json_history.json"))
            .unwrap_or_else(tomato_services::json_history::legacy_path)
    }
    pub(super) fn open_editor(&mut self, path: PathBuf) {
        self.open_document(path, None);
    }
    pub(super) fn open_tree_editor(&mut self, connection: Connection, path: PathBuf) {
        if tomato_services::files::object_tree::is_object(&connection) {
            self.open_document(path, Some(self.tree_connection_shared(&connection)));
        } else {
            self.open_editor(path);
        }
    }
    fn open_document(&mut self, path: PathBuf, remote: Option<std::sync::Arc<Connection>>) {
        let source_id = if let Some(connection) = &remote {
            format!("object-file:{}:{}", connection.id, path.display())
        } else {
            format!("file:{}", path.display())
        };
        if let Some(tab) = self.tabs.iter().find(|tab| tab.source_id == source_id) {
            self.active = Some(tab.id.clone());
        } else {
            let id = Connection::new("TOOL").id;
            self.tabs.push(Tab {
                id: id.clone(),
                source_id,
                title: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                panel: Panel::Editor {
                    path: path.clone(),
                    text: String::new(),
                    saved: String::new(),
                    markdown: Box::new(crate::tabs::markdown::State {
                        remote: remote.clone(),
                        ..Default::default()
                    }),
                },
                busy: false,
                error: None,
            });
            if let Some(connection) = remote {
                self.submit(
                    &id,
                    Command::ObjectTree {
                        connection: connection.as_ref().clone(),
                        action: tomato_services::files::object_tree::Request::Read(path),
                    },
                    Purpose::Read,
                );
            } else if path.exists() {
                self.submit(&id, Command::ReadText { path }, Purpose::Read);
            }
            self.active = Some(id);
        }
    }
    pub(super) fn submit(&mut self, id: &str, mut command: Command, mut purpose: Purpose) {
        if self.submit_object_read(id, &command) {
            return;
        }
        purpose = match command {
            Command::ConvertTool { .. } => Purpose::ToolConversion,
            Command::Certificate { .. } => Purpose::Certificate,
            Command::CloudBuckets { .. } => Purpose::CertificateBuckets,
            _ => purpose,
        };
        if let Command::Certificate {
            renewal: Some((profile, _)),
            ..
        } = &mut command
            && let Some(path) = self.repository.location()
        {
            *profile = path.to_owned();
        }
        let storage = match &mut command {
            Command::CloudBuckets { connection } => Some(connection),
            Command::Certificate { request, .. } if !request.bucket.trim().is_empty() => {
                Some(&mut request.storage)
            }
            _ => None,
        };
        if let Some(storage) = storage {
            if storage.text("password").is_empty() {
                let live = self.tabs.iter().find_map(|tab| {
                    let connection = match &tab.panel {
                        Panel::Terminal { connection, .. } => Some(connection),
                        Panel::Cloud { view, .. } => {
                            view.certificate.as_ref().map(|d| &d.request.storage)
                        }
                        _ => None,
                    }?;
                    (connection.id == storage.id && !connection.text("password").is_empty())
                        .then_some(connection.clone())
                });
                if let Some(live) = live {
                    storage.set("password", live.text("password"));
                }
            }
            if storage.text("password").is_empty() {
                self.credential = Some(credentials::Prompt::then(
                    storage.clone(),
                    credentials::Resume::StorageCommand {
                        tab: id.into(),
                        command: Box::new(command),
                    },
                ));
                return;
            }
        }
        if let Command::CloudBuckets { .. } = &command {
            let Some(Panel::Cloud { view, .. }) = self
                .tabs
                .iter_mut()
                .find(|tab| tab.id == id)
                .map(|tab| &mut tab.panel)
            else {
                return;
            };
            let Some(dialog) = &mut view.certificate else {
                return;
            };
            self.pending.retain(|_, pending| {
                pending.tab_id != id || !matches!(pending.purpose, Purpose::CertificateBuckets)
            });
            match self.queue.submit(command) {
                Ok(task) => {
                    dialog.bucket_task = Some(task);
                    self.pending.insert(
                        task,
                        Pending {
                            tab_id: id.into(),
                            purpose,
                        },
                    );
                }
                Err(error) => {
                    dialog.bucket_task = None;
                    dialog.bucket_error = Some(format!("Bucket 读取未提交：{error}"));
                }
            }
            return;
        }
        let saving = if let Command::SaveQueryRows { result, .. } = &command {
            let Some(Panel::Sql { workbench, .. }) = self
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| &tab.panel)
            else {
                return;
            };
            purpose = Purpose::QuerySave {
                result: *result,
                generation: workbench.results_generation,
            };
            Some(*result)
        } else {
            None
        };
        if let Command::Sql { connection, .. } | Command::SqlScript { connection, .. } = &command {
            let namespace = self
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .and_then(|tab| {
                    if let Panel::Sql { workbench, .. } = &tab.panel {
                        Some(workbench.schema.as_str())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            purpose =
                Purpose::SqlRequest(tabs::sql_scope::ResultContext::new(connection, namespace));
        }
        let execution = match &command {
            Command::SqlScript { mode, .. } => Some(*mode),
            Command::Sql { .. } => Some(tomato_services::database::script::Mode::Run),
            _ => None,
        };
        let completion_quiet = matches!(
            &command,
            Command::Database {
                action: tomato_core::workbench::DatabaseAction::Completion { .. },
                ..
            }
        );
        if completion_quiet {
            let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            let Panel::Sql { workbench, .. } = &mut tab.panel else {
                return;
            };
            workbench.completion.generation = workbench.completion.generation.wrapping_add(1);
            // A background refresh in the current scope can finish a live keyboard
            // request. Scope changes already clear the old catalog and request.
            purpose = Purpose::SqlCompletion(workbench.completion.generation);
        }
        let quiet = completion_quiet || matches!(purpose, Purpose::DesignSql(_));
        let hosts_command = matches!(&command, Command::Hosts { .. });
        match self.queue.submit(command) {
            Ok(task) => {
                self.pending.retain(|_, pending| {
                    pending.tab_id != id
                        || if quiet {
                            !matches!(
                                (&purpose, &pending.purpose),
                                (Purpose::SqlCompletion(_), Purpose::SqlCompletion(_))
                                    | (Purpose::DesignSql(_), Purpose::DesignSql(_))
                            )
                        } else {
                            matches!(
                                pending.purpose,
                                Purpose::Markdown(_)
                                    | Purpose::MarkdownImage(..)
                                    | Purpose::SqlCompletion(_)
                                    | Purpose::ObjectMutation(_)
                                    | Purpose::AiSql(_)
                                    | Purpose::QuerySave { .. }
                                    | Purpose::CertificateBuckets
                                    | Purpose::DesignSql(_)
                                    | Purpose::ObjectFields(_)
                            )
                        }
                });
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: id.into(),
                        purpose,
                    },
                );
                if !quiet && let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.busy = true;
                    tab.error = None;
                    if let Panel::Sql { workbench, .. } = &mut tab.panel {
                        workbench.execution = execution;
                    }
                    if let (Some(index), Panel::Sql { workbench, .. }) = (saving, &mut tab.panel)
                        && let Some(state) = workbench.query_results.get_mut(index)
                    {
                        state.saving = true;
                    }
                }
            }
            Err(error) => {
                if matches!(purpose, Purpose::Certificate)
                    && let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                    && let Panel::Cloud { view, .. } = &mut tab.panel
                    && let Some(dialog) = &mut view.certificate
                {
                    view.control = None;
                    dialog.issue_error = Some(format!("证书申请未提交：{error}"));
                    return;
                }
                if hosts_command
                    && let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                    && let Panel::Tool(tool) = &mut tab.panel
                    && let Some(view) = &mut tool.hosts
                    && view.snapshot.is_some()
                {
                    view.fail(&self.context, &error);
                    return;
                }
                if matches!(purpose, Purpose::ToolConversion)
                    && let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                    && let Panel::Tool(tool) = &mut tab.panel
                {
                    tool.converter
                        .get_or_insert_with(Default::default)
                        .fail(&error.to_string());
                    return;
                }
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                    && let Panel::Sql { workbench, .. } = &mut tab.panel
                    && let Some(browser) = &mut workbench.browser
                {
                    browser.notice = Some(format!("操作未提交: {error}"));
                }
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                    && let Panel::Sql { workbench, .. } = &mut tab.panel
                    && let Some(draft) = &mut workbench.designer
                {
                    draft.busy = false;
                    draft.saving = false;
                    draft.status = format!("操作未提交: {error}");
                    if matches!(purpose, Purpose::DesignSql(_)) {
                        draft.fail_sql(draft.status.clone());
                    }
                }
                self.error = Some(error.to_string());
            }
        }
    }
    pub(super) fn handle_intent(&mut self, id: &str, intent: Intent) {
        match intent {
            Intent::ImportData => self.open_table_data_import(id),
            Intent::ExportData(result) => self.open_data_export(id, result),
            Intent::Objects(action) => self.object_action(id, action),
            Intent::SaveDesign => self.save_design(id, None),
            Intent::CreateDesign(name) => self.save_design(id, Some(name)),
            Intent::RefreshDesign => self.refresh_design(id),
            Intent::RefreshDesignInfo(page) => self.refresh_design_info(id, page),
            Intent::LoadDesignSql => self.load_design_sql(id),
            Intent::SqlConnection(name) => self.choose_sql_connection(id, &name),
            Intent::SqlDatabase(name) => self.choose_sql_database(id, &name),
            Intent::AiSql { selected_prompt } => self.request_ai_sql(id, selected_prompt),
            Intent::AiSqlSettings => self.open_ai_settings(None),
            Intent::SshInspect { connection, id } => self.open_container_inspector(connection, id),
            Intent::SshCommand {
                connection,
                command,
                title,
            } => {
                if let Ok(mut tab) =
                    tabs::create_tab(connection, self.runtime.as_ref().map(|r| r.handle()))
                {
                    tab.title = title;
                    terminal_config::apply(&mut tab, &self.settings);
                    if let Panel::Terminal { session, view, .. } = &mut tab.panel {
                        view.initial_command = Some(command);
                        let ctx = self.context.clone();
                        session.on_output(move || ctx.request_repaint());
                    }
                    self.active = Some(tab.id.clone());
                    self.tabs.push(tab);
                }
            }
            Intent::Close => self.close(id),
            Intent::ConnectDesktop => self.connect_desktop(id),
            Intent::ToggleAdbWindow => self.toggle_adb_window(id),
            Intent::SubmitVncPassword => self.submit_vnc_password(id),
            Intent::ReconnectTerminal => {
                if let Some(runtime) = &self.runtime
                    && let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                    && let Panel::Terminal {
                        connection,
                        session,
                        view,
                        ..
                    } = &mut tab.panel
                {
                    if let Some(latest) = self
                        .store
                        .connections
                        .iter()
                        .find(|c| c.id == connection.id)
                    {
                        *connection = latest.clone();
                    }
                    let started = if view.ssh.as_ref().is_some_and(|w| w.standalone.is_some()) {
                        tomato_services::terminal::Terminal::start_tools(
                            connection,
                            runtime.handle(),
                        )
                    } else {
                        tomato_services::terminal::Terminal::start(connection, runtime.handle())
                    };
                    match started {
                        Ok(new_session) => {
                            let repaint = self.context.clone();
                            new_session.on_output(move || repaint.request_repaint());
                            *session = new_session;
                            if let Some(workspace) = &mut view.ssh {
                                workspace.reconnected();
                            }
                            view.answers.clear();
                            view.composition.clear();
                            view.focused_once = false;
                            view.selection = None;
                            tab.error = None;
                        }
                        Err(error) => tab.error = Some(format!("{error:#}")),
                    }
                }
                self.rebind_file_documents(id);
            }
            Intent::ConfigureMusic => {
                if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id)
                    && let Some(connection) = self
                        .store
                        .connections
                        .iter()
                        .find(|c| c.id == tab.source_id)
                {
                    self.edit_connection(connection.clone());
                }
            }
            Intent::Command(command) => {
                let purpose = crate::tabs::json::feedback::Feedback::for_command(&command)
                    .map(Purpose::Json)
                    .unwrap_or(Purpose::Normal);
                self.submit(id, *command, purpose);
            }
            Intent::Markdown { source, revision } => {
                match self.queue.submit(Command::Markdown { source }) {
                    Ok(task) => {
                        self.pending.insert(
                            task,
                            Pending {
                                tab_id: id.into(),
                                purpose: Purpose::Markdown(revision),
                            },
                        );
                    }
                    Err(error) => {
                        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                            && let Panel::Editor { markdown, .. } = &mut tab.panel
                        {
                            markdown.pending = false;
                            tab.error = Some(error.to_string());
                        }
                    }
                }
            }
            Intent::MarkdownImage(source) => {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
                    && let Panel::Editor { path, markdown, .. } = &mut tab.panel
                {
                    let command = if let Some(client) = &markdown.file_client {
                        Command::FileDocument {
                            client: Some(client.clone()),
                            action: tomato_services::files::document::Action::Image {
                                document: path.to_string_lossy().replace('\\', "/"),
                                source: source.clone(),
                            },
                        }
                    } else if let Some(connection) = &markdown.remote {
                        Command::ObjectTree {
                            connection: connection.as_ref().clone(),
                            action: tomato_services::files::object_tree::Request::Image {
                                document: path.clone(),
                                source: source.clone(),
                            },
                        }
                    } else {
                        Command::MarkdownImage {
                            document: path.clone(),
                            source: source.clone(),
                        }
                    };
                    match self.queue.submit(command) {
                        Ok(task) => {
                            self.pending.insert(
                                task,
                                Pending {
                                    tab_id: id.into(),
                                    purpose: Purpose::MarkdownImage(
                                        source,
                                        markdown.resource_generation,
                                    ),
                                },
                            );
                        }
                        Err(error) => {
                            markdown.image_failed(source, error.to_string());
                        }
                    }
                }
            }
            Intent::PrintMarkdown => self.start_markdown_print(id),
            Intent::ExportMarkdownHtml => self.start_markdown_html(id),
            Intent::Edit(path) => self.open_editor(path),
            Intent::OpenFileDocument(path) => self.open_file_document(id, path),
            Intent::LoadSql(path) => self.submit(id, Command::ReadText { path }, Purpose::SqlRead),
            Intent::SaveQuery => self.query_action(query_tree::Action::Save(id.into())),
            Intent::Save(path, text) => {
                let remote =
                    self.tabs
                        .iter()
                        .find(|tab| tab.id == id)
                        .and_then(|tab| match &tab.panel {
                            Panel::Editor { markdown, .. } => markdown.remote.clone(),
                            _ => None,
                        });
                let file_client =
                    self.tabs
                        .iter()
                        .find(|tab| tab.id == id)
                        .and_then(|tab| match &tab.panel {
                            Panel::Editor { markdown, .. } => markdown.file_client.clone(),
                            _ => None,
                        });
                let command = if let Some(client) = file_client {
                    Command::FileDocument {
                        client: Some(client),
                        action: tomato_services::files::document::Action::Write {
                            path: path.to_string_lossy().replace('\\', "/"),
                            text: text.clone(),
                        },
                    }
                } else if let Some(connection) = remote {
                    Command::ObjectTree {
                        connection: connection.as_ref().clone(),
                        action: tomato_services::files::object_tree::Request::Write {
                            path,
                            text: text.clone(),
                        },
                    }
                } else {
                    Command::WriteText {
                        path,
                        text: text.clone(),
                    }
                };
                self.submit(id, command, Purpose::Save(text));
            }
            Intent::ConfigureServer(config) => {
                self.configure_server(id, &config);
            }
            Intent::StartConfigured(config) => self.start_configured_server(id, config),
        }
    }
    pub(super) fn close(&mut self, id: &str) {
        self.close_tab(id, true);
    }
    pub(super) fn close_tab(&mut self, id: &str, disconnect: bool) {
        if !self.close_file_children(id) {
            return;
        }
        self.closing_file_document(id);
        if !self.flush_server_config(id) {
            return;
        }
        if disconnect
            && let Some(tab) = self.tabs.iter().find(|tab| tab.id == id)
            && let Panel::Sql { connection, .. } = &tab.panel
        {
            let session_id = connection.id.as_str();
            let used_elsewhere = self.tabs.iter().any(|other| other.id != id && matches!(&other.panel, Panel::Sql { connection, .. } if connection.id == session_id));
            let tree_owns_session = self
                .sql_hosts
                .get(session_id)
                .is_some_and(|host| host.pg.is_some() || host.direct.is_some());
            if !used_elsewhere
                && !tree_owns_session
                && let Err(error) = self.queue.submit(Command::Disconnect {
                    id: session_id.into(),
                })
            {
                self.error = Some(format!(
                    "关闭数据库会话失败，请等待后台任务完成后重试：{error:#}"
                ));
                return;
            }
        }
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id)
            && let Panel::Editor { markdown, .. } = &tab.panel
        {
            // egui retains TextEditState by ID, including complete undo snapshots.
            // Dropping the tab alone would leave those strings in Context memory.
            for editor_id in std::iter::once(markdown.editor_id)
                .chain(markdown.find_field_ids)
                .flatten()
            {
                self.context.data_mut(|data| {
                    data.remove::<egui::widgets::text_edit::TextEditState>(editor_id)
                });
            }
        }
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) {
            match &tab.panel {
                Panel::Mongo { view, .. } => view.release(&self.context),
                Panel::MessageQueue { view, .. } => view.release(&self.context),
                Panel::Sql { workbench, .. } => {
                    if let Some(browser) = &workbench.browser {
                        browser.release(&self.context);
                    }
                    if let Some(designer) = &workbench.designer {
                        designer.release(&self.context);
                    }
                    workbench.completion.editor.release(&self.context);
                    workbench.selectors.release(&self.context);
                    for state in &workbench.query_results {
                        state.release(&self.context);
                    }
                }
                Panel::Redis { view, .. } => view.release(&self.context),
                Panel::NativeDesktop { view, .. } => {
                    if let Some(editor) = &view.vnc_password_editor {
                        editor.release(&self.context);
                    }
                }
                Panel::Terminal { view, .. } => {
                    if let Some(workspace) = &view.ssh {
                        for pane in workspace.panes.values() {
                            if let Some(edit) = &pane.rename {
                                edit.release(&self.context);
                            }
                        }
                    }
                }
                Panel::Tool(tool) => {
                    if let Some(hosts) = &tool.hosts {
                        hosts.editor.release(&self.context);
                        hosts.rename_editor.release(&self.context);
                        if let Some(prompt) = &hosts.authorization {
                            prompt.control.cancel();
                            prompt.editor.release(&self.context);
                        }
                    }
                }
                _ => {}
            }
        }
        // TabPane chooses the closest remaining tab to the left of a closed active tab.
        let replacement = self
            .tabs
            .iter()
            .position(|tab| tab.id == id)
            .and_then(|index| {
                let visible = |tab: &&Tab| match self.section {
                    1 => tab.source_id.starts_with("tool:"),
                    2 => tab.source_id.starts_with("server:"),
                    _ => tab.workspace_visible(),
                };
                self.tabs[..index]
                    .iter()
                    .rev()
                    .find(visible)
                    .or_else(|| self.tabs[index + 1..].iter().find(visible))
                    .map(|tab| tab.id.clone())
            });
        if self.tabs.iter().any(|tab| {
            tab.id == id
                && matches!(&tab.panel,Panel::NativeDesktop{view,..}if view.window.is_some())
        }) {
            self.context
                .send_viewport_cmd_to(rdp_windows::viewport(id), egui::ViewportCommand::Close);
        }
        self.close_session_dialog(id);
        self.tabs.retain(|tab| tab.id != id);
        let live = &self.tabs;
        self.sql_hosts.retain(|key, host| {
            host.pg.is_some()
                || host.direct.is_some()
                || live.iter().any(
                    |tab| matches!(&tab.panel,Panel::Sql{connection,..} if &connection.id==key),
                )
        });

        self.pending.retain(|_, task| {
            task.tab_id != id || matches!(task.purpose, Purpose::ObjectMutation(_))
        });
        if self.active.as_deref() == Some(id) {
            self.active = replacement;
        }
    }
}

impl TomatoApp {
    pub(super) fn query_result_dialogs(&mut self, ctx: &egui::Context) {
        for tab in &mut self.tabs {
            if let Panel::Sql {
                results, workbench, ..
            } = &mut tab.panel
            {
                for (index, state) in workbench.query_results.iter_mut().enumerate() {
                    if (state.view.dialog.is_some()
                        || state
                            .editing
                            .as_ref()
                            .and_then(|editing| editing.editor.as_ref())
                            .is_some_and(|editor| editor.temporal.is_some()))
                        && let Some(result) = results.get(index)
                    {
                        crate::panels::render_query_result_dialog(
                            ctx,
                            result,
                            state,
                            egui::Id::new((&tab.id, "result-dialog", index)),
                        );
                    }
                }
            }
        }
    }
}

impl TomatoApp {
    pub(super) fn poll_desktop(tab: &mut Tab, ctx: &egui::Context, root_active: bool) {
        if let Panel::NativeDesktop {
            session: Some(session),
            view,
            ..
        } = &mut tab.panel
        {
            session.poll();
            if let Some(window) = &mut view.window {
                let status = session.status();
                if window.last_status.as_ref() != Some(&status) {
                    window.last_status = Some(status);
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            }
            let active = view.window.as_ref().map_or_else(
                || root_active && ctx.input(|input| input.focused),
                |window| window.focused,
            );
            if view.focused && !active {
                let mut releases = Vec::new();
                view.release_inputs(&mut |input| releases.push(input));
                view.queue_inputs(releases);
            }
            if session.status() == tomato_services::remote_desktop::Status::Ready {
                for _ in 0..32 {
                    let Some(input) = view.pending.front().copied() else {
                        break;
                    };
                    if session.send(input).is_err() {
                        break;
                    }
                    view.pending.pop_front();
                }
                if !view.pending.is_empty() {
                    ctx.request_repaint_after(std::time::Duration::from_millis(5));
                }
            } else {
                view.pending.clear();
            }
            if let Some(text) = session.take_clipboard() {
                ctx.copy_text(text);
            }
            if session.take_bell() && !tomato_services::remote_desktop::play_system_bell() {
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
            }
        }
    }
}
