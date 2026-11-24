//! Owns modal drafts and asynchronous generation, independently of database execution.
use super::*;
use tomato_core::ai_sql::{
    editor::{self, Insertion},
    settings::Settings,
};
pub(super) enum Dialog {
    Settings {
        draft: Box<Settings>,
        timeout: String,
        resume: Option<(String, bool)>,
        editors: Box<[tabs::text_state::Editor; 4]>,
        focus: bool,
    },
    Prompt {
        tab: String,
        text: String,
        plan: Box<Plan>,
        editor: tabs::text_state::Editor,
        focus: bool,
    },
}
pub(super) struct Plan {
    insertion: Insertion,
    selected: String,
}
impl Dialog {
    fn release(&self, ctx: &egui::Context) {
        match self {
            Self::Settings { editors, .. } => {
                for editor in editors.iter() {
                    editor.release(ctx);
                }
            }
            Self::Prompt { editor, .. } => editor.release(ctx),
        }
    }
}
impl TomatoApp {
    pub(super) fn open_ai_settings(&mut self, resume: Option<(String, bool)>) {
        if let Some(dialog) = self.ai_dialog.take() {
            dialog.release(&self.context);
        }
        let draft = Settings::read(&self.settings);
        let timeout = draft.timeout.to_string();
        self.ai_dialog = Some(Dialog::Settings {
            draft: Box::new(draft),
            timeout,
            resume,
            editors: Box::default(),
            focus: true,
        });
    }
    pub(super) fn request_ai_sql(&mut self, id: &str, selected_prompt: bool) {
        let Some(Panel::Sql {
            connection,
            workbench,
            sql,
            ..
        }) = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| &tab.panel)
        else {
            return;
        };
        if workbench.ai.pending() {
            return;
        }
        if workbench.schema.trim().is_empty() {
            self.error = Some("请先选择连接和数据库".into());
            return;
        }
        if !Settings::read(&self.settings).configured() {
            self.open_ai_settings(Some((id.into(), selected_prompt)));
            return;
        }
        if sql.len() > tomato_core::ai_sql::MAX_TEXT {
            self.error = Some("SQL 文档超过 8 MiB".into());
            return;
        }
        let range = workbench.completion.selection.as_sorted_char_range();
        let offset = |value| {
            sql.char_indices()
                .nth(value)
                .map_or(sql.len(), |(index, _)| index)
        };
        let selected = &sql[offset(range.start.into())..offset(range.end.into())];
        let caret = workbench.completion.selection.primary.index.into();
        let directive = editor::directive(sql, caret);
        if selected_prompt && !tomato_core::ai_sql::is_blank(selected) {
            let prompt = selected.trim_matches(|ch: char| ch <= ' ').into();
            let plan = Plan {
                insertion: Insertion::after(sql, range.end.into()),
                selected: String::new(),
            };
            self.start_ai_sql(id, prompt, plan);
        } else if let Some(directive) = directive {
            let prompt = directive.prompt.into();
            let plan = Plan {
                insertion: Insertion::directive(sql, &directive),
                selected: String::new(),
            };
            self.start_ai_sql(id, prompt, plan);
        } else {
            let _ = connection;
            self.ai_dialog = Some(Dialog::Prompt {
                tab: id.into(),
                text: String::new(),
                plan: Box::new(Plan {
                    insertion: Insertion::after(sql, range.end.into()),
                    selected: selected.into(),
                }),
                editor: Default::default(),
                focus: true,
            });
        }
    }
    fn start_ai_sql(&mut self, id: &str, prompt: String, plan: Plan) {
        let settings = Settings::read(&self.settings);
        let held = self
            .tabs
            .iter()
            .filter_map(|tab| {
                if let Panel::Sql { workbench, .. } = &tab.panel {
                    workbench.ai.insertion.as_ref()
                } else {
                    None
                }
            })
            .fold(0usize, |sum, insertion| {
                sum.saturating_add(insertion.snapshot.capacity())
            });
        if held.saturating_add(plan.insertion.snapshot.capacity()) > 32 * 1024 * 1024 {
            self.error = Some("AI SQL 编辑快照已达 32 MiB，请等待生成完成后重试".into());
            return;
        }
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut tab.panel
        else {
            return;
        };
        if workbench.ai.pending() {
            return;
        }
        let request = tomato_services::ai_sql::Request {
            structure: if settings.include_schema {
                editor::schema_context(&workbench.completion.catalog().ai_tables, &prompt)
            } else {
                String::new()
            },
            settings,
            dialect: tomato_core::catalog::DATABASE_PROFILES
                .iter()
                .find(|profile| profile.id == connection.kind().to_ascii_lowercase())
                .map_or(connection.kind(), |profile| profile.label)
                .into(),
            database: tabs::workbench::query_database(connection, &workbench.schema).into(),
            schema: if connection.kind() == "POSTGRESQL" {
                workbench.schema.clone()
            } else {
                String::new()
            },
            prompt,
            selected: plan.selected,
        };
        match self.queue.submit(Command::AiSql {
            request: Box::new(request),
        }) {
            Ok(task) => {
                workbench.ai.generation = workbench.ai.generation.wrapping_add(1);
                workbench.ai.insertion = Some(plan.insertion);
                tab.error = None;
                self.pending.insert(
                    task,
                    Pending {
                        tab_id: id.into(),
                        purpose: Purpose::AiSql(workbench.ai.generation),
                    },
                );
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    pub(super) fn ai_sql_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.ai_dialog.take() else {
            return;
        };
        let settings = matches!(dialog, Dialog::Settings { .. });
        let height = if settings { 292. } else { 212. };
        let mut accept = false;
        let mut cancel = false;
        let viewport = egui::ViewportId::from_hash_of("ai-sql-dialog");
        let capture = ctx
            .data(|data| data.get_temp::<bool>(egui::Id::new("tomato-ai-dialog-fixture")))
            .unwrap_or(false);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(if settings {
                "AI SQL 设置"
            } else {
                "AI 生成 SQL"
            })
            .with_inner_size(egui::vec2(560., height))
            .with_resizable(false);
        if capture {
            builder = builder.with_active(false);
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("tomato-capture-viewport"), viewport)
            });
        }
        ctx.show_viewport_immediate(viewport,builder,|ui,_|{
            let origin=ui.max_rect().min;let at=|x,y,w,h|egui::Rect::from_min_size(origin+egui::vec2(x,y),egui::vec2(w,h));
            ui.painter().rect_filled(ui.max_rect(),0.,Color32::from_gray(244));
            crate::native_dialog::gradient(ui,at(0.,0.,560.,39.),Color32::from_gray(244),Color32::from_gray(249));
            ui.painter().line_segment([origin+egui::vec2(0.,38.5),origin+egui::vec2(560.,38.5)],egui::Stroke::new(1.,Color32::from_gray(200)));
            ui.painter().text(origin+egui::vec2(10.,19.5),egui::Align2::LEFT_CENTER,if settings{"配置 OpenAI 兼容的模型接口"}else{"描述你想查询或修改的数据"},egui::FontId::proportional(14.),Color32::BLACK);
            ui.style_mut().override_font_id=Some(egui::FontId::proportional(12.));
            ui.visuals_mut().selection.bg_fill=Color32::from_rgb(0,150,201);ui.visuals_mut().selection.stroke=egui::Stroke::new(1.,Color32::WHITE);
            let mut fields=Vec::new();
            let valid=match &mut dialog {
                Dialog::Settings{draft,timeout,editors,..}=>{
                    for (index,(label,y,value,password,hint)) in [("接口地址：",47.,&mut draft.base,false,"例如：https://api.openai.com/v1 或 http://localhost:11434/v1"),("模型：",82.,&mut draft.model,false,"模型名称"),("API Key：",117.,&mut draft.key,true,"本地无鉴权服务可以留空"),("超时：",152.,timeout,false,"")].into_iter().enumerate(){
                        ui.painter().text(origin+egui::vec2(8.,y+11.5),egui::Align2::LEFT_CENTER,label,egui::FontId::proportional(12.),Color32::BLACK);
                        let id=editors[index].prepare_id(ctx,egui::Id::new(("ai-settings",index)));let width=if index==3{71.}else{474.};
                        let output=ui.scope_builder(egui::UiBuilder::new().max_rect(at(78.,y,width,23.)),|ui|TextEdit::singleline(value).id(id).password(password).hint_text(hint).char_limit(16384).desired_width(width).min_size(egui::vec2(width,23.)).margin(egui::vec2(6.,3.)).show(ui)).inner;

                        if ctx.memory(|memory|memory.focused())==Some(id){ui.painter().rect_stroke(output.response.rect.expand(0.5),2.,egui::Stroke::new(1.,Color32::from_rgb(3,158,211)),egui::StrokeKind::Inside);}
                        if capture{fields.push([output.response.rect.left()-origin.x,output.response.rect.top()-origin.y,output.response.rect.width(),output.response.rect.height()]);}
                        editors[index].trim(ctx);
                    }
                    for (part,y,step) in [(0,152.,5_i64),(1,163.5,-5)]{
                        let rect=at(149.,y,24.,11.5);let id=egui::Id::new(("ai-timeout-spinner",part));let response=ui.interact(rect,id,egui::Sense::CLICK);
                        crate::native_dialog::gradient(ui,rect,Color32::from_gray(if part==0{253}else{249}),Color32::from_gray(if part==0{219}else{226}));
                        ui.painter().rect_stroke(rect,0.,egui::Stroke::new(1.,Color32::from_gray(181)),egui::StrokeKind::Inside);
                        if crate::native_dialog::spinner::repeat(ui,&response,id){let current=timeout.trim().parse::<i64>().unwrap_or(draft.timeout as i64);draft.timeout=current.saturating_add(step).clamp(5,300)as u64;*timeout=draft.timeout.to_string();}
                        let center=rect.center();let sign=if part==0{-1.}else{1.};ui.painter().add(egui::Shape::convex_polygon(vec![center+egui::vec2(-3.5,-sign*1.5),center+egui::vec2(3.5,-sign*1.5),center+egui::vec2(0.,sign*2.5)],Color32::from_gray(85),egui::Stroke::NONE));
                    }
                    if ctx.memory(|memory|memory.focused())==Some(egui::Id::new(("ai-settings",3))){let delta=ui.input(|input|i64::from(input.key_pressed(egui::Key::ArrowUp))-i64::from(input.key_pressed(egui::Key::ArrowDown)));if delta!=0{let current=timeout.trim().parse::<i64>().unwrap_or(draft.timeout as i64);draft.timeout=current.saturating_add(delta*5).clamp(5,300)as u64;*timeout=draft.timeout.to_string();}}
                    ui.painter().text(origin+egui::vec2(183.,163.5),egui::Align2::LEFT_CENTER,"秒",egui::FontId::proportional(12.),Color32::BLACK);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(at(78.,187.,474.,17.)),|ui|{ui.spacing_mut().interact_size.y=17.;ui.spacing_mut().icon_width=14.;ui.spacing_mut().icon_spacing=4.;ui.checkbox(&mut draft.include_schema,"生成时发送当前库的表结构（不发送数据行）");});
                    ui.scope_builder(egui::UiBuilder::new().max_rect(at(78.,215.,474.,30.)).layout(egui::Layout::top_down(egui::Align::Min)),|ui|{ui.add(egui::Label::new(RichText::new("API Key 加密保存在本机；数据库密码和查询结果数据行不会发送。用户主动选中的 SQL 会作为生成上下文发送。").size(11.).line_height(Some(15.)).color(Color32::from_gray(119))).halign(egui::Align::Min).wrap());});draft.configured()
                }
                Dialog::Prompt{text,editor,focus,..}=>{
                    let id=editor.prepare_id(ctx,egui::Id::new("ai-sql-prompt"));
                    let output=ui.scope_builder(egui::UiBuilder::new().max_rect(at(4.,43.,552.,82.)),|ui|TextEdit::multiline(text).id(id).desired_width(552.).min_size(egui::vec2(552.,82.)).desired_rows(4).char_limit(65536).hint_text("例如：查询学校标识为 10 的学生信息，按创建时间倒序").show(ui)).inner;
                    if *focus{output.response.request_focus();*focus=false;}
                    if ctx.memory(|memory|memory.focused())==Some(id){ui.painter().rect_stroke(output.response.rect.expand(0.5),2.,egui::Stroke::new(1.,Color32::from_rgb(3,158,211)),egui::StrokeKind::Inside);}
                    if capture{fields.push([output.response.rect.left()-origin.x,output.response.rect.top()-origin.y,output.response.rect.width(),output.response.rect.height()]);}
                    ui.scope_builder(egui::UiBuilder::new().max_rect(at(4.,135.,552.,30.)).layout(egui::Layout::top_down(egui::Align::Min)),|ui|{ui.add(egui::Label::new(RichText::new("也可以在编辑器中输入“-- ai: 需求”，将光标放在该行后按 Ctrl+I。\n生成结果只写入编辑器，不会自动执行。").size(11.).line_height(Some(15.)).color(Color32::from_gray(119))).halign(egui::Align::Min));});editor.trim(ctx);!tomato_core::ai_sql::is_blank(text)
                }
            };
            let ok=ui.add_enabled_ui(valid,|ui|crate::native_dialog::button(ui,at(390.,height-33.,75.,24.),if settings{"保存"}else{"生成"},true)).inner;accept=ok.clicked();
            let cancel_button=crate::native_dialog::button(ui,at(475.,height-33.,75.,24.),"取消",false);
            if let Dialog::Settings{focus,..}=&mut dialog&&*focus{if valid{ok.request_focus();}else{cancel_button.request_focus();}*focus=false;}
            cancel=cancel_button.clicked()||ui.input(|input|input.viewport().close_requested()||input.key_pressed(egui::Key::Escape));
            if settings&&valid&&ui.input(|input|input.key_pressed(egui::Key::Enter)){accept=true;}
            if capture {
                if let Some(image)=ui.input(|input|input.events.iter().find_map(|event|if let egui::Event::Screenshot{viewport_id,image,..}=event&&*viewport_id==viewport{Some(image.clone())}else{None})){ctx.data_mut(|data|data.insert_temp(egui::Id::new("tomato-capture-image"),image));}
                if let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from){let _=std::fs::write(path.with_extension("dialog.json"),serde_json::json!({"ready":true,"clientWidth":ui.max_rect().width(),"clientHeight":ui.max_rect().height(),"okX":ok.rect.left()-origin.x,"okY":ok.rect.top()-origin.y,"okWidth":ok.rect.width(),"okHeight":ok.rect.height(),"fields":fields,"httpRequested":false,"settingsSaved":false}).to_string());}
            }
        });
        if cancel {
            dialog.release(ctx);
            return;
        }
        if !accept {
            self.ai_dialog = Some(dialog);
            return;
        }
        dialog.release(ctx);
        match dialog {
            Dialog::Settings {
                mut draft,
                timeout,
                resume,
                ..
            } => {
                draft.timeout = timeout
                    .trim()
                    .parse::<i64>()
                    .map(|value| value.clamp(5, 300) as u64)
                    .unwrap_or(draft.timeout);
                let mut settings = self.settings.clone();
                draft.store(&mut settings);
                match self.preferences.save(&settings) {
                    Ok(()) => {
                        self.settings = settings;
                        if let Some((tab, selected)) = resume {
                            self.request_ai_sql(&tab, selected);
                        }
                    }
                    Err(error) => {
                        self.error = Some(format!("保存全局设置失败：{error}"));
                        self.ai_dialog = Some(Dialog::Settings {
                            draft,
                            timeout,
                            resume,
                            editors: Box::default(),
                            focus: false,
                        });
                    }
                }
            }
            Dialog::Prompt {
                tab, text, plan, ..
            } => self.start_ai_sql(&tab, text.trim_matches(|ch: char| ch <= ' ').into(), *plan),
        }
    }
}
