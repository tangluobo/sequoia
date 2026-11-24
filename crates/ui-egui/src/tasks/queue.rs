//! Bounded command bus with serial database/history actors and shared admission limits.
use std::sync::Arc;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch},
    task::JoinHandle,
};
use tomato_services::commands::{Command, CommandHandler, Engine, Output};
mod actor;
mod routing;
pub struct Completion {
    pub id: u64,
    pub result: anyhow::Result<Output>,
}
struct Queued {
    id: u64,
    command: Command,
    _reservation: OwnedSemaphorePermit,
    _slot: OwnedSemaphorePermit,
}
pub struct TaskQueue {
    #[cfg(test)]
    test_completed: mpsc::Sender<Completion>,
    sender: mpsc::Sender<Queued>,
    receiver: mpsc::Receiver<Completion>,
    budget: Arc<Semaphore>,
    slots: Arc<Semaphore>,
    next_id: u64,
    stopping: watch::Sender<bool>,
    dispatcher: Option<JoinHandle<()>>,
}
pub struct QueueShutdown {
    dispatcher: Option<JoinHandle<()>>,
    receiver: mpsc::Receiver<Completion>,
}
impl QueueShutdown {
    pub async fn wait(self) {
        // Free buffered results off the UI thread and unblock full deliveries.
        drop(self.receiver);
        if let Some(dispatcher) = self.dispatcher {
            let _ = dispatcher.await;
        }
    }
}
impl TaskQueue {
    pub fn new(runtime: &tokio::runtime::Runtime, repaint: egui::Context) -> Self {
        let (sender, commands) = mpsc::channel::<Queued>(64);
        let (completed, receiver) = mpsc::channel::<Completion>(8);
        #[cfg(test)]
        let test_completed = completed.clone();
        let budget = Arc::new(Semaphore::new(32 * 1024)); // KiB, including requests already routed to actors.
        let (stopping, _) = watch::channel(false);
        let dispatcher = runtime.spawn(routing::run(
            commands,
            stopping.clone(),
            actor::Context {
                completed,
                concurrency: Arc::new(Semaphore::new(4)),
                repaint,
            },
        ));
        Self {
            #[cfg(test)]
            test_completed,
            sender,
            receiver,
            budget,
            slots: Arc::new(Semaphore::new(64)),
            next_id: 0,
            stopping,
            dispatcher: Some(dispatcher),
        }
    }
    #[cfg(test)]
    pub(crate) fn deliver_for_test(&self, id: u64, result: anyhow::Result<Output>) {
        self.test_completed
            .try_send(Completion { id, result })
            .expect("bounded test completion delivery");
    }
    #[cfg(test)]
    pub(crate) fn hold_admission_for_test(&self) -> OwnedSemaphorePermit {
        self.budget
            .clone()
            .try_acquire_many_owned(32 * 1024)
            .expect("test queue must be idle")
    }
    pub fn submit(&mut self, command: Command) -> anyhow::Result<u64> {
        anyhow::ensure!(!*self.stopping.borrow(), "后台任务正在关闭");
        let slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| anyhow::anyhow!("后台任务已达64项，请等待当前任务完成"))?;
        let reservation = self
            .budget
            .clone()
            .try_acquire_many_owned(queued_units(&command))
            .map_err(|_| anyhow::anyhow!("后台任务载荷已达32 MiB，请等待当前任务完成"))?;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("任务编号溢出"))?;
        self.sender
            .try_send(Queued {
                id: self.next_id,
                command,
                _reservation: reservation,
                _slot: slot,
            })
            .map_err(|_| anyhow::anyhow!("后台任务队列已满或已退出"))?;
        Ok(self.next_id)
    }
    /// UI-owned submissions are serial; workers only release these permits.
    /// A background read can wait for completion events without failing the view.
    pub(crate) fn can_submit(&self, command: &Command) -> bool {
        !*self.stopping.borrow()
            && !self.sender.is_closed()
            && self.slots.available_permits() > 0
            && self.budget.available_permits() >= queued_units(command) as usize
    }
    pub fn poll(&mut self) -> impl Iterator<Item = Completion> + '_ {
        std::iter::from_fn(|| self.receiver.try_recv().ok())
    }
    pub fn shutdown(&mut self) -> QueueShutdown {
        self.stopping.send_replace(true);
        let (_closed, receiver) = mpsc::channel(1);
        QueueShutdown {
            dispatcher: self.dispatcher.take(),
            receiver: std::mem::replace(&mut self.receiver, receiver),
        }
    }
}
async fn stopped(stop: &mut watch::Receiver<bool>) {
    if *stop.borrow_and_update() {
        return;
    }
    while stop.changed().await.is_ok() {
        if *stop.borrow_and_update() {
            return;
        }
    }
}
#[cfg(test)]
mod disconnect_tests;
#[cfg(test)]
mod shutdown_tests;
async fn route(
    worker: &mpsc::Sender<Queued>,
    job: Queued,
    completed: &mpsc::Sender<Completion>,
    repaint: &egui::Context,
) -> bool {
    if let Err(error) = worker.try_send(job) {
        let job = error.into_inner();
        let id = job.id;
        drop(job);
        let _ = completed
            .send(Completion {
                id,
                result: Err(anyhow::anyhow!("会话操作队列已满或已关闭")),
            })
            .await;
        repaint.request_repaint();
        false
    } else {
        true
    }
}
async fn run_job(
    job: Queued,
    engine: &mut Engine,
    completed: &mpsc::Sender<Completion>,
    concurrency: &Arc<Semaphore>,
    repaint: &egui::Context,
) -> bool {
    let Queued {
        id,
        command,
        _reservation,
        _slot,
    } = job;
    let Ok(_permit) = concurrency.acquire().await else {
        return false;
    };
    let result = engine.execute(command).await;
    let delivered = completed.send(Completion { id, result }).await.is_ok();
    // A completion-triggered UI frame may immediately enqueue deferred reads.
    // Release admission before waking it; retain backpressure until send accepts.
    drop(_reservation);
    drop(_slot);
    drop(_permit);
    repaint.request_repaint();
    delivered
}
fn queued_units(command: &Command) -> u32 {
    let mut bytes = match command {
        Command::ConnectionImport { request, .. } => request.retained_bytes(),
        Command::ConvertTool { request, .. } => request.retained_bytes(),
        Command::Music { request, .. } => request.retained_bytes(),
        Command::Development { action, .. } => {
            serde_json::to_vec(action).map_or(32 * 1024 * 1024 + 1, |v| v.len())
        }
        Command::TransferTables {
            request, accepted, ..
        } => {
            let mut retained = connection_bytes(&request.source)
                .saturating_add(connection_bytes(&request.target))
                .saturating_add(request.source_schema.capacity())
                .saturating_add(request.target_schema.capacity());
            retained = retained
                .saturating_add(request.tables.capacity() * std::mem::size_of::<String>())
                .saturating_add(request.tables.iter().map(String::capacity).sum::<usize>());
            if let Some(plan) = accepted {
                retained = retained.saturating_add(
                    plan.tables.capacity()
                        * std::mem::size_of::<tomato_core::table_transfer::TablePlan>(),
                );
                for table in &plan.tables {
                    retained = retained.saturating_add(
                        table.source.schema.capacity()
                            + table.source.name.capacity()
                            + table.target.schema.capacity()
                            + table.target.name.capacity()
                            + table.stamp.capacity()
                            + table.sql.capacity() * std::mem::size_of::<String>()
                            + table.sql.iter().map(String::capacity).sum::<usize>(),
                    );
                }
            }
            retained
        }
        Command::BackupObjects {
            namespace, schema, ..
        } => namespace
            .capacity()
            .saturating_add(schema.as_ref().map_or(0, String::capacity)),
        Command::BackupDatabase {
            root, operation, ..
        } => root.capacity().saturating_add(operation.retained_bytes()),
        Command::Cloud { action, .. } => {
            serde_json::to_vec(action).map_or(32 * 1024 * 1024 + 1, |v| v.len())
        }
        Command::MessageQueue { action, .. } => {
            serde_json::to_vec(action).map_or(32 * 1024 * 1024 + 1, |v| v.len())
        }
        Command::Certificate { request, .. } => {
            serde_json::to_vec(request).map_or(32 * 1024 * 1024 + 1, |v| v.len())
        }
        Command::AiSql { request } => request.retained_bytes(),
        Command::ExtractResources { options } => options
            .categories
            .iter()
            .chain(options.extensions.iter())
            .fold(1024usize, |sum, s| sum.saturating_add(s.capacity())),
        Command::Trae { action, .. } => action.queued_bytes(),
        Command::Mongo { action, .. } => action.queued_bytes(),
        Command::RedisData { action, .. } => action.queued_bytes(),
        Command::Hosts { action, .. } => action.queued_bytes(),
        Command::BackupFiles { root, request } => {
            root.capacity().saturating_add(request.retained_bytes())
        }
        Command::NamedQueries { root, request } => {
            root.capacity().saturating_add(request.queued_bytes())
        }
        Command::Sql { sql, .. } | Command::SqlScript { sql, .. } => sql.capacity(),
        Command::SaveQueryRows {
            editing, changes, ..
        } => changes
            .iter()
            .fold(editing.retained_bytes(), |bytes, change| {
                bytes.saturating_add(change.queued_bytes())
            }),
        Command::ConnectionExport { prepared, .. } => prepared.connections.iter().fold(
            prepared
                .connections
                .capacity()
                .saturating_mul(std::mem::size_of::<tomato_core::Connection>()),
            |sum, c| sum.saturating_add(connection_bytes(c)),
        ),
        Command::Json { text, .. } | Command::WriteText { text, .. } => text.capacity(),
        Command::Markdown { source } => source.capacity(),
        Command::MarkdownImage { document, source } => {
            document.capacity().saturating_add(source.capacity())
        }
        Command::MarkdownHtml {
            source,
            title,
            path,
        } => source
            .capacity()
            .saturating_add(title.capacity())
            .saturating_add(path.capacity()),
        Command::ObjectTree { action, .. } => action.queued_bytes(),
        Command::FileDocument { action, .. } => action.queued_bytes(),
        Command::LocalTree { action, .. } => action.queued_bytes(),
        Command::JsonHistory {
            action: tomato_services::json_history::Action::Format { json },
            ..
        } => json.capacity(),
        Command::ExportCsv { columns, rows, .. } => columns
            .iter()
            .chain(rows.iter().flatten())
            .fold(0usize, |sum, v| sum.saturating_add(v.capacity())),
        Command::Redis { args, .. } => args
            .iter()
            .fold(0usize, |sum, v| sum.saturating_add(v.capacity())),
        Command::Database {
            action: tomato_core::workbench::DatabaseAction::ObjectFields { schema, catalog },
            ..
        } => schema.capacity().saturating_add(catalog.retained_bytes()),
        Command::Database {
            action: tomato_core::workbench::DatabaseAction::DeleteRows { rows, .. },
            ..
        } => rows.iter().fold(0usize, |sum, (_, row)| {
            row.iter()
                .fold(sum, |sum, cell| sum.saturating_add(cell_bytes(cell)))
        }),
        Command::Database {
            action: tomato_core::workbench::DatabaseAction::Apply { changes, .. },
            ..
        } => {
            use tomato_core::workbench::RowChange;
            changes.iter().fold(0usize, |sum, change| {
                sum.saturating_add(match change {
                    RowChange::Insert { values } => values
                        .iter()
                        .flatten()
                        .fold(0usize, |sum, c| sum.saturating_add(cell_bytes(c))),
                    RowChange::Update { original, values } => original
                        .iter()
                        .chain(values)
                        .fold(0usize, |sum, c| sum.saturating_add(cell_bytes(c))),
                    RowChange::Delete { original } => original
                        .iter()
                        .fold(0usize, |sum, c| sum.saturating_add(cell_bytes(c))),
                })
            })
        }
        _ => 0,
    };
    if let Command::DatabaseAdmin { connection, .. }
    | Command::Mongo { connection, .. }
    | Command::RedisData { connection, .. }
    | Command::Redis { connection, .. }
    | Command::ObjectTree { connection, .. }
    | Command::Sql { connection, .. }
    | Command::SqlScript { connection, .. }
    | Command::Database { connection, .. }
    | Command::SaveQueryRows { connection, .. }
    | Command::BackupObjects { connection, .. }
    | Command::BackupDatabase { connection, .. } = command
    {
        bytes = bytes.saturating_add(connection_bytes(connection));
    }
    u32::try_from(bytes.div_ceil(1024).max(1)).unwrap_or(u32::MAX)
}
fn cell_bytes(cell: &tomato_core::workbench::Cell) -> usize {
    use tomato_core::workbench::Cell;
    std::mem::size_of::<Cell>()
        + match cell {
            Cell::Text(text) => text.capacity(),
            Cell::Binary(bytes) => bytes.capacity(),
            _ => 0,
        }
}

fn connection_bytes(connection: &tomato_core::Connection) -> usize {
    connection.retained_bytes()
}
