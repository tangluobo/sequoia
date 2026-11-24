//! Original development workspace; views emit typed background use cases.
mod code;
mod log;
mod preview;
mod settings;
mod sidebar;
#[cfg(test)]
mod tests;
mod tree;
mod view;
use crate::{icons::Icons, tabs::text_state::Editor, tasks::TaskQueue};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
    time::{Duration, Instant},
};
use tomato_core::development::{Config, Project};
use tomato_services::{
    commands::{Command, Output},
    development::{self as service, Action, RunItem},
};
struct Document {
    path: PathBuf,
    text: String,
    saved: String,
    revision: String,
    editor: Editor,
    read_only: bool,
    changes: Vec<tomato_core::development::LineChange>,
    target_line: Option<usize>,
}
enum Request {
    Ack,
    Normal(u64),
    Git(u64),
    Job {
        root: PathBuf,
        dependencies: bool,
        generation: u64,
    },
    Config {
        select: Option<String>,
        launch: Option<Box<Action>>,
    },
    Directory {
        epoch: u64,
        save: bool,
    },
    Inspect(u64),
    List(u64),
    Read {
        generation: u64,
        serial: u64,
    },
    Diff {
        generation: u64,
        serial: u64,
    },
    Write {
        generation: u64,
        text: String,
        next: Option<Navigation>,
    },
    Versions(u64),
    Log {
        dependencies: bool,
        epoch: u64,
    },
    Detect {
        kind: String,
        path: PathBuf,
        epoch: u64,
    },
    ValidateEntry {
        epoch: u64,
    },
}
#[derive(Clone)]
enum Navigation {
    Project(usize),
    File(PathBuf),
}
enum Confirm {
    Navigate(Navigation),
    Rollback(Vec<String>),
    Push,
}
pub(crate) struct State {
    context: egui::Context,
    queue: TaskQueue,
    path: PathBuf,
    pending: HashMap<u64, Request>,
    config: Config,
    revision: Option<String>,
    loaded: bool,
    generation: u64,
    selected: Option<usize>,
    file_serial: u64,
    tab: usize,
    settings: Option<settings::Draft>,
    add: Option<settings::DirectoryDraft>,
    confirm: Option<Confirm>,
    entries: BTreeMap<PathBuf, Vec<service::Entry>>,
    expanded: BTreeSet<PathBuf>,
    overview: Option<service::Overview>,
    selected_run: Option<usize>,
    document: Option<Document>,
    file_selection: Option<PathBuf>,
    versions: Option<service::Versions>,
    selected_changes: BTreeSet<String>,
    version_nodes: Vec<tree::Node>,
    version_open: BTreeSet<String>,
    realtime: bool,
    next_scan: Instant,
    commit_message: String,
    push_remote: String,
    push_branch: String,
    logs: HashMap<PathBuf, LogView>,
    dependency_logs: HashMap<PathBuf, LogView>,
    last_poll: Instant,
    log_pending: BTreeSet<bool>,
    status: String,
    notice: Option<String>,
    error: Option<String>,
    divider: [f32; 2],
}
#[derive(Default)]
struct LogView {
    epoch: u64,
    text: String,
    offset: u64,
    running: bool,
    dependencies: bool,
    exit_code: Option<i32>,
    editor: Editor,
    dependency_rows: Vec<(usize, String)>,
    dependency_open: BTreeSet<usize>,
}
impl State {
    #[cfg(test)]
    pub(crate) fn probe(&self) -> Probe<'_> {
        let log = self.root().as_ref().and_then(|root| self.logs.get(root));
        Probe {
            loaded: self.loaded,
            ready: self.overview.is_some(),
            files_loaded: !self.entries.is_empty(),
            document_path: self.document.as_ref().map(|d| d.path.as_path()),
            modal: self.confirm.is_some(),
            settings_open: self.settings.is_some(),
            add_open: self.add.is_some(),
            error: self.error.as_deref(),
            log: log.map(|l| l.text.as_str()),
            running: log.is_some_and(|l| l.running),
        }
    }
    pub fn new(runtime: &tokio::runtime::Runtime, ctx: &egui::Context, path: PathBuf) -> Self {
        let mut state = Self {
            context: ctx.clone(),
            queue: TaskQueue::new(runtime, ctx.clone()),
            path,
            pending: Default::default(),
            config: Default::default(),
            revision: None,
            loaded: false,
            generation: 0,
            file_serial: 0,
            selected: None,
            tab: 0,
            settings: None,
            add: None,
            confirm: None,
            entries: Default::default(),
            expanded: Default::default(),
            overview: None,
            selected_run: None,
            document: None,
            file_selection: None,
            versions: None,
            selected_changes: Default::default(),
            version_nodes: vec![],
            version_open: BTreeSet::new(),
            realtime: false,
            next_scan: Instant::now(),
            commit_message: String::new(),
            push_remote: "origin".into(),
            push_branch: "main".into(),
            logs: Default::default(),
            dependency_logs: Default::default(),
            last_poll: Instant::now(),
            log_pending: Default::default(),
            status: "正在加载开发配置...".into(),
            notice: None,
            error: None,
            divider: [0.38; 2],
        };
        state.submit(
            Action::Load,
            Request::Config {
                select: None,
                launch: None,
            },
        );
        state
    }
    fn root(&self) -> Option<PathBuf> {
        self.project().map(|p| PathBuf::from(&p.path))
    }
    fn project(&self) -> Option<&Project> {
        self.selected.and_then(|i| self.config.projects.get(i))
    }
    fn run(&self) -> Option<&RunItem> {
        self.overview.as_ref()?.runs.get(self.selected_run?)
    }
    fn config_busy(&self) -> bool {
        self.pending
            .values()
            .any(|r| matches!(r, Request::Config { .. }))
    }
    pub fn dirty(&self) -> bool {
        self.document.as_ref().is_some_and(|d| d.text != d.saved)
    }
    pub(crate) fn modal_open(&self) -> bool {
        self.settings.is_some()
            || self.add.is_some()
            || self.confirm.is_some()
            || self.error.is_some()
            || self.notice.is_some()
    }
    fn submit(&mut self, action: Action, request: Request) {
        match self.queue.submit(Command::Development {
            path: self.path.clone(),
            action,
        }) {
            Ok(id) => {
                self.pending.insert(id, request);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn ordinary(&mut self, action: Action) {
        let request = if let Action::Launch { project, .. } | Action::Dependencies { project, .. } =
            &action
        {
            let dependencies = matches!(&action, Action::Dependencies { .. });
            let root = PathBuf::from(&project.path);
            let view = self.log_mut(root.clone(), dependencies);
            view.epoch = view.epoch.wrapping_add(1);
            view.running = true;
            Request::Job {
                root,
                dependencies,
                generation: self.generation,
            }
        } else if matches!(&action, Action::Git { .. }) {
            Request::Git(self.generation)
        } else {
            Request::Normal(self.generation)
        };
        self.submit(action, request);
    }
    fn log_mut(&mut self, root: PathBuf, dependencies: bool) -> &mut LogView {
        let map = if dependencies {
            &mut self.dependency_logs
        } else {
            &mut self.logs
        };
        if !map.contains_key(&root)
            && map.len() >= 32
            && let Some(old) = map.iter().find(|(_, v)| !v.running).map(|(p, _)| p.clone())
            && let Some(value) = map.remove(&old)
        {
            value.editor.release(&self.context);
        }
        map.entry(root).or_default()
    }
    fn save_config(&mut self, config: Config, select: Option<String>, launch: Option<Action>) {
        self.submit(
            Action::Save {
                config: Box::new(config),
                revision: self.revision.clone(),
            },
            Request::Config {
                select,
                launch: launch.map(Box::new),
            },
        );
    }
    fn navigate(&mut self, next: Navigation) {
        if self.dirty() {
            self.confirm = Some(Confirm::Navigate(next));
        } else {
            self.navigate_now(next)
        }
    }
    fn navigate_now(&mut self, next: Navigation) {
        match next {
            Navigation::Project(index) => {
                if let Some(document) = &self.document {
                    document.editor.release(&self.context);
                }
                self.selected = Some(index);
                self.generation += 1;
                self.entries.clear();
                self.expanded.clear();
                self.overview = None;
                self.selected_run = None;
                self.document = None;
                self.file_selection = None;
                self.versions = None;
                self.selected_changes.clear();
                if let Some(project) = self.project().cloned() {
                    let root = PathBuf::from(&project.path);
                    self.expanded.insert(root.clone());
                    self.submit(
                        Action::List {
                            root: root.clone(),
                            path: root.clone(),
                        },
                        Request::List(self.generation),
                    );
                    self.submit(
                        Action::Inspect { project },
                        Request::Inspect(self.generation),
                    );
                    self.refresh_versions(true);
                    self.status = format!("已选择目录：{}", root.display());
                }
            }
            Navigation::File(path) => {
                if let Some(root) = self.root() {
                    self.file_serial += 1;
                    let relative = path
                        .strip_prefix(&root)
                        .ok()
                        .map(|p| p.to_string_lossy().replace('\\', "/"));
                    let deleted = relative.as_ref().is_some_and(|path| {
                        self.versions.as_ref().is_some_and(|v| {
                            v.git
                                && v.changes
                                    .iter()
                                    .any(|c| &c.path == path && c.status == "已删除")
                        })
                    });
                    let action = if deleted {
                        Action::ReadVersion {
                            root,
                            path: relative.unwrap(),
                        }
                    } else {
                        Action::Read { root, path }
                    };
                    self.submit(
                        action,
                        Request::Read {
                            generation: self.generation,
                            serial: self.file_serial,
                        },
                    );
                }
            }
        }
    }
    fn refresh_versions(&mut self, reset: bool) {
        if let Some(root) = self.root() {
            self.next_scan = Instant::now() + Duration::from_secs(2);
            self.submit(
                Action::Versions {
                    root,
                    realtime: self.realtime,
                    reset,
                },
                Request::Versions(self.generation),
            );
        }
    }
    fn refresh_dependencies(&mut self) {
        if let Some(project) = self
            .project()
            .filter(|p| {
                (p.project_type.starts_with("MAVEN") || p.project_type.starts_with("GRADLE"))
                    && !p.jdk_id.is_empty()
                    && !p.maven_id.is_empty()
            })
            .cloned()
        {
            self.ordinary(Action::Dependencies {
                config: Box::new(self.config.clone()),
                project,
            });
        }
    }
    fn save_file(&mut self, next: Option<Navigation>) {
        if let (Some(root), Some(document)) = (self.root(), &self.document)
            && !document.read_only
        {
            let text = document.text.clone();
            self.submit(
                Action::Write {
                    root,
                    path: document.path.clone(),
                    text: text.clone(),
                    revision: document.revision.clone(),
                },
                Request::Write {
                    generation: self.generation,
                    text,
                    next,
                },
            );
        }
    }
    fn launch(&mut self) {
        let Some(mut project) = self.project().cloned() else {
            return;
        };
        let Some(item) = self.run().cloned() else {
            self.error = Some("请先选择 Java main 或 npm script".into());
            return;
        };
        project.main_class = if item.kind == "JAVA" {
            item.target.clone()
        } else {
            project.main_class
        };
        if item.kind == "JAVA" {
            project.main_source_path = item.source.to_string_lossy().into_owned();
        }
        let mut config = self.config.clone();
        config.projects[self.selected.unwrap()] = project.clone();
        let restart = self
            .logs
            .get(&PathBuf::from(&project.path))
            .is_some_and(|l| l.running);
        self.tab = 3;
        self.save_config(
            config.clone(),
            Some(project.path.clone()),
            Some(Action::Launch {
                config: Box::new(config),
                project,
                item,
                restart,
            }),
        );
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        let completed: Vec<_> = self.queue.poll().collect();
        for completion in completed {
            let Some(request) = self.pending.remove(&completion.id) else {
                continue;
            };
            if matches!(request, Request::Ack) {
                continue;
            }
            if let Request::Log { dependencies, .. } = &request {
                self.log_pending.remove(dependencies);
            }
            let output = match completion.result {
                Ok(Output::Development(output)) => output,
                Ok(_) => continue,
                Err(e) => {
                    if let Request::Job {
                        root, dependencies, ..
                    } = &request
                    {
                        self.log_mut(root.clone(), *dependencies).running = false;
                    }
                    self.error = Some(e.to_string());
                    continue;
                }
            };
            let generation = match &request {
                Request::Normal(g)
                | Request::Git(g)
                | Request::Inspect(g)
                | Request::List(g)
                | Request::Versions(g) => Some(*g),
                Request::Read { generation, .. } => Some(*generation),
                Request::Diff { generation, .. } => Some(*generation),
                Request::Job { generation, .. } => Some(*generation),
                Request::Write { generation, .. } => Some(*generation),
                _ => None,
            };
            if generation.is_some_and(|g| g != self.generation) {
                continue;
            }
            if matches!(&request,Request::Read{serial,..}|Request::Diff{serial,..}if *serial!=self.file_serial)
            {
                continue;
            }
            match output {
                service::Output::Config { config, revision } => {
                    let reconfigured = self.settings.is_some();
                    let previous = self.project().map(|p| p.path.clone());
                    self.loaded = true;
                    self.config = config;
                    self.selected = previous
                        .as_ref()
                        .and_then(|path| self.config.projects.iter().position(|p| &p.path == path));
                    if previous.is_some() && self.selected.is_none() {
                        if let Some(d) = &self.document {
                            d.editor.release(ctx);
                        }
                        self.document = None;
                        self.overview = None;
                        self.entries.clear();
                        self.versions = None;
                        self.generation += 1;
                    }
                    self.revision = revision;
                    self.settings = None;
                    self.add = None;
                    self.status = "开发配置已加载".into();
                    if reconfigured {
                        self.refresh_dependencies();
                    }
                    if let Request::Config { select, launch } = request {
                        if let Some(action) = launch {
                            self.ordinary(*action);
                            self.last_poll = Instant::now() - Duration::from_secs(1);
                        }
                        if let Some(path) = select {
                            if let Some(index) =
                                self.config.projects.iter().position(|p| p.path == path)
                                && (self.selected != Some(index) || self.overview.is_none())
                            {
                                self.navigate_now(Navigation::Project(index));
                            }
                        } else if self
                            .selected
                            .is_some_and(|i| i >= self.config.projects.len())
                        {
                            self.selected = None;
                            self.document = None;
                            self.overview = None;
                            self.entries.clear();
                            self.versions = None;
                        }
                    }
                }
                service::Output::Overview(mut value) => {
                    if let Some(index) = self.selected {
                        let project = &mut self.config.projects[index];
                        project.project_type = value.project_type.clone();
                        self.selected_run = value
                            .runs
                            .iter()
                            .position(|r| {
                                r.kind == project.run_type && r.target == project.run_target
                            })
                            .or_else(|| (value.runs.len() == 1).then_some(0));
                        if let Some(run) = self.selected_run.and_then(|i| value.runs.get(i)) {
                            project.run_type = run.kind.clone();
                            project.run_target = run.target.clone();
                        }
                        value.profiles.extend(
                            project
                                .active_profiles
                                .iter()
                                .filter(|p| !value.profiles.contains(p))
                                .cloned()
                                .collect::<Vec<_>>(),
                        );
                        value.profiles.sort();
                        value.profiles.dedup();
                    }
                    self.overview = Some(value);
                    self.refresh_dependencies();
                }
                service::Output::Entries { path, entries } => {
                    self.entries.insert(path, entries);
                }
                service::Output::File {
                    path,
                    text,
                    revision,
                    read_only,
                } => {
                    if let Some(old) = &self.document {
                        old.editor.release(ctx);
                    }
                    if !read_only && let Some(root) = self.root() {
                        self.submit(
                            Action::Diff {
                                root,
                                path: path.clone(),
                                revision: revision.clone(),
                            },
                            Request::Diff {
                                generation: self.generation,
                                serial: self.file_serial,
                            },
                        );
                    }
                    self.document = Some(Document {
                        path,
                        saved: text.clone(),
                        text,
                        revision,
                        editor: Default::default(),
                        read_only,
                        changes: vec![],
                        target_line: None,
                    });
                }
                service::Output::Written { path, revision } => {
                    if let Some(root) = self.root() {
                        self.submit(
                            Action::Diff {
                                root,
                                path: path.clone(),
                                revision: revision.clone(),
                            },
                            Request::Diff {
                                generation: self.generation,
                                serial: self.file_serial,
                            },
                        );
                    }
                    if let Request::Write { text, next, .. } = request {
                        if let Some(document) = &mut self.document
                            && document.path == path
                        {
                            document.saved = text;
                            document.revision = revision;
                        }
                        if let Some(next) = next {
                            self.navigate_now(next)
                        }
                    }
                    self.status = "文件已保存".into();
                    self.refresh_versions(false);
                }
                service::Output::Versions(value) => {
                    self.selected_changes
                        .retain(|p| value.changes.iter().any(|c| &c.path == p));
                    self.version_nodes = tree::build(&value.changes, &self.selected_changes);
                    self.versions = Some(value);
                }
                service::Output::Diff {
                    path,
                    revision,
                    changes,
                } => {
                    if let Some(document) = &mut self.document
                        && document.path == path
                        && document.revision == revision
                    {
                        document.changes = changes;
                    }
                }
                service::Output::Log(log) => {
                    let acknowledge = (!log.running && !log.text.is_empty())
                        .then(|| (log.root.clone(), log.offset, log.dependencies));
                    let view = self.log_mut(log.root, log.dependencies);
                    if let Request::Log { epoch, .. } = &request
                        && *epoch != view.epoch
                    {
                        continue;
                    }
                    if log.reset || log.offset < view.offset {
                        view.text.clear();
                        view.editor.reset();
                    }
                    if tomato_core::development::log::append(&mut view.text, &log.text) > 0 {
                        view.editor.reset();
                    }
                    view.offset = log.offset;
                    view.running = log.running;
                    view.dependencies = log.dependencies;
                    view.exit_code = log.exit_code;
                    if log.dependencies && !log.running {
                        view.dependency_rows =
                            tomato_core::development::dependency_lines(&view.text);
                    }
                    if let Some((root, after, dependencies)) = acknowledge
                        && let Ok(id) = self.queue.submit(Command::Development {
                            path: self.path.clone(),
                            action: Action::Poll {
                                root,
                                after,
                                dependencies,
                            },
                        })
                    {
                        self.pending.insert(id, Request::Ack);
                    }
                }
                service::Output::Directory {
                    valid,
                    project_type,
                } => {
                    if let Request::Directory { epoch, save } = request {
                        settings::directory_result(self, epoch, save, valid, project_type);
                    }
                }
                service::Output::EntryValidated { message } => {
                    if let Request::ValidateEntry { epoch } = request
                        && let Some(draft) = &mut self.settings
                        && draft.edit.is_some_and(|edit| edit.epoch == epoch)
                    {
                        if let Some(message) = message {
                            draft.warning = Some(message);
                            draft.warning_focus = true;
                        } else {
                            draft.accept();
                        }
                    }
                }
                service::Output::Text(text) => {
                    if let Request::Detect { kind, path, epoch } = request {
                        if let Some(draft) = &mut self.settings {
                            if draft.kind != kind
                                || draft.path != path
                                || !draft.edit.is_some_and(|edit| edit.epoch == epoch)
                            {
                                continue;
                            }
                            draft.detected(text);
                        }
                    } else {
                        self.status = text;
                        self.last_poll = Instant::now() - Duration::from_secs(1);
                        if matches!(request, Request::Git(_)) {
                            self.notice = Some(self.status.clone());
                            self.refresh_versions(false);
                        }
                    }
                }
            }
        }
        if let Some(root) = self.root() {
            if self.last_poll.elapsed() >= Duration::from_millis(150) {
                for dependencies in [false, true] {
                    let map = if dependencies {
                        &self.dependency_logs
                    } else {
                        &self.logs
                    };
                    let poll = map.get(&root).is_some_and(|l| l.running)
                        || (!dependencies && !map.contains_key(&root));
                    if !self.log_pending.contains(&dependencies) && poll {
                        let after = map.get(&root).map_or(0, |l| l.offset);
                        let epoch = map.get(&root).map_or(0, |l| l.epoch);
                        self.submit(
                            Action::Poll {
                                root: root.clone(),
                                after,
                                dependencies,
                            },
                            Request::Log {
                                dependencies,
                                epoch,
                            },
                        );
                        self.log_pending.insert(dependencies);
                    }
                }
                self.last_poll = Instant::now();
            }
            if self.realtime
                && self.tab == 1
                && Instant::now() >= self.next_scan
                && !self
                    .pending
                    .values()
                    .any(|p| matches!(p, Request::Versions(_)))
            {
                self.refresh_versions(false);
            }
            if self.logs.get(&root).is_some_and(|l| l.running)
                || self.dependency_logs.get(&root).is_some_and(|l| l.running)
                || !self.log_pending.is_empty()
            {
                ctx.request_repaint_after(Duration::from_millis(150));
            } else if self.realtime && self.tab == 1 {
                ctx.request_repaint_after(Duration::from_secs(2));
            }
        }
    }
    pub fn sidebar(&mut self, ui: &mut egui::Ui, icons: &mut Icons) {
        sidebar::show(ui, self, icons);
    }
    pub fn show(&mut self, ui: &mut egui::Ui, icons: &mut Icons) {
        self.poll(ui.ctx());
        view::show(ui, self, icons);
        settings::dialogs(ui.ctx(), self, icons);
    }
    pub(crate) async fn shutdown(&mut self) {
        let Ok(id) = self.queue.submit(Command::Development {
            path: self.path.clone(),
            action: Action::Shutdown,
        }) else {
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if self.queue.poll().any(|c| c.id == id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
#[cfg(test)]
pub(crate) struct Probe<'a> {
    pub loaded: bool,
    pub ready: bool,
    pub files_loaded: bool,
    pub document_path: Option<&'a std::path::Path>,
    pub modal: bool,
    pub settings_open: bool,
    pub add_open: bool,
    pub error: Option<&'a str>,
    pub log: Option<&'a str>,
    pub running: bool,
}
