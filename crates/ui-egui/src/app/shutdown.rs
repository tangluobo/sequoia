//! Close is a state transition, not a blocking wait in eframe's on_exit callback.
use super::*;
use std::time::Duration;

pub(super) struct Shutdown {
    task: tokio::task::JoinHandle<()>,
    pub(super) rdp_diagnostics: Option<serde_json::Value>,
}
impl TomatoApp {
    pub(super) fn closing(&self) -> bool {
        self.shutdown.is_some()
    }

    /// Also used as an idempotent best-effort fallback for forced native exits.
    pub fn begin_shutdown(&mut self) {
        if self.shutdown.is_some() {
            return;
        }
        let rdp_diagnostics = self.authorized_rdp_diagnostics();
        self._ddns = None;
        self.certificate_renewals = None;
        self.cancel_markdown_print();
        self.export_dialog = None;
        self.import_dialog = None;
        self.import_reader = None;
        self.input_share = None;
        for tab in &self.tabs {
            if let Panel::Cloud { view, .. } = &tab.panel
                && let Some(control) = &view.control
            {
                control.cancel();
            }
        }
        if let Some(draft) = &self.database_admin {
            draft.release(&self.context);
        }
        if let Some(draft) = &self.table_transfer {
            draft.release(&self.context);
        }
        if let Some(control) = self.table_paste.take() {
            control.cancel();
        }
        if let Some(draft) = &self.backup_dialog {
            draft.release(&self.context);
        }
        let mut jobs = Vec::new();
        let mut terminals = Vec::new();
        for host in self.ssh_hosts.values_mut() {
            if let Some(job) = host.job.take() {
                terminals.push(job.terminal);
            }
        }
        // A desktop session's Drop only signals its workers. Native clipboard
        // windows now have their own STA, including creation and destruction.
        for tab in self.tabs.drain(..) {
            if let Panel::Terminal {
                session, mut view, ..
            } = tab.panel
            {
                if let Some(workspace) = &mut view.ssh {
                    workspace.cancel_images();
                    if let Some(owner) = &workspace.object_copy {
                        jobs.push(owner.0.clone());
                    }
                    if let Some(upload) = &workspace.upload {
                        jobs.push(upload.owner.0.clone());
                    }
                }
                terminals.push(session);
            }
        }
        for job in &jobs {
            job.cancel();
        }
        self.pending.clear();
        self.collaboration.take();
        self.active = None;
        let mut development = self.development.take();
        let export = self.data_export.take().and_then(|mut draft| {
            draft.control.cancel();
            draft.task.take()
        });
        let import = self.data_import.take().and_then(|mut draft| {
            draft.control.cancel();
            draft.task.take()
        });
        let commands = self.queue.shutdown();
        let Some(runtime) = &self.runtime else {
            return;
        };
        let context = self.context.clone();
        let task = runtime.spawn(async move {
            let started = tokio::time::Instant::now();
            let mut sessions = tokio::task::JoinSet::new();
            for terminal in terminals {
                sessions.spawn(terminal.shutdown());
            }
            // Stop command actors before observing asynchronous rollback counters.
            // Their network close grace runs in parallel and is bounded per actor.
            let _ =
                tokio::time::timeout_at(started + Duration::from_secs(3), commands.wait()).await;
            let file_cleanup = async {
                // Aborting actors first makes their RAII upload guards visible
                // before the cleanup counters are observed.
                while sessions.join_next().await.is_some() {}
                for job in jobs {
                    job.wait_cleanup().await;
                }
                tomato_services::files::storage::wait_upload_cleanup().await;
            };
            let development_cleanup = async {
                if let Some(development) = &mut development {
                    development.shutdown().await;
                }
            };
            // Independent resources are cancelled together, not one tab at a
            // time. Keep the existing certificate/DNS safety grace, but pump
            // the native UI throughout it. File/ADB cleanup has one 6 s bound.
            tokio::join!(
                async {
                    if let Some(import) = import {
                        let _ =
                            tokio::time::timeout_at(started + Duration::from_secs(6), import).await;
                    }
                },
                async {
                    if let Some(export) = export {
                        let _ =
                            tokio::time::timeout_at(started + Duration::from_secs(6), export).await;
                    }
                },
                async {
                    let _ = tokio::time::timeout_at(started + Duration::from_secs(6), file_cleanup)
                        .await;
                },
                async {
                    let _ = tokio::time::timeout_at(
                        started + Duration::from_secs(6),
                        development_cleanup,
                    )
                    .await;
                },
                async {
                    let _ = tokio::time::timeout_at(
                        started + Duration::from_secs(6),
                        tomato_services::remote_desktop::wait_adb_cleanup(),
                    )
                    .await;
                },
                async {
                    let _ = tokio::time::timeout_at(
                        started + Duration::from_secs(2),
                        tomato_services::remote_desktop::wait_rdp_cleanup(),
                    )
                    .await;
                },
                async {
                    let _ = tokio::time::timeout_at(
                        started + Duration::from_secs(40),
                        tomato_services::cloud::certificate::wait_cleanup(),
                    )
                    .await;
                },
            );
            context.request_repaint_of(egui::ViewportId::ROOT);
        });
        self.shutdown = Some(Shutdown {
            task,
            rdp_diagnostics,
        });
    }
    pub(super) fn poll_shutdown(&self, ctx: &egui::Context) {
        let Some(shutdown) = &self.shutdown else {
            return;
        };
        if shutdown.task.is_finished() {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        } else {
            if ctx.input(|i| i.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            // Covers cancellation completion on a hidden/minimized native tree
            // as well as the tiny gap between repaint and JoinHandle completion.
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    #[cfg(test)]
    pub fn shutdown_file_transfers(&mut self) {
        self.begin_shutdown();
        if let Some(shutdown) = self.shutdown.take()
            && let Some(runtime) = &self.runtime
        {
            let _ = runtime.block_on(shutdown.task);
        }
    }
}

#[cfg(test)]
mod tests;
