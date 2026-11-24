//! Workspace cloud controller. The view emits commands; persistence and HTTP run in its actor.
use super::*;
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};
use tomato_services::collaboration as cloud;
mod fixture;
#[cfg(test)]
mod tests;
mod view;

pub(super) struct Draft {
    pub id: String,
    pub project: String,
    pub parent: String,
    pub kind: cloud::Kind,
    pub name: String,
    pub text: String,
    pub revision: i64,
    pub baseline: String,
    pub saving: Option<(String, String)>,
    pub envelope: serde_json::Value,
    pub source: Option<Arc<cloud::Object>>,
}
impl Draft {
    fn from_object(o: &Arc<cloud::Object>) -> Self {
        let text = if matches!(
            o.kind,
            cloud::Kind::Query | cloud::Kind::Snippet | cloud::Kind::Pipeline
        ) {
            o.body["text"].as_str().unwrap_or_default().into()
        } else if o.body["base64"].is_string() {
            String::new()
        } else {
            serde_json::to_string_pretty(&o.body).unwrap_or_default()
        };
        let mut draft = Self {
            id: o.id.clone(),
            project: o.project_id.clone(),
            parent: o.parent_id.clone(),
            kind: o.kind,
            name: o.name.clone(),
            revision: o.revision,
            baseline: String::new(),
            text,
            saving: None,
            envelope: if matches!(
                o.kind,
                cloud::Kind::Query | cloud::Kind::Snippet | cloud::Kind::Pipeline
            ) {
                metadata_body(&o.body)
            } else {
                serde_json::Value::Null
            },
            source: Some(o.clone()),
        };
        draft.baseline = draft.fingerprint();
        draft
    }
    fn fingerprint(&self) -> String {
        format!(
            "{}:{}:{}",
            tomato_services::queries::stamp(self.name.as_bytes()),
            tomato_services::queries::stamp(self.parent.as_bytes()),
            tomato_services::queries::stamp(self.text.as_bytes())
        )
    }
    fn dirty(&self) -> bool {
        self.fingerprint() != self.baseline
    }
    fn mutation(&self) -> anyhow::Result<cloud::Mutation> {
        let mutation = cloud::Mutation {
            parent_id: self.parent.clone(),
            kind: self.kind,
            name: self.name.clone(),
            body: if matches!(
                self.kind,
                cloud::Kind::Query | cloud::Kind::Snippet | cloud::Kind::Pipeline
            ) {
                let mut body = self.envelope.clone();
                if !body.is_object() {
                    body = serde_json::json!({})
                }
                body["text"] = self.text.clone().into();
                body
            } else if let Some(source) = self
                .source
                .as_ref()
                .filter(|o| o.body["base64"].is_string())
            {
                source.body.clone()
            } else {
                serde_json::from_str(&self.text)?
            },
            base_revision: self.revision,
            mutation_id: cloud::new_id(),
            deleted: false,
        };
        mutation.validate()?;
        Ok(mutation)
    }
}
struct Query {
    object: cloud::Object,
    acknowledged: String,
    mutation: String,
}
pub(super) struct State {
    client: cloud::Client,
    view: Arc<cloud::View>,
    open: bool,
    base: String,
    email: String,
    password: String,
    code: String,
    name: String,
    filter: String,
    project_name: String,
    invite_email: String,
    invite_role: cloud::Role,
    target: String,
    selected: BTreeSet<String>,
    draft: Option<Draft>,
    detail: Option<&'static str>,
    error: Option<String>,
    queries: HashMap<String, Query>,
    applied: HashMap<String, String>,
    local_pending: HashMap<String, String>,
    local_mismatches: HashMap<String, String>,
    loaded: String,
    confirm: Option<Action>,
    account: String,
    fixture: Option<fixture::Fixture>,
}
enum Action {
    Send(cloud::Command),
    Edit(String),
    New(cloud::Kind),
    Save,
    Import(cloud::Kind),
    ReplaceFile,
    Export(String),
    Bind,
    OpenQuery,
    Delete(String),
    ConfirmDelete(String),
    Transfer(bool),
    Portal,
    PublishConnection,
    PublishQuery,
    PublishPersonal,
}
impl State {
    fn send(&mut self, command: cloud::Command) {
        if let Err(e) = self.client.send(command) {
            self.error = Some(e.to_string());
        }
    }
    fn reset_draft(&mut self) {
        self.draft = None;
        self.loaded.clear();
    }
}
impl TomatoApp {
    pub(super) fn open_collaboration(&mut self) {
        if let Some(state) = &mut self.collaboration {
            state.open = true;
            return;
        }
        let Some(runtime) = &self.runtime else { return };
        let context = self.context.clone();
        let root = self
            .repository
            .location()
            .unwrap_or(&Store::default_path())
            .with_file_name("collaboration");
        self.collaboration = Some(Box::new(State {
            client: cloud::Client::start(runtime.handle(), root, move || context.request_repaint()),
            view: Arc::new(Default::default()),
            open: true,
            base: "http://127.0.0.1:8443".into(),
            email: String::new(),
            password: String::new(),
            code: String::new(),
            name: String::new(),
            filter: String::new(),
            project_name: String::new(),
            invite_email: String::new(),
            invite_role: cloud::Role::Editor,
            target: String::new(),
            selected: Default::default(),
            draft: None,
            detail: None,
            error: None,
            queries: Default::default(),
            applied: Default::default(),
            local_pending: Default::default(),
            local_mismatches: Default::default(),
            loaded: String::new(),
            confirm: None,
            account: String::new(),
            fixture: None,
        }));
    }
    pub(super) fn collaboration_ui(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.collaboration.take() else {
            return;
        };
        if let Some(view) = state.client.changed() {
            state.view = view;
            self.cloud_apply(&mut state);
        }
        self.cloud_fixture(&mut state, ctx);
        let mut action = None;
        let was_open = state.open;
        if state.open {
            let mut open = true;
            egui::Window::new("Tomato Cloud · 云端协作")
                .id(egui::Id::new("tomato-cloud"))
                .open(&mut open)
                .default_size([980., 760.])
                .vscroll(true)
                .min_size([680., 480.])
                .show(ctx, |ui| {
                    action = view::show(ui, &mut state, &mut self.icons);
                });
            state.open = open;
        }
        if was_open
            && !state.open
            && state
                .draft
                .as_ref()
                .is_none_or(|d| !d.dirty() && d.saving.is_none())
        {
            state.reset_draft();
            state.send(cloud::Command::Unload);
        }
        if let Some(action) = action {
            self.cloud_action(&mut state, action);
        }
        if state.confirm.is_some() {
            egui::Window::new("确认删除")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("删除后同步到全部成员。请先删除子对象；最近版本仍保留在历史中。");
                    ui.horizontal(|ui| {
                        if ui.button("删除").clicked()
                            && let Some(action) = state.confirm.take()
                        {
                            self.cloud_action(&mut state, action)
                        }
                        if ui.button("取消").clicked() {
                            state.confirm = None;
                        }
                    });
                });
        }
        if let Some(error) = state.error.clone() {
            egui::Window::new("云端协作操作失败")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(error);
                    if ui.button("确定").clicked() {
                        state.error = None;
                    }
                });
        }
        state
            .queries
            .retain(|tab, _| self.tabs.iter().any(|t| t.id == *tab));
        self.collaboration = Some(state);
    }
    fn cloud_action(&mut self, state: &mut State, action: Action) {
        let result = (|| -> anyhow::Result<()> {
            match action {
                Action::PublishPersonal => {
                    let mut object = state
                        .view
                        .personal
                        .as_deref()
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("请先登录并同步个人配置"))?;
                    let body =
                        cloud::personal_body(&self.settings, &state.view.bindings, &self.store);
                    if !object.body.is_object() {
                        object.body = serde_json::json!({});
                    }
                    object.body["appearance"] = body["appearance"].clone();
                    object.body["connections"] = body["connections"].clone();
                    state.send(cloud::Command::Save {
                        project: "@personal".into(),
                        id: "@personal".into(),
                        mutation: cloud::Mutation::from_object(&object),
                    });
                }
                Action::PublishConnection => {
                    let c = self
                        .selected_connection
                        .as_ref()
                        .and_then(|id| self.store.connections.iter().find(|c| &c.id == id))
                        .ok_or_else(|| anyhow::anyhow!("请先在左侧连接树选中一个连接"))?;
                    state.send(cloud::Command::Save {
                        project: state.view.selected.clone(),
                        id: cloud::new_id(),
                        mutation: cloud::Mutation {
                            parent_id: String::new(),
                            kind: if c.kind.is_none() {
                                cloud::Kind::Group
                            } else {
                                cloud::Kind::Connection
                            },
                            name: c.name.clone(),
                            body: if c.kind.is_none() {
                                serde_json::json!({})
                            } else {
                                cloud::connection_body_in_store(c, &self.store)
                            },
                            base_revision: 0,
                            mutation_id: cloud::new_id(),
                            deleted: false,
                        },
                    });
                }
                Action::PublishQuery => {
                    let tab = self
                        .tabs
                        .iter()
                        .find(|t| Some(&t.id) == self.active.as_ref())
                        .ok_or_else(|| anyhow::anyhow!("请先打开查询编辑器"))?;
                    let Panel::Sql {
                        sql, connection, ..
                    } = &tab.panel
                    else {
                        anyhow::bail!("当前标签不是 SQL 查询")
                    };
                    let parent = state
                        .view
                        .bindings
                        .iter()
                        .find(|(local, _)| local == &connection.id)
                        .map(|(_, o)| o.id.clone())
                        .unwrap_or_default();
                    state.send(cloud::Command::Save{project:state.view.selected.clone(),id:cloud::new_id(),mutation:cloud::Mutation{parent_id:parent,kind:cloud::Kind::Query,name:tab.title.trim_start_matches('*').into(),body:serde_json::json!({"text":sql,"database":connection.text("database")}),base_revision:0,mutation_id:cloud::new_id(),deleted:false}});
                }
                Action::Send(command) => {
                    if let cloud::Command::Connect { base, email, .. }
                    | cloud::Command::Offline { base, email } = &command
                    {
                        let api = cloud::Api::new(base)?;
                        let key = format!("{}:{}", api.base(), email.trim().to_lowercase());
                        anyhow::ensure!(
                            state.queries.is_empty() || state.account == key,
                            "请先关闭当前云账号的查询标签，再切换服务器或账号"
                        );
                        anyhow::ensure!(
                            state.draft.as_ref().is_none_or(|d| !d.dirty()),
                            "请先保存当前草稿"
                        );
                        if state.account != key {
                            state.applied.clear();
                            state.reset_draft();
                        }
                        state.account = key;
                    }
                    if matches!(command, cloud::Command::ClearCache) {
                        anyhow::ensure!(
                            state.queries.is_empty(),
                            "请先关闭云端查询标签，再清理离线副本"
                        );
                    }
                    state.client.send(command)?;
                }
                Action::Edit(id) => {
                    state.reset_draft();
                    state.send(cloud::Command::Load(id));
                }
                Action::New(kind) => {
                    anyhow::ensure!(!state.view.selected.is_empty(), "请先选择项目");
                    let body = if kind == cloud::Kind::Connection {
                        serde_json::json!({"type":"MYSQL","host":"127.0.0.1","port":3306,"username":"","database":""})
                    } else {
                        serde_json::json!({"text":""})
                    };
                    let text = if matches!(
                        kind,
                        cloud::Kind::Query | cloud::Kind::Snippet | cloud::Kind::Pipeline
                    ) {
                        String::new()
                    } else {
                        serde_json::to_string_pretty(&body)?
                    };
                    state.draft = Some(Draft {
                        id: cloud::new_id(),
                        project: state.view.selected.clone(),
                        parent: String::new(),
                        kind,
                        name: format!("新建{}", kind.label()),
                        text,
                        baseline: String::new(),
                        revision: 0,
                        saving: None,
                        envelope: body,
                        source: None,
                    });
                    if let Some(draft) = &mut state.draft {
                        draft.baseline = draft.fingerprint();
                    }
                    state.loaded = "new".into();
                }
                Action::Save => {
                    let draft = state
                        .draft
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("请打开一个对象"))?;
                    let mutation = draft.mutation()?;
                    let accepted = mutation.mutation_id.clone();
                    state.client.send(cloud::Command::Save {
                        project: draft.project.clone(),
                        id: draft.id.clone(),
                        mutation,
                    })?;
                    draft.saving = Some((accepted, draft.fingerprint()));
                }
                Action::Bind => {
                    let object = state
                        .view
                        .document
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("请打开连接对象"))?;
                    anyhow::ensure!(
                        matches!(object.kind, cloud::Kind::Connection | cloud::Kind::Group),
                        "此对象不是连接或分组"
                    );
                    let mut nodes = vec![(**object).clone()];
                    let mut parent = object.parent_id.clone();
                    for _ in 0..63 {
                        if parent.is_empty() {
                            break;
                        }
                        let Some(node) =
                            state.view.items.iter().find(|o| {
                                o.object.id == parent && o.object.kind == cloud::Kind::Group
                            })
                        else {
                            break;
                        };
                        nodes.push(node.object.clone());
                        parent = node.object.parent_id.clone();
                    }
                    let mut next = self.store.clone();
                    let mut bindings = Vec::new();
                    for object in nodes.iter().rev() {
                        let local = local_id(state, &object.id);
                        if !next.connections.iter().any(|c| c.id == local) {
                            let mut c =
                                Connection::new(object.body["type"].as_str().unwrap_or("MYSQL"));
                            c.id = local.clone();
                            next.connections.push(c);
                        };
                        let c = next.connections.iter_mut().find(|c| c.id == local).unwrap();
                        cloud::apply_connection(c, object)?;
                        if !object.parent_id.is_empty() {
                            let group = local_id(state, &object.parent_id);
                            if next
                                .connections
                                .iter()
                                .any(|c| c.id == group && c.kind.is_none())
                            {
                                next.connections
                                    .iter_mut()
                                    .find(|c| c.id == local)
                                    .unwrap()
                                    .parent_id = Some(group);
                            }
                        }
                        bindings.push((object.id.clone(), local));
                    }
                    if self.persist_store(next) {
                        state.send(cloud::Command::BindMany(bindings));
                    }
                }
                Action::OpenQuery => self.open_cloud_query(state)?,
                Action::Delete(id) => {
                    state.confirm = Some(Action::ConfirmDelete(id));
                }
                Action::ConfirmDelete(id) => {
                    state.send(cloud::Command::Delete(vec![id]));
                }
                Action::Transfer(copy) => {
                    anyhow::ensure!(
                        !state.selected.is_empty() && !state.target.is_empty(),
                        "请选择对象与目标项目"
                    );
                    state.send(cloud::Command::Transfer {
                        project: state.view.selected.clone(),
                        objects: state.selected.iter().cloned().collect(),
                        target: state.target.clone(),
                        copy,
                    });
                }
                Action::Portal => {
                    let base = state.base.clone();
                    if let Some(runtime) = &self.runtime {
                        runtime.spawn_blocking(move || {
                            let _ = open::that(base);
                        });
                    }
                }
                Action::Import(kind) => {
                    let client = state.client.dispatcher();
                    let project = state.view.selected.clone();
                    let repaint = ctx_clone(&self.context);
                    if let Some(runtime) = &self.runtime {
                        runtime.spawn(async move {
                            if let Some(file) = rfd::AsyncFileDialog::new().pick_file().await {
                                let _ = client
                                    .send(cloud::Command::Import {
                                        project,
                                        path: file.path().into(),
                                        kind,
                                        replace: None,
                                    })
                                    .await;
                            }
                            repaint.request_repaint();
                        });
                    }
                }
                Action::ReplaceFile => {
                    let draft = state
                        .draft
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("请先打开对象"))?;
                    let project = draft.project.clone();
                    let kind = draft.kind;
                    let id = draft.id.clone();
                    let revision = draft.revision;
                    let client = state.client.dispatcher();
                    if let Some(runtime) = &self.runtime {
                        runtime.spawn(async move {
                            if let Some(file) = rfd::AsyncFileDialog::new().pick_file().await {
                                let _ = client
                                    .send(cloud::Command::Import {
                                        project,
                                        path: file.path().into(),
                                        kind,
                                        replace: Some((id, revision)),
                                    })
                                    .await;
                            }
                        });
                    }
                }
                Action::Export(id) => {
                    let client = state.client.dispatcher();
                    let repaint = ctx_clone(&self.context);
                    if let Some(runtime) = &self.runtime {
                        runtime.spawn(async move {
                            if let Some(file) = rfd::AsyncFileDialog::new()
                                .set_file_name("workspace.json")
                                .save_file()
                                .await
                            {
                                let _ = client
                                    .send(cloud::Command::Export {
                                        id,
                                        path: file.path().into(),
                                    })
                                    .await;
                            }
                            repaint.request_repaint();
                        });
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            state.error = Some(format!("{error:#}"));
        }
    }
    fn cloud_apply(&mut self, state: &mut State) {
        state.local_pending.retain(|id, stamp| {
            !state
                .view
                .local_accepted
                .iter()
                .any(|(accepted, value)| accepted == id && value == stamp)
                && state.view.baselines.get(id) != Some(stamp)
        });
        if let Some(personal) = &state.view.personal
            && personal.revision > 0
        {
            let stamp = tomato_services::queries::stamp(
                &serde_json::to_vec(&personal.body).unwrap_or_default(),
            );
            if state.applied.get("@personal") != Some(&stamp) {
                let result = (|| -> anyhow::Result<()> {
                    let next = cloud::apply_personal(&self.settings, &personal.body)?;
                    if serde_json::to_value(&next)? != serde_json::to_value(&self.settings)? {
                        self.preferences.save(&next)?;
                        self.settings = next;
                        self.settings_form = None;
                        crate::font_assets::terminal::ensure(
                            &self.context,
                            &self.settings.ssh_terminal_font_name,
                        );
                        for tab in &mut self.tabs {
                            terminal_config::apply_runtime(tab, &self.settings);
                        }
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        state.applied.insert("@personal".into(), stamp);
                    }
                    Err(e) => state.error = Some(format!("同步个人配置失败：{e:#}")),
                }
            }
        }
        if let Some(draft) = &mut state.draft
            && let Some((id, text)) = &draft.saving
            && state.view.accepted.contains(id)
        {
            draft.baseline = text.clone();
            draft.saving = None;
        }
        if state.open
            && let Some(object) = &state.view.document
        {
            let identity = format!("{}:{}", object.id, object.revision);
            if state.loaded != identity && state.draft.as_ref().is_none_or(|d| !d.dirty()) {
                state.draft = Some(Draft::from_object(object));
                state.loaded = identity;
            }
        }
        let mut next = self.store.clone();
        let mut changed = false;
        let mut applied = Vec::new();
        if let Some(personal) = &state.view.personal
            && personal.revision > 0
        {
            for (local, object) in &state.view.bindings {
                if let Some(options) = personal.body["connections"][&object.id].as_object()
                    && let Some(connection) = next.connections.iter_mut().find(|c| &c.id == local)
                {
                    for key in cloud::PERSONAL_CONNECTION_KEYS {
                        if let Some(value) = options.get(*key)
                            && connection.options.get(*key) != Some(value)
                        {
                            connection.options.insert((*key).into(), value.clone());
                            changed = true;
                        }
                    }
                }
            }
        }
        for (local, object) in &state.view.bindings {
            if state.view.local_origins.contains(&object.id)
                || state.local_pending.contains_key(&object.id)
            {
                continue;
            }
            if let Some(connection) = self.store.connections.iter().find(|c| &c.id == local) {
                let body = if object.kind == cloud::Kind::Group {
                    object.body.clone()
                } else {
                    cloud::connection_body_in_store(connection, &self.store)
                };
                let stamp = cloud::connection_stamp(&connection.name, &body);
                if state
                    .view
                    .baselines
                    .get(&object.id)
                    .is_some_and(|baseline| baseline != &stamp)
                    && stamp != cloud::connection_stamp(&object.name, &object.body)
                {
                    // The persisted legacy file is authoritative. A stale GUI
                    // must not upload its old projection over an external edit.
                    if state.local_mismatches.get(&object.id) != Some(&stamp) {
                        state.local_mismatches.insert(object.id.clone(), stamp);
                        let _ = state.client.send(cloud::Command::Refresh);
                    }
                    continue;
                }
                state.local_mismatches.remove(&object.id);
            }
            let stamp =
                tomato_services::queries::stamp(&serde_json::to_vec(object).unwrap_or_default());
            if state.applied.get(local) == Some(&stamp) {
                continue;
            }
            let parent = state
                .view
                .bindings
                .iter()
                .find(|(_, o)| o.id == object.parent_id && o.kind == cloud::Kind::Group)
                .map(|(local, _)| local.clone());
            if let Some(c) = next.connections.iter_mut().find(|c| &c.id == local) {
                match cloud::apply_connection(c, object) {
                    Ok(()) => {
                        if parent.is_some()
                            || c.parent_id.as_ref().is_some_and(|old| {
                                state
                                    .view
                                    .bindings
                                    .iter()
                                    .any(|(local, o)| local == old && o.kind == cloud::Kind::Group)
                            })
                        {
                            c.parent_id = parent;
                        }
                        applied.push((local.clone(), stamp));
                        changed = true;
                    }
                    Err(e) => state.error = Some(e.to_string()),
                }
            }
        }
        if changed && self.persist_store(next) {
            state.applied.extend(applied);
        }
        let mut close = Vec::new();
        for (tab_id, query) in &mut state.queries {
            if let Some(item) = state.view.items.iter().find(|o| {
                o.object.id == query.object.id && !o.dirty && o.object.mutation_id == query.mutation
            }) {
                query.object.revision = item.object.revision;
            }
            if let Some((id, error)) = &state.view.failed
                && id == &query.object.id
                && let Some(tab) = self.tabs.iter_mut().find(|t| &t.id == tab_id)
            {
                tab.error = Some(error.clone());
                if let Panel::Sql { workbench, .. } = &mut tab.panel
                    && let Some(document) = &mut workbench.document
                {
                    document.saving = false;
                }
            }
            if let Some((_, stamp)) = state
                .view
                .saved
                .iter()
                .rev()
                .find(|(id, _)| id == &query.object.id)
                && !query.mutation.is_empty()
                && state.view.accepted.contains(&query.mutation)
                && query.acknowledged != query.mutation
            {
                if let Some(Tab {
                    panel: Panel::Sql { sql, workbench, .. },
                    ..
                }) = self.tabs.iter_mut().find(|t| &t.id == tab_id)
                    && let Some(document) = &mut workbench.document
                {
                    document.saved(
                        document.scope.clone(),
                        query.object.name.clone(),
                        stamp.clone(),
                    );
                    document.changed(sql);
                    if document.close_after_save && !document.dirty() {
                        close.push(tab_id.clone());
                    }
                }
                query.acknowledged = query.mutation.clone();
            }
        }
        for id in close {
            self.close_tab(&id, false);
        }
    }
    pub(super) fn collaboration_connections_changed(&mut self) {
        if let Some(state) = &mut self.collaboration {
            for (local, object) in &state.view.bindings {
                if let Some(c) = self.store.connections.iter().find(|c| &c.id == local) {
                    let body = if object.kind == cloud::Kind::Group {
                        object.body.clone()
                    } else {
                        cloud::connection_body_in_store(c, &self.store)
                    };
                    if object.body != body || object.name != c.name {
                        let stamp = cloud::connection_stamp(&c.name, &body);
                        match state.client.send(cloud::Command::LocalConnection {
                            id: object.id.clone(),
                            name: c.name.clone(),
                            body,
                        }) {
                            Ok(()) => {
                                state.local_pending.insert(object.id.clone(), stamp);
                            }
                            Err(e) => state.error = Some(e.to_string()),
                        }
                    }
                }
            }
        }
    }
    pub(super) fn collaboration_personal_changed(&mut self) {
        if let Some(state) = &mut self.collaboration
            && let Some(object) = &state.view.personal
            && object.revision > 0
        {
            let local = cloud::personal_body(&self.settings, &state.view.bindings, &self.store);
            if object.body["appearance"] != local["appearance"]
                || object.body["connections"] != local["connections"]
            {
                let mut mutation = cloud::Mutation::from_object(object);
                mutation.body["appearance"] = local["appearance"].clone();
                mutation.body["connections"] = local["connections"].clone();
                state.send(cloud::Command::Save {
                    project: "@personal".into(),
                    id: "@personal".into(),
                    mutation,
                });
            }
        }
    }
    fn open_cloud_query(&mut self, state: &mut State) -> anyhow::Result<()> {
        let object = state
            .view
            .document
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("请先打开查询"))?;
        anyhow::ensure!(
            matches!(object.kind, cloud::Kind::Query | cloud::Kind::Snippet),
            "此对象不能在 SQL 编辑器打开"
        );
        if let Some((tab, _)) = state.queries.iter().find(|(_, q)| q.object.id == object.id) {
            self.active = Some(tab.clone());
            return Ok(());
        }
        let mut parent = object.parent_id.clone();
        let mut local = None;
        for _ in 0..64 {
            if parent.is_empty() {
                break;
            }
            if let Some((id, _)) = state.view.bindings.iter().find(|(_, o)| o.id == parent) {
                local = Some(id.clone());
                break;
            }
            parent = state
                .view
                .items
                .iter()
                .find(|o| o.object.id == parent)
                .map(|o| o.object.parent_id.clone())
                .unwrap_or_default();
        }
        let local = local
            .or_else(|| self.selected_connection.clone())
            .ok_or_else(|| anyhow::anyhow!("先将所属连接接入连接树，或选中一个本地数据库连接"))?;
        let connection = self
            .store
            .connections
            .iter()
            .find(|c| c.id == local)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("本地连接不存在"))?;
        let mut tab = tabs::create_tab(connection, self.runtime.as_ref().map(|r| r.handle()))?;
        let Panel::Sql {
            sql,
            workbench,
            connection,
            ..
        } = &mut tab.panel
        else {
            anyhow::bail!("请选择数据库连接")
        };
        *sql = object.body["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("对象没有 SQL 文本"))?
            .into();
        if let Some(database) = object.body["database"].as_str() {
            connection.set("database", database);
        }
        let scope = tomato_core::queries::Scope {
            connection: connection.name.clone(),
            database: connection.text("database").into(),
            path: String::new(),
        };
        let mut document = crate::tabs::queries::Document::new(scope, sql);
        let stamp = tomato_services::queries::stamp(sql.as_bytes());
        document.loaded(object.name.clone(), stamp.clone());
        workbench.document = Some(document);
        workbench.completion.focus_requested = true;
        tab.title = object.name.clone();
        tab.source_id = format!("cloud-query:{}", object.id);
        let id = tab.id.clone();
        self.active = Some(id.clone());
        state.queries.insert(
            id,
            Query {
                object: {
                    let mut metadata = (**object).clone();
                    metadata.body = metadata_body(&object.body);
                    metadata
                },
                acknowledged: String::new(),
                mutation: String::new(),
            },
        );
        self.tabs.push(tab);
        self.section = 0;
        state.open = false;
        state.reset_draft();
        state.send(cloud::Command::Unload);
        Ok(())
    }
    pub(super) fn save_cloud_query(&mut self, id: &str) -> anyhow::Result<bool> {
        let Some(state) = &mut self.collaboration else {
            return Ok(false);
        };
        let Some(query) = state.queries.get_mut(id) else {
            return Ok(false);
        };
        let Some(Tab {
            panel: Panel::Sql { sql, workbench, .. },
            ..
        }) = self.tabs.iter_mut().find(|t| t.id == id)
        else {
            return Ok(false);
        };
        let mut mutation = cloud::Mutation::from_object(&query.object);
        anyhow::ensure!(
            state
                .view
                .projects
                .iter()
                .any(|p| p.id == query.object.project_id && p.role.write()),
            "云端查询当前为只读或权限已撤销"
        );
        mutation.body["text"] = sql.clone().into();
        query.mutation = mutation.mutation_id.clone();
        state.client.send(cloud::Command::Save {
            project: query.object.project_id.clone(),
            id: query.object.id.clone(),
            mutation,
        })?;
        if let Some(document) = &mut workbench.document {
            document.saving = true;
        }
        Ok(true)
    }
}
fn ctx_clone(ctx: &egui::Context) -> egui::Context {
    ctx.clone()
}
fn metadata_body(value: &serde_json::Value) -> serde_json::Value {
    value
        .as_object()
        .map(|map| {
            serde_json::Value::Object(
                map.iter()
                    .filter(|(key, _)| key.as_str() != "text" && key.as_str() != "base64")
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            )
        })
        .unwrap_or_else(|| serde_json::json!({}))
}
fn local_id(state: &State, object: &str) -> String {
    let key = if state.account.is_empty() {
        format!(
            "{}:{}",
            state.base.trim_end_matches('/'),
            state.email.trim().to_lowercase()
        )
    } else {
        state.account.clone()
    };
    format!(
        "cloud-{}",
        tomato_services::queries::stamp(format!("{key}:{object}").as_bytes())
    )
}
