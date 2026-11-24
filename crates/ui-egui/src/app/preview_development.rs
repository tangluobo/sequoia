use super::*;
pub(super) fn enabled() -> bool {
    std::env::var_os("TOMATO_UI_CAPTURE").is_some()
        && matches!(
            std::env::var("TOMATO_UI_PREVIEW").as_deref(),
            Ok("development" | "color_transpose")
        )
}
impl TomatoApp {
    pub(super) fn development_preview(&mut self, ctx: &egui::Context) {
        if !enabled() {
            return;
        }
        let Some(profile) = std::env::var_os("TOMATO_DATA_DIR").map(PathBuf::from) else {
            return;
        };
        if !profile.file_name().is_some_and(|n| {
            n.to_string_lossy()
                .starts_with("tomato-development-fixture-")
        }) || !self.store.connections.is_empty()
        {
            return;
        }
        let game = std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("color_transpose");
        let mode = std::env::var("TOMATO_DEVELOPMENT_STATE").unwrap_or_else(|_| "editor".into());
        let settings = crate::development::State::settings_preview_size(&mode);
        if game
            && !self
                .tabs
                .iter()
                .any(|t| matches!(&t.panel,Panel::Tool(tool) if tool.kind=="color_transpose_game"))
        {
            self.open_tool("color_transpose_game", "颜色转置游戏");
        }
        if !game {
            self.ensure_development();
            let Some(state) = &mut self.development else {
                return;
            };
            state.poll(ctx);
            if let Err(e) = state.preview_prepare(&mode) {
                self.error = Some(e.to_string());
                return;
            }
        }
        let id = egui::ViewportId::from_hash_of("development-fixture");
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("tomato-capture-viewport"), id));
        ctx.show_viewport_immediate(id,egui::ViewportBuilder::default().with_title("原版功能对照验收").with_inner_size(settings.unwrap_or_else(||if game{egui::vec2(896.,650.)}else{egui::vec2(1100.,650.)})).with_resizable(false).with_active(false),|ui,_|{
            ui.spacing_mut().item_spacing=egui::Vec2::ZERO;
            ui.painter().rect_filled(ui.max_rect(),0,Color32::WHITE);
            if game {
                if let Some(tab)=self.tabs.iter_mut().find(|t|matches!(&t.panel,Panel::Tool(tool) if tool.kind=="color_transpose_game")) && let Panel::Tool(tool)=&mut tab.panel{
                    let state=tool.color_transpose.get_or_insert_with(||Box::new(crate::panels::color_transpose::State::fixture(&mode)));
                    crate::panels::color_transpose::show(ui,state);
                }
            } else if settings.is_some() {
                if let Some(state)=&mut self.development && let Err(error)=state.preview_settings(ui,&mode){self.error=Some(error.to_string());}
            } else if let Some(state)=&mut self.development{
                egui::Panel::left("development-fixture-sidebar").exact_size(250.).resizable(false).show_separator_line(false).frame(egui::Frame::NONE.fill(Color32::WHITE)).show(ui,|ui|{
                    let (header,_)=ui.allocate_exact_size(egui::vec2(250.,52.),egui::Sense::hover());
                    self.icons.paint(ui,"images/development/folder.svg",egui::Rect::from_min_size(header.min+egui::vec2(15.,16.),egui::vec2(20.,20.)));
                    crate::panels::ensure_bold_font(ctx);
                    ui.painter().text(header.min+egui::vec2(43.,16.),egui::Align2::LEFT_TOP,"开发目录",egui::FontId::new(14.,egui::FontFamily::Name("md-bold".into())),Color32::from_gray(51));
                    state.sidebar(ui,&mut self.icons);
                });
                egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::WHITE)).show(ui,|ui|state.show(ui,&mut self.icons));
            }
            if let Some(image)=ui.input(|i|i.events.iter().find_map(|event|if let egui::Event::Screenshot{viewport_id,image,..}=event&&*viewport_id==id{Some(image.clone())}else{None})){ctx.data_mut(|d|d.insert_temp(egui::Id::new("tomato-capture-image"),image));}
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}
