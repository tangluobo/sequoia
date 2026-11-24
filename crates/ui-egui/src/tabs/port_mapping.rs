use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::mpsc,
};
use tomato_services::terminal::ssh::forwarding::{Forward, Mapping, Repository};
pub struct Editor {
    pub mapping: Mapping,
    pub local: String,
    pub remote: String,
    pub editing: Option<usize>,
    pub focus: bool,
}
pub struct State {
    pub mappings: Vec<Mapping>,
    pub selected: BTreeSet<usize>,
    pub delete: Option<Vec<String>>,
    pub reverse: std::collections::VecDeque<usize>,
    pub anchor: Option<usize>,
    generation: u64,
    pub editor: Option<Editor>,
    pub status: String,
    repository: Option<Repository>,
    pub runtime: Option<tokio::runtime::Handle>,
    pub known_hosts: PathBuf,
    pub active: HashMap<String, Forward>,
    pending: HashMap<String, (u64, tokio::task::JoinHandle<()>)>,
    sender: mpsc::SyncSender<(u64, String, anyhow::Result<Forward>)>,
    receiver: mpsc::Receiver<(u64, String, anyhow::Result<Forward>)>,
}
impl State {
    pub fn new(
        path: PathBuf,
        runtime: Option<tokio::runtime::Handle>,
        known_hosts: PathBuf,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel(32);
        let (repository, mappings, status) = match Repository::load(path) {
            Ok((r, m)) => (Some(r), m, "就绪".into()),
            Err(e) => (None, vec![], format!("加载失败：{e:#}")),
        };
        Self {
            mappings,
            delete: None,
            reverse: Default::default(),
            anchor: None,
            generation: 0,
            repository,
            status,
            runtime,
            known_hosts,
            sender,
            receiver,
            selected: Default::default(),
            editor: None,
            active: Default::default(),
            pending: Default::default(),
        }
    }
    pub fn poll(&mut self) {
        while let Ok((generation, id, result)) = self.receiver.try_recv() {
            if !self
                .pending
                .get(&id)
                .is_some_and(|(ticket, _)| *ticket == generation)
            {
                continue;
            }
            self.pending.remove(&id);
            match result {
                Ok(forward) => {
                    self.active.insert(id, forward);
                    self.status = "映射已启用".into();
                }
                Err(e) => self.status = format!("启动失败：{e:#}"),
            }
        }
        self.active.retain(|_, forward| forward.is_active());
    }
    pub fn busy(&self, id: &str) -> bool {
        self.pending.contains_key(id)
    }
    pub fn stop(&mut self, id: &str) {
        if let Some((_, task)) = self.pending.remove(id) {
            task.abort();
        }
        self.active.remove(id);
        self.status = "映射已停止".into();
    }
    pub fn request_start(
        &mut self,
        index: usize,
        hosts: &[tomato_core::Connection],
        ctx: &egui::Context,
    ) {
        let mapping = &self.mappings[index];
        if self.busy(&mapping.id) || self.active.contains_key(&mapping.id) {
            return;
        }
        if mapping.direction == "R" {
            if !self.reverse.contains(&index) {
                self.reverse.push_back(index);
            }
        } else {
            self.start(index, hosts, ctx);
        }
    }
    pub fn start(&mut self, index: usize, hosts: &[tomato_core::Connection], ctx: &egui::Context) {
        if self.pending.len() >= 16 {
            self.status = "最多同时连接 16 条映射".into();
            return;
        }
        let mapping = self.mappings[index].clone();
        if self.busy(&mapping.id) || self.active.contains_key(&mapping.id) {
            return;
        }
        let Some(host) = hosts.iter().find(|h| h.id == mapping.ssh_host_id).cloned() else {
            self.status = "引用的 SSH 主机不存在".into();
            return;
        };
        let Some(runtime) = &self.runtime else {
            self.status = "后台运行时不可用".into();
            return;
        };
        let known = self.known_hosts.clone();
        let sender = self.sender.clone();
        let context = ctx.clone();
        let id = mapping.id.clone();
        self.generation += 1;
        let generation = self.generation;
        let task = runtime.spawn(async move {
            let repaint = context.clone();
            let result = Forward::start(mapping, host, known, move || repaint.request_repaint());
            let _ = sender.try_send((generation, id, result));
            context.request_repaint();
        });
        self.pending
            .insert(self.mappings[index].id.clone(), (generation, task));
    }
    pub fn save(&mut self, mut mappings: Vec<Mapping>) -> bool {
        let result = self
            .repository
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("配置加载失败，保存已禁用"))
            .and_then(|r| r.save(&mappings));
        match result {
            Ok(()) => {
                std::mem::swap(&mut mappings, &mut self.mappings);
                true
            }
            Err(e) => {
                self.status = format!("保存失败：{e:#}");
                false
            }
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        for (_, (_, task)) in self.pending.drain() {
            task.abort();
        }
    }
}
