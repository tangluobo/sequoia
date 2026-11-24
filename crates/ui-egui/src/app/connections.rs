use super::db_tree::{self, Node};
use super::*;

enum TreeAction {
    Root(&'static str),
    Rename,
    BeginRename(Box<super::tree_rename::Draft>),
    Remote(Connection, super::remote_tree::Entry, &'static str),
    Backup(super::backup_tree::Target, &'static str),
    Query(super::query_tree::Action),
    MongoPrompt(
        Connection,
        tomato_services::mongo::Target,
        super::mongo_tree::Kind,
    ),
    MongoRefresh(Connection, Option<String>),
    Connection(&'static str, String),
    Schema(Connection, String, bool),
    Database(Connection, String, bool),
    CloseDatabase(Connection, String),
    Table(Connection, tomato_core::workbench::TableInfo, bool),
    NewTable(Connection, String),
    DatabaseAdmin(Connection, String, super::database_admin::Mode),
    NewQuery(Connection, String),
    CopyTable(Connection, tomato_core::workbench::TableId),
    MutateObjects(Connection, tomato_core::database_objects::Request),
    PasteTables(Connection, String),
    Objects(Connection, String),
    Ssh(Connection, tomato_services::terminal::ssh::tools::Section),
    Container(Connection, String, &'static str),
    LoadContainers(Connection),
    LocalFolder(Connection, PathBuf, bool),
    LocalFile(Connection, PathBuf),
    LocalEdit(Connection, PathBuf, super::local_tree::Kind),
    SubmitRename,
    CancelRename,
    SubmitConnectionName,
    CancelConnectionName,
}
fn local_menu(ui: &mut Ui, icons: &mut crate::icons::Icons, label: &str, icon: &str) -> bool {
    let icon = icons.get(ui.ctx(), &format!("images/connect/{icon}"));
    ui.add(
        crate::compact_menu::Item::new(label)
            .icon_column()
            .icon(icon),
    )
    .clicked()
}
impl TomatoApp {
    pub(super) fn connection_list(&mut self, ui: &mut Ui) {
        let mut connected: std::collections::HashSet<_> = self
            .tabs
            .iter_mut()
            .filter_map(|tab| match &mut tab.panel {
                Panel::Sql {
                    connection,
                    workbench,
                    ..
                } if !workbench.schemas.is_empty()
                    && !matches!(
                        connection.kind(),
                        "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE"
                    ) =>
                {
                    Some(connection.id.clone())
                }
                Panel::Terminal {
                    connection,
                    session,
                    ..
                } => (session.status() == tomato_services::terminal::Status::Ready)
                    .then(|| connection.id.clone()),
                Panel::NativeDesktop {
                    connection,
                    session: Some(session),
                    ..
                } if session.status() == tomato_services::remote_desktop::Status::Ready => {
                    Some(connection.id.clone())
                }
                _ => None,
            })
            .collect();
        connected.extend(
            self.remote_hosts
                .iter()
                .filter(|(_, h)| h.ready)
                .map(|(id, _)| id.clone()),
        );
        connected.extend(
            self.mongo_hosts
                .iter()
                .filter(|(_, h)| h.ready)
                .map(|(id, _)| id.clone()),
        );
        connected.extend(
            self.redis_hosts
                .iter()
                .filter(|(_, h)| h.ready)
                .map(|(id, _)| id.clone()),
        );
        connected.extend(
            self.sql_hosts
                .iter()
                .filter(|(_, host)| {
                    !host.schemas.is_empty()
                        || host
                            .direct
                            .as_ref()
                            .is_some_and(|tree| tree.loaded || tree.loading)
                        || host.pg.as_ref().is_some_and(|pg| pg.loaded || pg.loading)
                })
                .map(|(id, _)| id.clone()),
        );
        let rows = db_tree::rows(
            &self.store,
            &self.tabs,
            &self.collapsed,
            db_tree::Hosts {
                remote: &self.remote_hosts,
                backups: &self.backups,
                sql: &self.sql_hosts,
                queries: &self.queries,
                ssh: &self.ssh_hosts,
                local: &self.local_trees,
                mongo: &self.mongo_hosts,
                redis: &self.redis_hosts,
            },
            &self.filter.trim_matches(|c: char| c <= ' ').to_lowercase(),
        );
        let mut action = None;
        let rename_allowed = self.local_edit.is_none()
            && self.connection_rename.is_none()
            && self.dialog.is_none()
            && self.credential.is_none()
            && self.backup_edit.is_none()
            && self.queries.dialog.is_none()
            && self.tree_objects.is_none()
            && self.confirmation.is_none()
            && !self.pending.values().any(|p| match &p.purpose {
                Purpose::LocalMutation { .. } | Purpose::TreeObjects(_) => true,
                Purpose::NamedQuery(job) => {
                    matches!(job.kind, super::query_tree::JobKind::Change { .. })
                }
                Purpose::BackupTree(job) => job.changes_names(),
                _ => false,
            });
        super::tree_rename::prepare(
            ui,
            &mut self.tree_rename,
            &rows,
            &self.filter,
            self.selected_connection.as_deref(),
            rename_allowed,
        );
        let was_editing = self.tree_rename.as_ref().is_some_and(|d| d.due.is_none());
        let mut rename_commit = None;
        let mut backup_loads = Vec::new();
        let mut drop_action = None;
        let can_drag = self.filter.trim().is_empty()
            && !was_editing
            && self.local_edit.is_none()
            && self.connection_rename.is_none()
            && !self
                .pending
                .values()
                .any(|pending| matches!(pending.purpose, Purpose::LocalMutation { .. }));
        let tree_id = ui.make_persistent_id("connection-tree-focus");
        let background = ui.interact(
            ui.available_rect_before_wrap(),
            tree_id,
            egui::Sense::click(),
        );
        let mut tree_has_focus = background.has_focus()
            || ui.memory(|m| m.focused()).is_some_and(|id| {
                ui.ctx()
                    .data(|d| d.get_temp::<egui::Id>(tree_id))
                    .is_some_and(|previous| previous == id)
            });
        let keyboard_active =
            tree_has_focus && rename_allowed && !was_editing && !egui::Popup::is_any_open(ui.ctx());
        let mut reveal = None;
        let collapsing_pending = ui.input(|i| i.key_pressed(egui::Key::ArrowLeft))
            && rows.iter().any(|row| {
                Some(&row.key()) == self.selected_connection.as_ref()
                    && !self.collapsed.contains(&row.key())
                    && matches!(&row.node, Node::LocalFile(c, entry) if entry.directory
                    && self.local_trees.get(&c.id).is_some_and(|host|host.loading(&entry.path)))
            });
        if keyboard_active
            && !collapsing_pending
            && let Some(index) = super::tree_keyboard::next(
                ui,
                &rows,
                self.selected_connection.as_deref(),
                (ui.available_height() / crate::theme::TREE_ROW_HEIGHT)
                    .floor()
                    .max(1.) as usize,
            )
        {
            super::tree_keyboard::select(
                &mut super::tree_keyboard::Selection {
                    selected: &mut self.selected_connection,
                    anchor: &mut self.tree_keyboard_anchor,
                    connections: &mut self.connection_selection,
                    files: &mut self.local_selection,
                    sql: &mut self.sql_selection,
                },
                &rows,
                index,
                ui.input(|i| i.modifiers),
            );
            reveal = Some(index);
            background.request_focus();
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut area = ScrollArea::vertical()
            .id_salt("connection-tree")
            .auto_shrink([false, false]);
        if let Some(index) = reveal {
            area = area.vertical_scroll_offset(index as f32 * crate::theme::TREE_ROW_HEIGHT);
        }
        let scroll = area.show_rows(
                ui,
                crate::theme::TREE_ROW_HEIGHT,
                rows.len(),
                |ui, visible| {
                    let mut rename_rect=None;
                    for index in visible {
                        let row = &rows[index];
                        if let Node::Backup(c,database,path,None,view)=&row.node {
                            let key=super::backup_tree::Key::new(&c.id,database,path);
                            if !self.backups.directories.contains_key(&key) {
                                backup_loads.push((c.id.clone(),(*view).map(str::to_owned),tomato_core::backup::Scope{connection:c.name.clone(),database:(*database).into(),path:(*path).into()}));
                            }
                        }
                        let key = row.key();
                        let inline_editing=super::tree_rename::editing(&self.tree_rename,&key);
                        let was_single=super::tree_rename::single(row,self.selected_connection.as_deref(),&self.connection_selection,&self.local_selection,&self.sql_selection);
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), crate::theme::TREE_ROW_HEIGHT),
                            if inline_editing {egui::Sense::hover()} else if can_drag { egui::Sense::click_and_drag() } else { egui::Sense::click() },
                        );
                        tree_has_focus |= response.has_focus() || response.clicked() || response.secondary_clicked();
                        if response.has_focus() || response.clicked() || response.secondary_clicked() { ui.ctx().data_mut(|d| d.insert_temp(tree_id, response.id)); }
                        if response.clicked() { self.tree_keyboard_anchor = Some(key.clone()); }
                        if reveal == Some(index) { response.scroll_to_me(None); }
                        let selected_local=matches!(&row.node,Node::LocalFile(c,entry) if self.local_selection.contains(&(c.id.clone(),entry.path.clone())));
                        let selected_node=match &row.node {Node::Connection(c)=>self.connection_selection.contains(&c.id),Node::Table(c,_) if matches!(c.kind(),"MYSQL"|"POSTGRESQL"|"ORACLE"|"HIVE")=>self.sql_selection.contains(&key),_=>self.selected_connection.as_deref()==Some(&key)};
                        if selected_node || selected_local || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                0,
                                if selected_node || selected_local {
                                    crate::theme::TREE_SELECTED
                                } else {
                                    crate::theme::TREE_HOVER
                                },
                            );
                        }
                        if let Some(drop) = super::tree_drag::row(ui, &response, &row.node, &self.store.connections, can_drag) { drop_action = Some(drop); }
                        let x = rect.left() + 2.0 + row.depth as f32 * 18.0;
                        let expandable = match &row.node {
                            Node::Remote(_,e)=>e.folder(),
                            Node::Backup(c,database,path,item,_)=>{
                                if item.is_some_and(|item|!item.directory){false}else{
                                    let key=super::backup_tree::Key::new(&c.id,database,path);
                                    let key=if let Some(item)=item{key.child(&item.name)}else{key};
                                    self.backups.directories.get(&key).is_none_or(|directory|directory.loading||!directory.items.is_empty())
                                }
                            }
                            Node::Query(c,namespace,path,entry)=>self.queries.has_children(&c.id,namespace,path,*entry),
                            Node::Connection(c) => {
                                self.local_trees.get(&c.id).is_some_and(|host|host.directories.get(&host.root).is_some_and(|entries|!entries.is_empty())) || self.mongo_hosts.get(&c.id).is_some_and(|h|!h.databases.is_empty()) || self.redis_hosts.get(&c.id).is_some_and(|h|!h.databases.is_empty()) || self.ssh_hosts.contains_key(&c.id) || self.remote_hosts.get(&c.id).is_some_and(|h|h.ready) || self.store.connections.iter().any(|child| child.parent_id.as_deref() == Some(&c.id))
                                    || self.sql_hosts.get(&c.id).is_some_and(|host|!host.schemas.is_empty()||host.pg.as_ref().is_some_and(|pg|!pg.names.is_empty()||pg.loading))
                            }
                            // Java creates closed database/schema nodes without children.
                            // Only double-click or the explicit Open action loads them.
                            Node::Database(c,d,_)=>self.sql_hosts.get(&c.id).and_then(|h|h.pg.as_ref()).and_then(|pg|pg.databases.get(*d)).and_then(|db|db.namespaces.as_ref()).is_some_and(|names|!names.is_empty()),
                            Node::Schema(c,s,opened) => if c.kind()=="REDIS" {false}else if c.kind()=="MONGODB" {self.mongo_hosts.get(&c.id).and_then(|h|h.collections.get(*s)).is_some_and(|rows|!rows.is_empty())}else{*opened},
                            Node::Objects(c,s,folder)=>*folder!=db_tree::Folder::Functions&&self.sql_hosts.get(&c.id).and_then(|h|h.tables_for(c,s)).is_some_and(|tables|tables.iter().any(|table|table.view==(*folder==db_tree::Folder::Views))),
                            Node::LocalFile(c, entry) => entry.directory && self.local_trees.get(&c.id).is_some_and(|host|host.directories.get(&entry.path).is_none_or(|entries|!entries.is_empty())),
                            Node::Ssh(c,tomato_services::terminal::ssh::tools::Section::Docker)=>self.ssh_hosts.get(&c.id).is_some_and(|h|!h.containers.is_empty()),
                            _ => false,
                        };
                        let expanded = match &row.node {
                            Node::Backup(c,database,path,item,_)=>{
                                let parent=super::backup_tree::Key::new(&c.id,database,path);
                                let actual=if let Some(item)=item{parent.child(&item.name)}else{parent};
                                self.backups.directories.contains_key(&actual)&&!self.collapsed.contains(&key)
                            }
                            Node::Query(..)=>!self.collapsed.contains(&key),
                            Node::Remote(_,_)=>!self.collapsed.contains(&key),
                            Node::Objects(..)=>!self.collapsed.contains(&key),
                            Node::Connection(c) => {
                                !self.collapsed.contains(&key)
                                    && (c.kind.is_none()
                                        || self.ssh_hosts.contains_key(&c.id)
                                        || self.local_trees.contains_key(&c.id) || self.mongo_hosts.contains_key(&c.id) || self.redis_hosts.contains_key(&c.id)
                                        || self.sql_hosts.contains_key(&c.id) || self.remote_hosts.contains_key(&c.id))
                            }
                            Node::Schema(_, _, expanded)|Node::Database(_,_,expanded) => *expanded && !self.collapsed.contains(&key),
                            Node::LocalFile(c, entry) => self.local_trees.get(&c.id).is_some_and(|host|host.directories.contains_key(&entry.path) || host.loading(&entry.path)) && !self.collapsed.contains(&key),
                            Node::Ssh(..)=>!self.collapsed.contains(&key),
                            _ => false,
                        };
                        if expandable {
                            let arrow = egui::Rect::from_min_size(
                                egui::pos2(x, rect.top()),
                                egui::vec2(16.0, rect.height()),
                            );
                            let center = arrow.center();
                            let points = if expanded {
                                vec![
                                    center + egui::vec2(-5.0, -2.5),
                                    center + egui::vec2(0.0, 2.5),
                                    center + egui::vec2(5.0, -2.5),
                                ]
                            } else {
                                vec![
                                    center + egui::vec2(-2.5, -5.0),
                                    center + egui::vec2(2.5, 0.0),
                                    center + egui::vec2(-2.5, 5.0),
                                ]
                            };
                            ui.painter().add(egui::Shape::line(
                                points.clone(),
                                egui::Stroke::new(1.8, Color32::from_gray(136)),
                            ));
                            // JavaFX uses ROUND line caps and joins on this path.
                            for point in points {
                                ui.painter().circle_filled(point,0.9,Color32::from_gray(136));
                            }
                            if ui
                                .interact(
                                    arrow,
                                    ui.make_persistent_id(("expand", &key)),
                                    egui::Sense::click(),
                                )
                                .clicked() || keyboard_active && reveal.is_none() && Some(&key) == self.selected_connection.as_ref() && ui.input(|i| (!expanded && i.key_pressed(egui::Key::ArrowRight)) || (expanded && i.key_pressed(egui::Key::ArrowLeft)))
                            {
                                match &row.node {
                                    Node::Remote(c,e)=>action=Some(TreeAction::Remote((*c).clone(),(*e).clone(),"expand")),
                                    Node::Backup(c,database,path,item,view)=>{
                                        if expanded{self.collapsed.insert(key.clone());}else{self.collapsed.remove(&key);action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),"expand"));}
                                    }
                                    Node::Query(c,namespace,path,entry)=>{
                                        if !self.collapsed.remove(&key){self.collapsed.insert(key.clone());}else{action=Some(TreeAction::Query(super::query_tree::row_action(c,namespace,path,*entry,"expand")));}
                                    }
                                    Node::LocalFile(c,entry) if entry.directory && !expanded=>{action=Some(TreeAction::LocalFolder((*c).clone(),entry.path.clone(),false));}
                                    Node::Connection(c)
                                        if c.kind.is_some()
                                            && c.kind()!="SSH"
                                            && c.kind()!="LOCAL_DIRECTORY"
                                            && !(matches!(c.kind(),"MYSQL"|"POSTGRESQL"|"ORACLE"|"HIVE")&&self.sql_hosts.contains_key(&c.id))
                                            && !self
                                                .tabs
                                                .iter()
                                                .any(|tab| tab.source_id == c.id) =>
                                    {
                                        action = Some(TreeAction::Connection("open", c.id.clone()))
                                    }
                                    Node::Database(c,d,_) if !expanded=>{action=Some(TreeAction::Database((*c).clone(),(*d).into(),false));}
                                    Node::Schema(c,s,_) if c.kind()=="MONGODB" && !expanded && self.mongo_hosts.get(&c.id).is_some_and(|h|h.collections.contains_key(*s))=>{self.collapsed.remove(&key);}
                                    Node::Schema(c, s, _) if !expanded => {
                                        self.collapsed.remove(&key);
                                        action =
                                            Some(TreeAction::Schema((*c).clone(), (*s).into(),false));
                                    }
                                    _ => {
                                        if !self.collapsed.remove(&key) {
                                            self.collapsed.insert(key.clone());
                                        }
                                    }
                                }
                            }
                        }
                        let icon_size=match &row.node {Node::Connection(c) if connected.contains(c.id.as_str())=>16.,_=>20.};
                        let icon_rect=egui::Rect::from_min_size(egui::pos2(x+16.,rect.center().y-icon_size/2.),egui::vec2(icon_size,icon_size));
                        let loading=match &row.node {
                            Node::Database(c,d,_)=>self.sql_hosts.get(&c.id).and_then(|h|h.pg.as_ref()).and_then(|pg|pg.databases.get(*d)).is_some_and(|db|db.loading),
                            Node::Connection(c)=>self.sql_hosts.get(&c.id).is_some_and(|h|h.direct.as_ref().is_some_and(|tree|tree.loading)||h.pg.as_ref().is_some_and(|pg|pg.loading))||self.mongo_hosts.get(&c.id).is_some_and(|h|h.busy&&h.loading.is_none()) || self.redis_hosts.get(&c.id).is_some_and(|h|h.busy),
                            Node::Schema(c,s,_)=>self.sql_hosts.get(&c.id).is_some_and(|h|h.direct.as_ref().is_some_and(|tree|tree.scopes.get(*s).is_some_and(|scope|scope.loading)))||self.sql_hosts.get(&c.id).and_then(|h|h.pg.as_ref()).and_then(|pg|pg.databases.get(c.text("database"))).and_then(|db|db.scopes.get(*s)).is_some_and(|scope|scope.loading)||self.mongo_hosts.get(&c.id).is_some_and(|h|h.busy&&h.loading.as_deref()==Some(*s)),_=>false,
                        };
                        if loading {ui.place(icon_rect,egui::Spinner::new().size(16.));}else{self.icons.paint(ui,&row.icon(),icon_rect);}
                        let renaming=matches!((&row.node,&self.local_edit),(Node::LocalFile(c,entry),Some(edit)) if edit.kind==super::local_tree::Kind::Rename && edit.connection.id==c.id && edit.path==entry.path);
                        if inline_editing {
                            rename_rect=Some(egui::Rect::from_min_max(egui::pos2(x+20.+icon_size,rect.top()),rect.right_bottom()-egui::vec2(2.,0.)));
                        } else if let (Node::Connection(c),Some(rename))=(&row.node,&mut self.connection_rename) && c.id==rename.id {
                            let editor=ui.place(egui::Rect::from_min_max(egui::pos2(x+20.+icon_size,rect.top()+4.),rect.right_bottom()-egui::vec2(2.,4.)),TextEdit::singleline(&mut rename.name));
                            if rename.focus {editor.request_focus();rename.focus=false;}
                            if ui.input(|i|i.key_pressed(egui::Key::Escape)) {action=Some(TreeAction::CancelConnectionName);}
                            else if editor.lost_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter)) {action=Some(TreeAction::SubmitConnectionName);}
                        } else if renaming {
                            let edit=self.local_edit.as_mut().unwrap();
                            let editor=ui.place(egui::Rect::from_min_max(egui::pos2(x+20.+icon_size,rect.top()+4.),rect.right_bottom()-egui::vec2(2.,4.)),TextEdit::singleline(&mut edit.name).interactive(!edit.busy));
                            if edit.focus {editor.request_focus();edit.focus=false;}
                            if ui.input(|i|i.key_pressed(egui::Key::Escape)) {action=Some(TreeAction::CancelRename);}
                            else if editor.lost_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter)) {action=Some(TreeAction::SubmitRename);}
                        } else {ui.painter().text(
                            egui::pos2(
                                x + 20. + icon_size,
                                rect.center().y,
                            ),
                            egui::Align2::LEFT_CENTER,
                            row.name(),
                            crate::theme::java_font(12.0),
                            Color32::BLACK,
                        );}
                        super::tree_rename::clicked(ui,&mut self.tree_rename,row,&response,super::tree_rename::Click {name_x:x+20.+icon_size,filter:&self.filter,was_single,allowed:rename_allowed});
                        if response.clicked() {
                            response.request_focus();
                            self.selected_connection = Some(key.clone());
                            if let Node::Table(c, table) = &row.node && matches!(c.kind(),"MYSQL"|"POSTGRESQL"|"ORACLE"|"HIVE") {
                                if rows.iter().any(|candidate|self.sql_selection.contains(&candidate.key())&&matches!(&candidate.node,Node::Table(other,item) if other.id!=c.id||other.text("database")!=c.text("database")||item.table.schema!=table.table.schema)) {self.sql_selection.clear();self.sql_anchor=None;}
                                let modifiers=ui.input(|i|i.modifiers);
                                if modifiers.shift {
                                    let anchor=self.sql_anchor.as_ref().and_then(|key|rows.iter().position(|row|row.key()==*key)).unwrap_or(index);
                                    if !modifiers.command { self.sql_selection.clear(); }
                                    for candidate in &rows[anchor.min(index)..=anchor.max(index)] {
                                        if let Node::Table(other, item)=&candidate.node && other.id==c.id && other.text("database")==c.text("database") && item.table.schema==table.table.schema {self.sql_selection.insert(candidate.key());}
                                    }
                                } else if modifiers.command {
                                    if !self.sql_selection.remove(&key) {self.sql_selection.insert(key.clone());}
                                    self.sql_anchor=Some(key.clone());
                                } else {
                                    self.sql_selection.clear(); self.sql_selection.insert(key.clone()); self.sql_anchor=Some(key.clone());
                                }
                            } else {self.sql_selection.clear(); self.sql_anchor=None;}
                            if let Node::LocalFile(c,entry)=&row.node {
                                self.connection_selection.clear();self.connection_anchor=None;
                                let modifiers=ui.input(|i|i.modifiers);
                                if modifiers.shift {
                                    let anchor=self.local_anchor.as_ref().and_then(|key|rows.iter().position(|row|row.key()==*key)).unwrap_or(index);
                                    if !modifiers.command {self.local_selection.clear();}
                                    for row in &rows[anchor.min(index)..=anchor.max(index)] {if let Node::LocalFile(other,file)=&row.node && other.id==c.id {self.local_selection.insert((c.id.clone(),file.path.clone()));}}
                                } else if modifiers.command {
                                    let value=(c.id.clone(),entry.path.clone());
                                    if !self.local_selection.remove(&value) {self.local_selection.insert(value);}
                                    self.local_anchor=Some(key.clone());
                                } else {self.local_selection.clear();self.local_selection.insert((c.id.clone(),entry.path.clone()));self.local_anchor=Some(key.clone());}
                            } else {
                                self.local_selection.clear();self.local_anchor=None;
                                if let Node::Connection(c)=&row.node {
                                    let modifiers=ui.input(|i|i.modifiers);
                                    if modifiers.shift {
                                        let anchor=self.connection_anchor.as_ref().and_then(|key|rows.iter().position(|row|row.key()==*key)).unwrap_or(index);
                                        if !modifiers.command {self.connection_selection.clear();}
                                        for row in &rows[anchor.min(index)..=anchor.max(index)] {if let Node::Connection(c)=&row.node {self.connection_selection.insert(c.id.clone());}}
                                    } else if modifiers.command {
                                        if !self.connection_selection.remove(&c.id) {self.connection_selection.insert(c.id.clone());}
                                        self.connection_anchor=Some(c.id.clone());
                                    } else {
                                        self.connection_selection.clear();self.connection_selection.insert(c.id.clone());self.connection_anchor=Some(c.id.clone());
                                    }
                                } else {self.connection_selection.clear();self.connection_anchor=None;}
                            }
                        }
                        if response.secondary_clicked() && let Node::Table(c,_)=&row.node && matches!(c.kind(),"MYSQL"|"POSTGRESQL"|"ORACLE"|"HIVE") {
                            response.request_focus(); self.selected_connection=Some(key.clone());
                            if !self.sql_selection.contains(&key) {self.sql_selection.clear(); self.sql_selection.insert(key.clone()); self.sql_anchor=Some(key.clone());}
                        }
                        if response.secondary_clicked() && let Node::Connection(c)=&row.node {
                            response.request_focus();self.selected_connection=Some(key.clone());
                            self.local_selection.clear();self.local_anchor=None;
                            if !self.connection_selection.contains(&c.id) {self.connection_selection.clear();self.connection_selection.insert(c.id.clone());self.connection_anchor=Some(c.id.clone());}
                        }
                        if response.secondary_clicked() && let Node::LocalFile(c,entry)=&row.node && !selected_local {
                            response.request_focus();
                            self.connection_selection.clear();self.connection_anchor=None;
                            self.local_selection.clear();self.local_selection.insert((c.id.clone(),entry.path.clone()));self.selected_connection=Some(key.clone());self.local_anchor=Some(key.clone());
                        }
                        if response.secondary_clicked() && !matches!(&row.node,Node::Connection(_)|Node::LocalFile(..)) {
                            response.request_focus();self.selected_connection=Some(key.clone());self.connection_selection.clear();self.connection_anchor=None;self.local_selection.clear();self.local_anchor=None;
                        }
                        let can_rename_row=rename_allowed && super::tree_rename::eligible(&row.node)
                            && super::tree_rename::single(row,self.selected_connection.as_deref(),&self.connection_selection,&self.local_selection,&self.sql_selection);
                        if response.clicked()&&let Node::Objects(c,s,db_tree::Folder::Tables)=&row.node {
                            action=Some(TreeAction::Objects((*c).clone(),(*s).into()));
                        }
                        if response.double_clicked() || keyboard_active && Some(&key) == self.selected_connection.as_ref() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            match &row.node {
                                Node::Remote(c,e)=>action=Some(TreeAction::Remote((*c).clone(),(*e).clone(),if matches!(e.kind,super::remote_tree::Kind::Product(_)){"expand"}else{"open"})),
                                Node::Backup(c,database,path,item,view)=>{
                                    if item.is_some_and(|item|!item.directory){action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),"restore"));}
                                    else if expanded{self.collapsed.insert(key.clone());}else{self.collapsed.remove(&key);action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),"expand"));}
                                }
                                Node::Query(c,namespace,path,entry)=>{
                                    if entry.is_none_or(|entry|entry.directory){
                                        if expandable&&!self.collapsed.contains(&key){self.collapsed.insert(key.clone());}else{self.collapsed.remove(&key);action=Some(TreeAction::Query(super::query_tree::row_action(c,namespace,path,*entry,"expand")));}
                                    }else{action=Some(TreeAction::Query(super::query_tree::row_action(c,namespace,path,*entry,"open")));}
                                }
                                Node::Connection(c) if c.kind.is_some() && !expandable => {
                                    action = Some(TreeAction::Connection("open", c.id.clone()))
                                }
                                Node::Connection(_) => {
                                    if !self.collapsed.remove(&key) {
                                        self.collapsed.insert(key.clone());
                                    }
                                }
                                Node::Database(c,d,_)=>{if expanded{self.collapsed.insert(key.clone());}else{action=Some(TreeAction::Database((*c).clone(),(*d).into(),false));}}
                                Node::Schema(c, s, opened) => {
                                    if matches!(c.kind(),"MYSQL"|"POSTGRESQL"|"ORACLE"|"HIVE") && *opened && expanded { self.collapsed.insert(key.clone()); }
                                    else { action = Some(TreeAction::Schema((*c).clone(), (*s).into(),false)); }
                                }
                                Node::Table(c, t) => {
                                    action =
                                        Some(TreeAction::Table((*c).clone(), (*t).clone(), false))
                                }
                                Node::Objects(c,s,folder)=>{
                                    if !self.collapsed.remove(&key){self.collapsed.insert(key.clone());}
                                    if *folder==db_tree::Folder::Tables{action=Some(TreeAction::Objects((*c).clone(),(*s).into()));}
                                }
                                Node::Ssh(c,section)=>{
                                    if *section==tomato_services::terminal::ssh::tools::Section::Docker {
                                        if expandable {if !self.collapsed.remove(&key){self.collapsed.insert(key.clone());}}else{self.collapsed.remove(&key);action=Some(TreeAction::LoadContainers((*c).clone()));}
                                    }else{action=Some(TreeAction::Ssh((*c).clone(),*section));}
                                }
                                Node::Container(c,container)=>action=Some(TreeAction::Container((*c).clone(),container.id.clone(),if container.running{"terminal"}else{"start"})),
                                Node::LocalFile(c, entry) => {
                                    if entry.directory { action=Some(TreeAction::LocalFolder((*c).clone(), entry.path.clone(), false)); }
                                    else if super::local_tree::markdown(&entry.path) { action=Some(TreeAction::LocalFile((*c).clone(),entry.path.clone())); }
                                }
                            }
                        }
                        if !matches!(&row.node,Node::Ssh(_,section) if *section != tomato_services::terminal::ssh::tools::Section::Docker) && !matches!(&row.node,Node::Remote(_,entry) if matches!(entry.kind,super::remote_tree::Kind::Messages(_))) {
                        response.context_menu(|ui| match &row.node {
                            Node::Remote(c,e)=>{
                                let items:&[(&str,&str)]=match e.kind{
                                    super::remote_tree::Kind::Product(ref product) if matches!(product.as_str(),"domain"|"dnspod")=>&[("open","打开域名列表"),("refresh","刷新")],
                                    super::remote_tree::Kind::Product(_)=>&[("refresh","刷新")],
                                    super::remote_tree::Kind::Topics=>&[("new","创建主题"),("refresh","刷新")],
                                    super::remote_tree::Kind::Topic(_)=>&[("open","查看详情"),("send","发送消息"),("delete","删除主题")],
                                    super::remote_tree::Kind::Groups|super::remote_tree::Kind::Cluster=>&[("refresh","刷新")],
                                    super::remote_tree::Kind::Group(_)|super::remote_tree::Kind::Broker(_)=>&[("open","查看详情")],
                                    super::remote_tree::Kind::Instance=>&[("open","属性")],
                                    super::remote_tree::Kind::Domain(_) if c.kind()=="ALIYUN"=>&[("refresh","刷新"),("certificate","申请 Let's Encrypt 证书")],
                                    super::remote_tree::Kind::Domain(_)=>&[("refresh","刷新"),("certificate","申请证书")],
                                    _=>&[("open","打开")],
                                };for(verb,label)in items{if *verb=="delete" || *verb=="refresh" && matches!(e.kind,super::remote_tree::Kind::Topics){crate::compact_menu::separator(ui);}
                                if ui.add(crate::compact_menu::Item::new(label)).clicked(){action=Some(TreeAction::Remote((*c).clone(),(*e).clone(),verb));ui.close();}}
                            }

                            Node::Backup(c,database,path,item,view)=>{
                                let folder=item.is_none_or(|item|item.directory);
                                if folder{
                                    if ui.add(crate::compact_menu::Item::new("打开目录")).clicked(){action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),"directory"));ui.close();}
                                    crate::compact_menu::separator(ui);
                                    for (label,verb) in [("新建备份","new"),("新建目录","mkdir")] {if ui.add_enabled(verb!="rename"||can_rename_row,crate::compact_menu::Item::new(label)).clicked(){action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),verb));ui.close();}}
                                    if item.is_some(){crate::compact_menu::separator(ui);for(label,verb)in[("重命名","rename"),("删除","delete")]{if ui.add_enabled(verb!="rename"||can_rename_row,crate::compact_menu::Item::new(label)).clicked(){action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),verb));ui.close();}}}
                                    crate::compact_menu::separator(ui);if ui.add(crate::compact_menu::Item::new("刷新")).clicked(){action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),"refresh"));ui.close();}
                                }else{
                                    if ui.add(crate::compact_menu::Item::new("还原备份")).clicked(){action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),"restore"));ui.close();}
                                    crate::compact_menu::separator(ui);if ui.add(crate::compact_menu::Item::new("打开备份目录")).clicked(){action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),"directory"));ui.close();}
                                    crate::compact_menu::separator(ui);for(label,verb)in[("重命名","rename"),("删除","delete")]{if ui.add_enabled(verb!="rename"||can_rename_row,crate::compact_menu::Item::new(label)).clicked(){action=Some(TreeAction::Backup(super::backup_tree::target(c,database,path,*item,*view),verb));ui.close();}}
                                }
                            }
                            Node::Query(c,namespace,path,entry)=>{
                                let directory=entry.is_none_or(|entry|entry.directory);
                                let items:&[(&str,&str)]=if entry.is_none() {&[("folder","打开目录"),("separator",""),("new","新建查询"),("directory","新建目录"),("separator",""),("refresh","刷新")]}else if directory {&[("folder","打开目录"),("separator",""),("new","新建查询"),("directory","新建目录"),("separator",""),("rename","重命名"),("delete","删除"),("separator",""),("refresh","刷新")]}else{&[("open","打开"),("separator",""),("rename","重命名"),("delete","删除")]};
                                for (verb,label) in items {
                                    if entry.is_none()&&matches!(*verb,"rename"|"delete"){continue;}
                                    if *verb=="separator"{crate::compact_menu::separator(ui);continue;}
                                    if ui.add_enabled(*verb!="rename"||can_rename_row,crate::compact_menu::Item::new(label)).clicked(){action=Some(TreeAction::Query(super::query_tree::row_action(c,namespace,path,*entry,verb)));ui.close();}
                                }
                            }
                            Node::Connection(c) => {
                                if c.kind.is_none() {
                                    for (id,label) in [("new_folder","新建目录"),("new_connection","新建连接")] {if ui.add_enabled(id!="rename"||can_rename_row,crate::compact_menu::Item::new(label)).clicked() {action=Some(TreeAction::Connection(id,c.id.clone()));ui.close();}}
                                    crate::compact_menu::separator(ui);
                                    for (id,label) in [("rename","重命名"),("delete","删除")] {if ui.add_enabled(id!="rename"||can_rename_row,crate::compact_menu::Item::new(label)).clicked() {action=Some(TreeAction::Connection(id,c.id.clone()));ui.close();}}
                                    return;
                                }
                                if c.kind()=="LOCAL_DIRECTORY" {
                                    for (kind,label,icon) in [(super::local_tree::Kind::Directory,"新建目录","folder.png"),(super::local_tree::Kind::Markdown,"新建 Markdown 文档","md_add.png")] {
                                        if local_menu(ui,&mut self.icons,label,icon) {action=Some(TreeAction::LocalEdit((*c).clone(),super::local_tree::root(c),kind));ui.close();}
                                    }
                                    crate::compact_menu::separator(ui);
                                }
                                if c.kind()=="LOCAL_DIRECTORY" && ui.add(crate::compact_menu::Item::new("刷新")).clicked() {
                                    action=Some(TreeAction::LocalFolder((*c).clone(), super::local_tree::root(c), true)); ui.close();
                                }
                                if c.kind()=="MONGODB" {
                                    for (id,label) in [("mongo_create_database","新建数据库"),("mongo_refresh","刷新"),("mongo_disconnect","关闭连接")] {
                                        if ui.add(crate::compact_menu::Item::new(label)).clicked(){action=Some(TreeAction::Connection(id,c.id.clone()));ui.close();}
                                    }
                                    crate::compact_menu::separator(ui);
                                }
                                if c.kind()=="REDIS" {
                                    for (id,label) in [("redis_refresh","刷新"),("redis_disconnect","关闭连接")] {if ui.add(crate::compact_menu::Item::new(label)).clicked(){action=Some(TreeAction::Connection(id,c.id.clone()));ui.close();}}
                                    crate::compact_menu::separator(ui);
                                }
                                if matches!(c.kind(),"ALIYUN"|"TENCENT_CLOUD"|"KAFKA"|"ROCKETMQ")&&self.remote_hosts.contains_key(&c.id){
                                    if ui.add(crate::compact_menu::Item::new("关闭连接")).clicked(){action=Some(TreeAction::Connection("remote_disconnect",c.id.clone()));ui.close();}
                                    if ui.add(crate::compact_menu::Item::new("刷新")).clicked(){action=Some(TreeAction::Connection("remote_refresh",c.id.clone()));ui.close();}
                                    crate::compact_menu::separator(ui);
                                }
                                let sql_open=self.sql_hosts.get(&c.id).is_some_and(|host|!host.schemas.is_empty()||host.pg.as_ref().is_some_and(|pg|!pg.names.is_empty()));
                                if matches!(c.kind(),"MYSQL"|"POSTGRESQL"|"ORACLE"|"HIVE")&&ui.add_enabled(sql_open,crate::compact_menu::Item::new("新建数据库")).clicked(){action=Some(TreeAction::DatabaseAdmin((*c).clone(),String::new(),super::database_admin::Mode::Create));ui.close();}
                                if (matches!(c.kind(),"MYSQL"|"POSTGRESQL"|"ORACLE"|"HIVE")&&sql_open)||(c.kind()=="SQLITE"&&self.sql_hosts.contains_key(&c.id)) {
                                    if ui.add(crate::compact_menu::Item::new("关闭连接")).clicked(){action=Some(TreeAction::Connection("sql_disconnect",c.id.clone()));ui.close();}
                                    crate::compact_menu::separator(ui);
                                }
                                for (id, label) in [
                                    ("open", "连接"),
                                    ("edit", "编辑"),
                                    ("rename", "重命名"),
                                    ("copy_connection", "复制连接"),
                                    ("delete", "删除"),
                                ] {
                                    if c.kind()=="TOOL" && c.text("toolType") != "music_downloader" && matches!(id,"open"|"edit") {continue;}
                                    if matches!(id,"edit"|"delete") {crate::compact_menu::separator(ui);}
                                    if ui.add_enabled(id!="rename"||can_rename_row,crate::compact_menu::Item::new(label)).clicked() {
                                        action = Some(TreeAction::Connection(id, c.id.clone()));
                                        ui.close();
                                    }
                                }
                                if matches!(c.kind(),"SSH"|"LOCAL_TERMINAL") && ui.add(crate::compact_menu::Item::new("复制会话")).clicked() {action=Some(TreeAction::Connection("copy",c.id.clone()));ui.close();}
                                if matches!(c.kind(),"MYSQL"|"POSTGRESQL"|"ORACLE"|"HIVE") && sql_open {
                                    crate::compact_menu::separator(ui);if ui.add(crate::compact_menu::Item::new("刷新")).clicked(){action=Some(TreeAction::Connection("sql_refresh",c.id.clone()));ui.close();}
                                }
                            }
                            Node::Database(c,d,opened)=>{if ui.add(crate::compact_menu::Item::new(if *opened {"关闭"} else {"打开"})).clicked(){action=Some(if *opened {TreeAction::CloseDatabase((*c).clone(),(*d).into())}else{TreeAction::Database((*c).clone(),(*d).into(),false)});ui.close();}
                                crate::compact_menu::separator(ui);
                                if ui.add(crate::compact_menu::Item::new("编辑")).clicked(){action=Some(TreeAction::DatabaseAdmin((*c).clone(),(*d).into(),super::database_admin::Mode::Edit));ui.close();}
                                if ui.add(crate::compact_menu::Item::new("删除")).clicked(){action=Some(TreeAction::DatabaseAdmin((*c).clone(),(*d).into(),super::database_admin::Mode::Drop));ui.close();}
                                crate::compact_menu::separator(ui); if ui.add_enabled(self.table_clipboard_port.available(),crate::compact_menu::Item::new("粘贴表")).clicked(){let mut target=(*c).clone();target.set("database",(*d).to_owned());action=Some(TreeAction::PasteTables(target,"public".into()));ui.close();}
                                crate::compact_menu::separator(ui);
                                if ui.add(crate::compact_menu::Item::new("刷新")).clicked(){action=Some(TreeAction::Database((*c).clone(),(*d).into(),true));ui.close();}}
                            Node::Schema(c, s, opened) => {
                                if c.kind()=="REDIS" {
                                    if ui.add(crate::compact_menu::Item::new("打开")).clicked(){action=Some(TreeAction::Schema((*c).clone(),(*s).into(),false));ui.close();}
                                    return;
                                }
                                if c.kind()=="MONGODB" {
                                    if ui.add(crate::compact_menu::Item::new("打开")).clicked(){action=Some(TreeAction::Schema((*c).clone(),(*s).into(),false));ui.close();}
                                    if ui.add(crate::compact_menu::Item::new("新建集合")).clicked(){action=Some(TreeAction::MongoPrompt((*c).clone(),tomato_services::mongo::Target {database:(*s).into(),collection:String::new()},super::mongo_tree::Kind::Create));ui.close();}
                                    if ui.add(crate::compact_menu::Item::new("刷新")).clicked(){action=Some(TreeAction::MongoRefresh((*c).clone(),Some((*s).into())));ui.close();}
                                    return;
                                }
                                if c.kind()=="MYSQL" {
                                    if ui.add(crate::compact_menu::Item::new(if *opened {"关闭"} else {"打开"})).clicked() {
                                        action=Some(if *opened {TreeAction::CloseDatabase((*c).clone(),(*s).into())} else {TreeAction::Schema((*c).clone(),(*s).into(),false)}); ui.close();
                                    }
                                    crate::compact_menu::separator(ui);
                                    for (label, mode) in [("编辑",super::database_admin::Mode::Edit),("删除",super::database_admin::Mode::Drop)] {
                                        if ui.add(crate::compact_menu::Item::new(label)).clicked() {action=Some(TreeAction::DatabaseAdmin((*c).clone(),(*s).into(),mode));ui.close();}
                                    }
                                    crate::compact_menu::separator(ui);
                                    if ui.add_enabled(self.table_clipboard_port.available(), crate::compact_menu::Item::new("粘贴表")).clicked(){action=Some(TreeAction::PasteTables((*c).clone(),(*s).into()));ui.close();}
                                }
                                if c.kind()=="ORACLE" && ui.add(crate::compact_menu::Item::new(if *opened { "关闭" } else { "打开" })).clicked() {
                                    action=Some(if *opened {TreeAction::CloseDatabase((*c).clone(),(*s).into())} else {TreeAction::Schema((*c).clone(),(*s).into(),false)});ui.close();
                                }
                                if c.kind()=="ORACLE" {
                                    crate::compact_menu::separator(ui);
                                    for (label,mode) in [("编辑",super::database_admin::Mode::Edit),("删除",super::database_admin::Mode::Drop)] {
                                        if ui.add(crate::compact_menu::Item::new(label)).clicked() {action=Some(TreeAction::DatabaseAdmin((*c).clone(),(*s).into(),mode));ui.close();}
                                    }
                                    crate::compact_menu::separator(ui);
                                    if ui.add_enabled(self.table_clipboard_port.available(),crate::compact_menu::Item::new("粘贴表")).clicked(){action=Some(TreeAction::PasteTables((*c).clone(),(*s).into()));ui.close();}
                                }
                                if c.kind()=="POSTGRESQL" && ui.add(crate::compact_menu::Item::new("打开")).clicked(){action=Some(TreeAction::Schema((*c).clone(),(*s).into(),false));ui.close();}
                                crate::compact_menu::separator(ui);
                                if ui.add(crate::compact_menu::Item::new("刷新")).clicked() {
                                    action = Some(TreeAction::Schema((*c).clone(), (*s).into(),true));
                                    ui.close();
                                }
                            }
                            Node::Table(c, t) => {
                                if c.kind()=="MONGODB" {
                                    if ui.add(crate::compact_menu::Item::new("打开文档")).clicked(){action=Some(TreeAction::Table((*c).clone(),(*t).clone(),false));ui.close();}
                                    for (label,kind) in [("重命名集合",super::mongo_tree::Kind::Rename),("删除集合",super::mongo_tree::Kind::Drop)] {
                                        if ui.add(crate::compact_menu::Item::new(label)).clicked(){action=Some(TreeAction::MongoPrompt((*c).clone(),tomato_services::mongo::Target {database:t.table.schema.clone(),collection:t.table.name.clone()},kind));ui.close();}
                                    }
                                    return;
                                }
                                for (label, structure) in [("打开表", false), ("设计表", true)]
                                {
                                    if local_menu(ui,&mut self.icons,label,if structure {"table_edit.png"} else {"table_open.png"}) {
                                        action = Some(TreeAction::Table(
                                            (*c).clone(),
                                            (*t).clone(),
                                            structure,
                                        ));
                                        ui.close();
                                    }
                                }
                                if local_menu(ui,&mut self.icons,"新建表","table_add.png"){action=Some(TreeAction::NewTable((*c).clone(),t.table.schema.clone()));ui.close();}
                                crate::compact_menu::separator(ui);
                                if local_menu(ui,&mut self.icons,"复制表","copy_tables.png"){action=Some(TreeAction::CopyTable((*c).clone(),t.table.clone()));ui.close();}
                                if ui.add_enabled(can_rename_row,crate::compact_menu::Item::new("重命名")).clicked(){action=Some(TreeAction::Rename);ui.close();}
                                crate::compact_menu::separator(ui);
                                if !t.view {
                                    for(label,mutation)in[("清空表",tomato_core::database_objects::Mutation::Clear),("截断表",tomato_core::database_objects::Mutation::Truncate)] {
                                        if local_menu(ui,&mut self.icons,label,"table_drop.png") {action=Some(TreeAction::MutateObjects((*c).clone(),tomato_core::database_objects::Request{targets:vec![tomato_core::database_objects::Target{table:t.table.clone(),view:false}],mutation}));ui.close();}
                                    }
                                    crate::compact_menu::separator(ui);
                                }
                                if local_menu(ui,&mut self.icons,"删除表","table_drop.png") {action=Some(TreeAction::MutateObjects((*c).clone(),tomato_core::database_objects::Request{targets:super::tree_objects::targets(&rows,&self.sql_selection,c,t),mutation:tomato_core::database_objects::Mutation::Drop}));ui.close();}
                                crate::compact_menu::separator(ui);
                                if local_menu(ui,&mut self.icons,"刷新","refresh.png"){action=Some(TreeAction::Schema((*c).clone(),t.table.schema.clone(),true));ui.close();}
                            }
                            Node::Objects(c,s,folder)=>{
                                if *folder==db_tree::Folder::Tables {
                                    if ui.add(crate::compact_menu::Item::new("打开对象")).clicked(){action=Some(TreeAction::Objects((*c).clone(),(*s).into()));ui.close();}
                                    crate::compact_menu::separator(ui);
                                    if ui.add(crate::compact_menu::Item::new("新建表")).clicked(){action=Some(TreeAction::NewTable((*c).clone(),(*s).into()));ui.close();}
                                    if local_menu(ui,&mut self.icons,"新建查询","query.png"){action=Some(TreeAction::NewQuery((*c).clone(),(*s).into()));ui.close();}
                                    crate::compact_menu::separator(ui);
                                    if ui.add_enabled(self.table_clipboard_port.available(),crate::compact_menu::Item::new("粘贴表")).clicked(){action=Some(TreeAction::PasteTables((*c).clone(),(*s).into()));ui.close();}
                                    crate::compact_menu::separator(ui);
                                }
                                if *folder!=db_tree::Folder::Functions&&ui.add(crate::compact_menu::Item::new("刷新")).clicked(){action=Some(TreeAction::Schema((*c).clone(),(*s).into(),true));ui.close();}
                            }
                            Node::Ssh(c,section)=>{
                                if *section==tomato_services::terminal::ssh::tools::Section::Docker && ui.add(crate::compact_menu::Item::new("刷新")).clicked() {action=Some(TreeAction::LoadContainers((*c).clone()));ui.close();}

                            }
                            Node::Container(c,container)=>{
                                let items=if container.running {vec![("stop","停止"),("restart","重启"),("logs","查看实时日志"),("terminal","进入终端"),("inspect","属性")]}else{vec![("start","启动"),("inspect","属性")]};
                                let busy=self.ssh_hosts.get(&c.id).is_some_and(|h|h.job.is_some());
                                for (id,label) in items {if matches!(id,"logs"|"inspect"){crate::compact_menu::separator(ui);}
                                    if ui.add_enabled(!busy,crate::compact_menu::Item::new(label)).clicked(){action=Some(TreeAction::Container((*c).clone(),container.id.clone(),id));ui.close();}}
                            }
                            Node::LocalFile(c, entry)=>{
                                use super::local_tree::Kind;
                                if entry.directory {
                                    for (kind,label,icon) in [(Kind::Directory,"新建目录","folder.png"),(Kind::Markdown,"新建 Markdown 文档","md_add.png")] {
                                        if local_menu(ui,&mut self.icons,label,icon) {action=Some(TreeAction::LocalEdit((*c).clone(),entry.path.clone(),kind));ui.close();}
                                    }
                                    crate::compact_menu::separator(ui);
                                }
                                if ui.add_enabled(can_rename_row,crate::compact_menu::Item::new("重命名")).clicked() {action=Some(TreeAction::Rename);ui.close();}
                                if entry.directory && ui.add(crate::compact_menu::Item::new("刷新")).clicked() {action=Some(TreeAction::LocalFolder((*c).clone(),entry.path.clone(),true));ui.close();}
                                crate::compact_menu::separator(ui);
                                if ui.add(crate::compact_menu::Item::new("删除")).clicked() {action=Some(TreeAction::LocalEdit((*c).clone(),entry.path.clone(),Kind::Delete));ui.close();}
                            }
                        });
                        }
                    }
                    if let Some(rect)=rename_rect {
                        rename_commit=super::tree_rename::render(ui,&mut self.tree_rename,rect);
                    }
                },
            );
        rename_commit =
            rename_commit.or_else(|| super::tree_rename::finish_frame(ui, &mut self.tree_rename));
        tree_has_focus &= !was_editing
            && self.tree_rename.as_ref().is_none_or(|d| d.due.is_some())
            && !egui::Popup::is_any_open(ui.ctx());
        let blank_top = (scroll.inner_rect.top() + scroll.content_size.y - scroll.state.offset.y)
            .clamp(scroll.inner_rect.top(), scroll.inner_rect.bottom());
        let blank = egui::Rect::from_min_max(
            egui::pos2(scroll.inner_rect.left(), blank_top),
            scroll.inner_rect.max,
        );
        if blank.height() > 0. {
            let root = ui.interact(
                blank,
                ui.make_persistent_id("connection-tree-root-menu"),
                egui::Sense::click(),
            );
            if root.clicked() || root.secondary_clicked() {
                background.request_focus();
            }
            root.context_menu(|ui| {
                for (verb, label) in [
                    ("folder", "新建目录"),
                    ("connection", "新建连接"),
                    ("separator", ""),
                    ("import", "导入连接"),
                    ("export", "导出连接"),
                ] {
                    if verb == "separator" {
                        crate::compact_menu::separator(ui);
                    } else if ui.add(crate::compact_menu::Item::new(label)).clicked() {
                        action = Some(TreeAction::Root(verb));
                        ui.close();
                    }
                }
            });
        }
        if rename_allowed
            && tree_has_focus
            && action.is_none()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2))
        {
            action = Some(TreeAction::Rename);
        }
        if let Some(drop) = super::tree_drag::root(
            ui,
            scroll.inner_rect,
            scroll.content_size.y,
            scroll.state.offset.y,
            can_drag,
        ) {
            drop_action = Some(drop);
        }
        if action.is_none()
            && self.local_edit.is_none()
            && self.connection_rename.is_none()
            && self.credential.is_none()
            && self.dialog.is_none()
            && tree_has_focus
            && let Some(Node::LocalFile(connection, entry)) = rows
                .iter()
                .find(|row| Some(row.key()) == self.selected_connection)
                .map(|row| &row.node)
        {
            if ui.input(|i| i.key_pressed(egui::Key::F2)) {
                action = Some(TreeAction::LocalEdit(
                    (*connection).clone(),
                    entry.path.clone(),
                    super::local_tree::Kind::Rename,
                ));
            } else if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
                action = Some(TreeAction::LocalEdit(
                    (*connection).clone(),
                    entry.path.clone(),
                    super::local_tree::Kind::Delete,
                ));
            }
        }
        if action.is_none()
            && tree_has_focus
            && self.local_edit.is_none()
            && self.connection_rename.is_none()
            && self.dialog.is_none()
            && self.credential.is_none()
            && let Some(id) = self
                .selected_connection
                .as_ref()
                .filter(|id| self.store.connections.iter().any(|c| &c.id == *id))
        {
            if ui.input(|i| i.key_pressed(egui::Key::F2)) {
                action = Some(TreeAction::Connection("rename", id.clone()));
            } else if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
                action = Some(TreeAction::Connection("delete", id.clone()));
            }
        }
        if let Some(drop) = drop_action {
            self.apply_tree_drop(drop);
            return;
        }
        if action.is_none()
            && tree_has_focus
            && self.queries.dialog.is_none()
            && let Some(Node::Query(c, namespace, path, Some(entry))) = rows
                .iter()
                .find(|row| Some(row.key()) == self.selected_connection)
                .map(|row| &row.node)
        {
            let verb = if ui.input(|input| input.key_pressed(egui::Key::F2)) {
                Some("rename")
            } else if ui.input(|input| input.key_pressed(egui::Key::Delete)) {
                Some("delete")
            } else {
                None
            };
            if let Some(verb) = verb {
                action = Some(TreeAction::Query(super::query_tree::row_action(
                    c,
                    namespace,
                    path,
                    Some(entry),
                    verb,
                )));
            }
        }
        if action.is_none()
            && tree_has_focus
            && self.backup_edit.is_none()
            && let Some(Node::Backup(c, database, path, Some(item), view)) = rows
                .iter()
                .find(|row| Some(row.key()) == self.selected_connection)
                .map(|row| &row.node)
        {
            let verb = if ui.input(|input| input.key_pressed(egui::Key::F2)) {
                Some("rename")
            } else if ui.input(|input| input.key_pressed(egui::Key::Delete)) {
                Some("delete")
            } else {
                None
            };
            if let Some(verb) = verb {
                action = Some(TreeAction::Backup(
                    super::backup_tree::target(c, database, path, Some(item), *view),
                    verb,
                ));
            }
        }
        if action.is_none()
            && tree_has_focus
            && self.tree_objects.is_none()
            && !self.sql_selection.is_empty()
            && self.credential.is_none()
            && self.dialog.is_none()
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
            && let Some(Node::Table(c, table)) = rows
                .iter()
                .find(|row| Some(row.key()) == self.selected_connection)
                .map(|row| &row.node)
            && matches!(c.kind(), "MYSQL" | "POSTGRESQL" | "ORACLE" | "HIVE")
        {
            action = Some(TreeAction::MutateObjects(
                (*c).clone(),
                tomato_core::database_objects::Request {
                    targets: super::tree_objects::targets(&rows, &self.sql_selection, c, table),
                    mutation: tomato_core::database_objects::Mutation::Drop,
                },
            ));
        }
        if matches!(
            &action,
            Some(
                TreeAction::Rename
                    | TreeAction::Connection("rename", _)
                    | TreeAction::LocalEdit(_, _, super::local_tree::Kind::Rename)
                    | TreeAction::Query(super::query_tree::Action::Rename { .. })
                    | TreeAction::Backup(_, "rename")
            )
        ) {
            action = if rename_allowed {
                rows.iter()
                    .find(|r| Some(r.key()) == self.selected_connection)
                    .filter(|r| {
                        super::tree_rename::single(
                            r,
                            self.selected_connection.as_deref(),
                            &self.connection_selection,
                            &self.local_selection,
                            &self.sql_selection,
                        )
                    })
                    .and_then(|r| super::tree_rename::Draft::new(r, &self.filter, None))
                    .map(TreeAction::BeginRename)
            } else {
                None
            };
        }
        drop(rows);
        if let Some(commit) = rename_commit {
            self.commit_tree_name(commit);
        }
        for (id, view, scope) in backup_loads {
            let key = super::backup_tree::Key::new(&id, &scope.database, &scope.path);
            if !self.backups.directories.contains_key(&key) {
                self.collapsed.insert(key.view_id(view.as_deref()));
                let _ = self.list_backups(&id, scope);
            }
        }
        match action {
            Some(TreeAction::Root(verb)) => match verb {
                "folder" => self.new_folder_at(None),
                "connection" => self.new_connection_at(None),
                "import" => self.import(),
                "export" => self.export(),
                _ => {}
            },
            Some(TreeAction::BeginRename(draft)) => {
                super::tree_rename::finish(&mut self.tree_rename, ui.ctx(), false);
                self.tree_rename = Some(draft);
                ui.ctx().request_repaint();
            }
            Some(TreeAction::Rename) => {}
            Some(TreeAction::Remote(c, e, verb)) => self.remote_entry(c, e, verb),
            Some(TreeAction::MutateObjects(connection, request)) => {
                self.confirm_tree_objects(connection, request)
            }
            Some(TreeAction::Backup(target, verb)) => self.backup_action(target, verb),
            Some(TreeAction::Query(action)) => self.query_action(action),
            Some(TreeAction::SubmitConnectionName) => self.submit_connection_name(),
            Some(TreeAction::CancelConnectionName) => self.connection_rename = None,
            Some(TreeAction::SubmitRename) => self.local_edit_submit(),
            Some(TreeAction::CancelRename) => self.local_edit = None,
            Some(TreeAction::LocalEdit(c, path, kind)) => self.local_edit_start(c, path, kind),
            Some(TreeAction::LocalFolder(connection, path, refresh)) => {
                self.local_tree_open(&connection, Some(path), refresh)
            }
            Some(TreeAction::LocalFile(connection, path)) => {
                self.open_tree_editor(connection, path)
            }
            Some(TreeAction::Ssh(connection, section)) => self.open_ssh_tool(connection, section),
            Some(TreeAction::Container(connection, id, action)) => {
                self.container_action(connection, id, action)
            }
            Some(TreeAction::LoadContainers(connection)) => self.ssh_tree_request(
                connection,
                tomato_services::terminal::ssh::tools::Operation::Query(
                    tomato_services::terminal::ssh::tools::Section::Docker,
                ),
            ),
            Some(TreeAction::MongoPrompt(connection, target, kind)) => {
                self.mongo_prompt(connection, target, kind)
            }
            Some(TreeAction::MongoRefresh(connection, database)) => {
                if let Some(database) = database {
                    self.mongo_collections(&connection.id, database, true);
                } else {
                    self.mongo_open(connection, true);
                }
            }
            Some(TreeAction::Schema(connection, schema, refresh)) => {
                if connection.kind() == "POSTGRESQL"
                    && self
                        .sql_hosts
                        .get(&connection.id)
                        .is_some_and(|h| h.pg.is_some())
                {
                    self.open_pg_schema(connection, schema, refresh);
                } else if matches!(connection.kind(), "MYSQL" | "ORACLE") {
                    self.open_direct_schema(connection, schema, refresh);
                } else {
                    self.select_schema(&connection.id, schema);
                }
            }
            Some(TreeAction::CloseDatabase(connection, database)) => {
                if matches!(connection.kind(), "MYSQL" | "ORACLE") {
                    self.close_direct_database(connection, database);
                } else {
                    self.close_pg_database(connection, database);
                }
            }
            Some(TreeAction::Database(connection, database, refresh)) => {
                self.open_pg_database(connection, database, refresh)
            }
            Some(TreeAction::Table(connection, table, structure)) => {
                self.open_table(connection, table, structure)
            }
            Some(TreeAction::CopyTable(connection, table)) => {
                self.start_copy_table(connection, table)
            }
            Some(TreeAction::PasteTables(connection, schema)) => {
                self.paste_tables(connection, schema)
            }
            Some(TreeAction::DatabaseAdmin(connection, database, mode)) => {
                self.open_database_admin(connection, database, mode)
            }
            Some(TreeAction::NewQuery(connection, schema)) => {
                self.new_scoped_query(connection, schema)
            }
            Some(TreeAction::NewTable(connection, schema)) => self.new_table(connection, schema),
            Some(TreeAction::Objects(connection, schema)) => self.open_objects(connection, schema),
            Some(TreeAction::Connection(action, id)) => {
                if action == "delete" {
                    self.request_connection_delete(&id);
                } else if let Some(connection) =
                    self.store.connections.iter().find(|c| c.id == id).cloned()
                {
                    match action {
                        "open" => {
                            if !matches!(connection.kind(), "LOCAL_DIRECTORY" | "MONGODB" | "REDIS")
                            {
                                self.collapsed.remove(&id);
                            }
                            if connection.kind() == "SSH"
                                && self.settings.ssh_service_management_enabled
                            {
                                self.ssh_hosts.entry(id.clone()).or_default();
                            }
                            self.open_connection(connection);
                        }
                        "edit" => self.edit_connection(connection),
                        "rename" => self.rename_connection(&id),
                        "copy_connection" => self.copy_connection(&id),
                        "new_folder" => self.new_folder_at(Some(id.clone())),
                        "new_connection" => self.new_connection_at(Some(id.clone())),
                        "copy" => self.open_connection(connection),
                        "remote_disconnect" => {
                            self.remote_hosts.remove(&id);
                            self.collapsed.insert(id);
                        }
                        "remote_refresh" => self.remote_open(connection, true),
                        "mongo_disconnect" => self.mongo_disconnect(&id),
                        "sql_disconnect" => self.disconnect_sql(&id),
                        "sql_refresh" => self.refresh_sql_host(connection),
                        "redis_disconnect" => self.redis_disconnect(&id),
                        "redis_refresh" => self.redis_open(connection, true),
                        "mongo_refresh" => self.mongo_open(connection, true),
                        "mongo_create_database" => self.mongo_prompt(
                            connection,
                            tomato_services::mongo::Target {
                                database: String::new(),
                                collection: String::new(),
                            },
                            super::mongo_tree::Kind::Database,
                        ),
                        _ => {}
                    }
                }
            }
            None => {}
        }
    }
    pub(super) fn edit_connection(&mut self, draft: Connection) {
        let editing = self
            .store
            .connections
            .iter()
            .any(|item| item.id == draft.id);
        self.dialog = Some(ConnectionDialog {
            adb_port: match draft.number("port") {
                0 => 5555,
                port => port,
            }
            .to_string(),
            draft,
            adb_pairing: Default::default(),
            tab: 0,
            editing,
        });
    }
    pub(super) fn persist_store(&mut self, next: Store) -> bool {
        if self.load_failed {
            self.error = Some("配置加载失败，保存已禁用。请修复原配置后重启。".into());
            return false;
        }
        if let Err(error) = self.repository.save(&next) {
            self.error = Some(format!("保存失败: {error:#}"));
            false
        } else {
            self.store = next;
            self.collaboration_connections_changed();
            self.status = "连接已保存（原版兼容加密格式）".into();
            true
        }
    }
    pub(super) fn import(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Tomato 配置", &["json"])
            .pick_file()
        {
            self.read_connection_import(path);
        }
    }
    pub(super) fn export(&mut self) {
        if self.store.connections.is_empty() {
            self.error = Some("没有可导出的连接".into());
        } else {
            self.export_dialog = Some(super::export::Dialog::default());
        }
    }
}
