//! Opt-in owned-fixture acceptance; never reads saved user connections.
use super::*;
use serde_json::json;
pub(super) struct Fixture {
    case: String,
    phase: u8,
    root: PathBuf,
    tab: Option<String>,
    finished: bool,
}
impl TomatoApp {
    pub(in crate::app) fn preview_collaboration(
        &mut self,
        ctx: &egui::Context,
    ) -> anyhow::Result<()> {
        let root = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("需要隔离配置"))?;
        anyhow::ensure!(
            root.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("tomato-cloud-fixture-"))
                && self.store.connections.is_empty()
                && std::env::var_os("TOMATO_UI_CAPTURE").is_some(),
            "云端夹具必须使用独立空配置"
        );
        let base = std::env::var("TOMATO_CLOUD_TEST_URL")?;
        anyhow::ensure!(
            base.starts_with("http://127.0.0.1:"),
            "夹具只能连接自己的回环服务器"
        );
        self.open_collaboration();
        let mut state = self.collaboration.take().unwrap();
        state.base = base.clone();
        state.email = "owner@example.test".into();
        state.name = "验收账号".into();
        let case = std::env::var("TOMATO_CLOUD_TEST_CASE").unwrap_or_else(|_| "objects".into());
        if case != "login" {
            self.cloud_action(
                &mut state,
                Action::Send(cloud::Command::Connect {
                    base,
                    email: "owner@example.test".into(),
                    password: "fixture-password-123".into(),
                    code: String::new(),
                }),
            );
        }
        state.fixture = Some(Fixture {
            case,
            phase: 0,
            root,
            tab: None,
            finished: false,
        });
        self.collaboration = Some(state);
        self.quit_approved = true;
        ctx.request_repaint();
        Ok(())
    }
    pub(super) fn cloud_fixture(&mut self, state: &mut State, ctx: &egui::Context) {
        let Some(mut f) = state.fixture.take() else {
            return;
        };
        if f.finished {
            state.fixture = Some(f);
            return;
        }
        let result = (|| -> anyhow::Result<bool> {
            if f.case == "login" {
                return Ok(true);
            }
            if state.view.user.is_none() {
                return Ok(false);
            }
            if f.phase == 0 {
                if let Some(p) = state.view.projects.iter().find(|p| p.name == "协作验收") {
                    state.send(cloud::Command::Select(p.id.clone()));
                    f.phase = 1;
                }
                return Ok(false);
            }
            if f.phase == 1 {
                if state.view.items.len() < 4 {
                    return Ok(false);
                }
                if f.case == "objects" {
                    return Ok(true);
                }
                if f.case == "members" {
                    state.detail = Some("成员");
                    state.send(cloud::Command::Detail(cloud::Detail::Members(
                        state.view.selected.clone(),
                    )));
                    f.phase = 8;
                    return Ok(false);
                }
                let object = state
                    .view
                    .items
                    .iter()
                    .find(|o| o.object.kind == cloud::Kind::Connection)
                    .ok_or_else(|| anyhow::anyhow!("缺少连接"))?;
                state.send(cloud::Command::Load(object.object.id.clone()));
                f.phase = 2;
                return Ok(false);
            }
            if f.phase == 2 {
                let Some(object) = state
                    .view
                    .document
                    .as_ref()
                    .filter(|o| o.kind == cloud::Kind::Connection)
                else {
                    return Ok(false);
                };
                let id = local_id(state, &object.id);
                let mut local = Connection::new("SQLITE");
                local.id = id;
                local.name = "旧连接".into();
                local.set("password", "fixture-secret-kept-local");
                local.set("futureJava", json!({"nested":[1,2,3]}));
                let mut next = self.store.clone();
                next.connections.push(local);
                anyhow::ensure!(self.persist_store(next), "隔离旧配置保存失败");
                self.cloud_action(state, Action::Bind);
                f.phase = 3;
                return Ok(false);
            }
            if f.phase == 3 {
                let Some((local, _)) = state
                    .view
                    .bindings
                    .iter()
                    .find(|(_, o)| o.kind == cloud::Kind::Connection)
                else {
                    return Ok(false);
                };
                let c = self
                    .store
                    .connections
                    .iter()
                    .find(|c| &c.id == local)
                    .unwrap();
                anyhow::ensure!(
                    c.text("password") == "fixture-secret-kept-local"
                        && c.options.contains_key("futureJava")
                        && c.parent_id.is_some(),
                    "连接、凭据或分组没有兼容保留"
                );
                if f.case == "connection" {
                    return Ok(true);
                }
                let q = state
                    .view
                    .items
                    .iter()
                    .find(|o| o.object.kind == cloud::Kind::Query)
                    .ok_or_else(|| anyhow::anyhow!("缺少查询"))?;
                state.send(cloud::Command::Load(q.object.id.clone()));
                f.phase = 4;
                return Ok(false);
            }
            if f.phase == 4 {
                if state
                    .view
                    .document
                    .as_ref()
                    .is_none_or(|o| o.kind != cloud::Kind::Query)
                {
                    return Ok(false);
                }
                self.open_cloud_query(state)?;
                let id = self.active.clone().unwrap();
                let tab = self.tabs.iter_mut().find(|t| t.id == id).unwrap();
                let Panel::Sql {
                    sql,
                    workbench,
                    connection,
                    ..
                } = &mut tab.panel
                else {
                    unreachable!()
                };
                *sql = "SELECT 'cloud-native-saved🙂' AS value".into();
                workbench.document.as_mut().unwrap().changed(sql);
                let mut mutation = cloud::Mutation::from_object(&state.queries[&id].object);
                mutation.body["text"] = sql.clone().into();
                state.queries.get_mut(&id).unwrap().mutation = mutation.mutation_id.clone();
                state.send(cloud::Command::Save {
                    project: state.queries[&id].object.project_id.clone(),
                    id: state.queries[&id].object.id.clone(),
                    mutation,
                });
                workbench.document.as_mut().unwrap().saving = true;
                let command = Command::Sql {
                    connection: connection.clone(),
                    sql: sql.clone(),
                };
                self.submit(&id, command, Purpose::Normal);
                f.tab = Some(id);
                f.phase = 5;
                return Ok(false);
            }
            if f.phase == 7 {
                return Ok(self.tabs.iter().all(|t| Some(&t.id) != f.tab.as_ref()));
            }
            if matches!(f.phase, 5 | 6) {
                let tab = self
                    .tabs
                    .iter()
                    .find(|t| Some(&t.id) == f.tab.as_ref())
                    .unwrap();
                let Panel::Sql { workbench, .. } = &tab.panel else {
                    unreachable!()
                };
                let doc = workbench.document.as_ref().unwrap();
                if doc.saving || doc.dirty() || tab.busy {
                    return Ok(false);
                }
                anyhow::ensure!(tab.error.is_none(), "查询执行失败：{:?}", tab.error);
                if f.case != "sql_repeat_close" {
                    return Ok(true);
                }
                let id = f.tab.as_ref().unwrap().clone();
                let query = &state.queries[&id];
                let mut mutation = cloud::Mutation::from_object(&query.object);
                mutation.body["text"] = "SELECT 'cloud-native-saved🙂' AS value".into();
                state.queries.get_mut(&id).unwrap().mutation = mutation.mutation_id.clone();
                state.send(cloud::Command::Save {
                    project: state.queries[&id].object.project_id.clone(),
                    id: state.queries[&id].object.id.clone(),
                    mutation,
                });
                if let Panel::Sql { workbench, .. } =
                    &mut self.tabs.iter_mut().find(|t| t.id == id).unwrap().panel
                {
                    let document = workbench.document.as_mut().unwrap();
                    document.saving = true;
                    document.close_after_save = f.phase == 6;
                }
                f.phase += 1;
                return Ok(false);
            }
            if f.phase == 8 {
                return Ok(state.view.detail.as_array().is_some_and(|a| !a.is_empty()));
            }
            Ok(false)
        })();
        match result {
            Ok(true) => {
                f.finished = true;
                let proof = json!({"case":f.case,"status":"passed","projects":state.view.projects.len(),"objects":state.view.items.len(),"bindings":state.view.bindings.len(),"queries":state.queries.len(),"online":state.view.online,"error":state.error,"message":state.view.message,"closed_saved_query":f.case=="sql_repeat_close"&&self.tabs.iter().all(|t|Some(&t.id)!=f.tab.as_ref())});
                let path = std::env::var_os("TOMATO_UI_CAPTURE")
                    .map(PathBuf::from)
                    .unwrap()
                    .with_extension("state.json");
                let _ = std::fs::write(path, serde_json::to_vec_pretty(&proof).unwrap());
            }
            Ok(false) => ctx.request_repaint_after(std::time::Duration::from_millis(20)),
            Err(error) => {
                state.error = Some(error.to_string());
                f.finished = true;
                let _ = std::fs::write(f.root.join("failed.txt"), error.to_string());
            }
        }
        state.fixture = Some(f);
    }
}
