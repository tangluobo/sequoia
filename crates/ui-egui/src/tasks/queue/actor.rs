//! A session actor serializes requests; cancellation and its final close are bounded.
use super::{Completion, Queued, run_job, stopped};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Semaphore, mpsc, watch};
use tomato_services::commands::{Command, CommandHandler, Engine};
#[derive(Clone)]
pub(super) struct Context {
    pub completed: mpsc::Sender<Completion>,
    pub concurrency: Arc<Semaphore>,
    pub repaint: egui::Context,
}
pub(super) struct Retired {
    pub key: String,
    pub task: u64,
}
impl Context {
    async fn execute(
        &self,
        job: Queued,
        engine: &mut Engine,
        stop: &mut watch::Receiver<bool>,
    ) -> bool {
        tokio::select! {
            biased;
            _=stopped(stop)=>false,
            delivered=run_job(job,engine,&self.completed,&self.concurrency,&self.repaint)=>delivered,
        }
    }
}
pub(super) async fn session(
    id: String,
    mut receive: mpsc::Receiver<Queued>,
    mut stop: watch::Receiver<bool>,
    context: Context,
    retired: mpsc::Sender<Retired>,
) {
    let mut engine = Engine::default();
    loop {
        let job = tokio::select! {
            biased;
            _=stopped(&mut stop)=>break,
            job=receive.recv()=>job,
        };
        let Some(job) = job else { break };
        let disconnect = matches!(job.command, Command::Disconnect { .. }).then_some(job.id);
        if !context.execute(job, &mut engine, &mut stop).await {
            break;
        }
        if let Some(task) = disconnect {
            // Only the supervisor can retire an idle actor: later requests may
            // already be queued behind this disconnect and must remain serial.
            let _ = retired
                .send(Retired {
                    key: id.clone(),
                    task,
                })
                .await;
        }
    }
    // No admission permit or completion delivery can delay protocol shutdown.
    // Actors close their independent Engine sessions concurrently.
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        engine.execute(Command::Disconnect { id }),
    )
    .await;
}
pub(super) async fn once(job: Queued, mut stop: watch::Receiver<bool>, context: Context) {
    let _ = context
        .execute(job, &mut Engine::default(), &mut stop)
        .await;
}
