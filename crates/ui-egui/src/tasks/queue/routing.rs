//! Factory/supervisor for per-connection actors and stateless one-shot requests.
use super::{Completion, Queued, actor, route, stopped};
use std::collections::HashMap;
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};
use tomato_services::commands::{Command, Output};
struct Worker {
    sender: mpsc::Sender<Queued>,
    last_task: u64,
}
fn key(command: &Command) -> Option<String> {
    match command {
        Command::InputShare { .. } => Some("input-share".into()),
        Command::Development { path, .. } => Some(format!("development:{}", path.display())),
        Command::Mongo { connection, .. } => Some(connection.id.clone()),
        Command::Redis { connection, .. } | Command::RedisData { connection, .. } => {
            Some(format!("redis:{}", connection.id))
        }
        Command::Hosts { config_path, .. } => Some(format!("hosts:{}", config_path.display())),
        Command::BackupFiles { root, .. } => Some(format!("backup-files:{}", root.display())),
        Command::NamedQueries { root, .. } => Some(format!("named-queries:{}", root.display())),
        Command::DatabaseAdmin { connection, .. }
        | Command::BackupObjects { connection, .. }
        | Command::BackupDatabase { connection, .. }
        | Command::Sql { connection, .. }
        | Command::SqlScript { connection, .. }
        | Command::SaveQueryRows { connection, .. }
        | Command::Database { connection, .. } => Some(connection.id.clone()),
        Command::Disconnect { id } | Command::DisconnectDatabase { id, .. } => Some(id.clone()),
        Command::Trae { path, .. } => Some(format!("trae:{}", path.display())),
        Command::JsonHistory { path, .. } => Some(format!("json-history:{}", path.display())),
        Command::ObjectTree { connection, .. } => Some(format!("object-tree:{}", connection.id)),
        _ => None,
    }
}
pub(super) async fn run(
    mut commands: mpsc::Receiver<Queued>,
    control: watch::Sender<bool>,
    context: actor::Context,
) {
    let mut stop = control.subscribe();
    let mut workers = HashMap::<String, Worker>::new();
    let (retired, mut retirements) = mpsc::channel::<actor::Retired>(64);
    let mut tasks = JoinSet::new();
    let routing = async {
        loop {
            let job = tokio::select! {
                Some(done) = retirements.recv() => {
                    // A disconnect is complete, and no later command has been
                    // accepted by this actor. Dropping its sender now releases
                    // the idle Engine without racing a newer actor/request.
                    if workers.get(&done.key).is_some_and(|worker| worker.last_task == done.task) {
                        workers.remove(&done.key);
                    }
                    continue;
                }
                job = commands.recv() => job,
            };
            let Some(job) = job else { break };
            // Finished one-shot tasks must not accumulate in idle sessions.
            while tasks.try_join_next().is_some() {}
            if let Some(key) = key(&job.command) {
                if matches!(job.command, Command::Disconnect { .. }) {
                    if let Some(worker) = workers.get_mut(&key) {
                        let task = job.id;
                        if route(&worker.sender, job, &context.completed, &context.repaint).await {
                            worker.last_task = task;
                        }
                    } else {
                        let task = job.id;
                        // Even an already-disconnected connection must release
                        // admission before waking deferred metadata reads.
                        drop(job);
                        let _ = context
                            .completed
                            .send(Completion {
                                id: task,
                                result: Ok(Output::Text("已断开数据库连接".into())),
                            })
                            .await;
                        context.repaint.request_repaint();
                    }
                    continue;
                }
                let id = key.clone();
                let worker = workers.entry(key).or_insert_with(|| {
                    let (send, receive) = mpsc::channel(64);
                    tasks.spawn(actor::session(
                        id,
                        receive,
                        control.subscribe(),
                        context.clone(),
                        retired.clone(),
                    ));
                    Worker {
                        sender: send,
                        last_task: 0,
                    }
                });
                let task = job.id;
                if route(&worker.sender, job, &context.completed, &context.repaint).await {
                    worker.last_task = task;
                }
            } else {
                tasks.spawn(actor::once(job, control.subscribe(), context.clone()));
            }
        }
    };
    tokio::select! {
        biased;
        _=stopped(&mut stop)=>{},
        _=routing=>{},
    }
    control.send_replace(true);
    drop(workers);
    commands.close();
    while commands.try_recv().is_ok() {}
    while tasks.join_next().await.is_some() {}
}
