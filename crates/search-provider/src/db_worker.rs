use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use sessions_chronicle_core::database::shell_search::{
    ShellSearchConnection, ShellSearchInterrupt, ShellSearchMetadata, build_match_expression,
    search_session_ids, subsearch_session_ids,
};

#[derive(Debug, Default)]
pub struct SearchResponse {
    pub expression: Option<String>,
    pub ids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct MetadataResponse {
    pub show_excerpts: bool,
    pub rows: Vec<Option<ShellSearchMetadata>>,
}

enum DbRequest {
    Search {
        generation: u64,
        expression: String,
        previous_results: Option<Vec<String>>,
        reply: async_channel::Sender<Vec<String>>,
    },
    Metadata {
        identifiers: Vec<String>,
        expression: Option<String>,
        reply: async_channel::Sender<MetadataResponse>,
    },
    #[cfg(test)]
    Block {
        started: async_channel::Sender<()>,
        release: async_channel::Receiver<()>,
    },
}

#[derive(Clone)]
pub struct DbWorker {
    sender: mpsc::Sender<DbRequest>,
    generation: Arc<AtomicU64>,
    next_operation: Arc<AtomicU64>,
    active_operation: Arc<AtomicU64>,
    interrupt: Arc<Mutex<Option<ShellSearchInterrupt>>>,
    #[cfg_attr(not(test), allow(dead_code))]
    connection_open_count: Arc<AtomicUsize>,
    deadline: Duration,
    #[cfg(test)]
    interrupt_count: Arc<AtomicUsize>,
}

impl DbWorker {
    pub fn new(db_path: PathBuf, app_id: String, deadline: Duration) -> Self {
        let (sender, receiver) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let next_operation = Arc::new(AtomicU64::new(0));
        let active_operation = Arc::new(AtomicU64::new(0));
        let interrupt = Arc::new(Mutex::new(None));
        let connection_open_count = Arc::new(AtomicUsize::new(0));
        #[cfg(test)]
        let interrupt_count = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new()
            .name("shell-search-db".into())
            .spawn({
                let generation = Arc::clone(&generation);
                let interrupt = Arc::clone(&interrupt);
                let connection_open_count = Arc::clone(&connection_open_count);
                move || {
                    run_worker(
                        receiver,
                        db_path,
                        app_id,
                        generation,
                        interrupt,
                        connection_open_count,
                    )
                }
            })
            .expect("spawn Shell search database worker");
        Self {
            sender,
            generation,
            next_operation,
            active_operation,
            interrupt,
            connection_open_count,
            deadline,
            #[cfg(test)]
            interrupt_count,
        }
    }

    pub async fn search_terms(
        &self,
        terms: &[String],
        previous_results: Option<Vec<String>>,
    ) -> SearchResponse {
        let request_generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.interrupt_current();
        let Some(expression) = build_match_expression(terms) else {
            tracing::debug!("Shell query rejected before database dispatch");
            self.active_operation.store(0, Ordering::Release);
            return SearchResponse::default();
        };

        let operation = self.next_operation.fetch_add(1, Ordering::AcqRel) + 1;
        self.active_operation.store(operation, Ordering::Release);
        let (reply, receiver) = async_channel::bounded(1);
        if self
            .sender
            .send(DbRequest::Search {
                generation: request_generation,
                expression: expression.clone(),
                previous_results,
                reply,
            })
            .is_err()
        {
            let _ = self.active_operation.compare_exchange(
                operation,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            return SearchResponse::default();
        }
        let ids = self.wait_for_reply(operation, receiver).await;
        SearchResponse {
            expression: Some(expression),
            ids: if self.generation.load(Ordering::Acquire) == request_generation {
                ids
            } else {
                Vec::new()
            },
        }
    }

    pub async fn metadata(
        &self,
        identifiers: Vec<String>,
        expression: Option<String>,
    ) -> MetadataResponse {
        let operation = self.next_operation.fetch_add(1, Ordering::AcqRel) + 1;
        self.active_operation.store(operation, Ordering::Release);
        let identifier_count = identifiers.len();
        let (reply, receiver) = async_channel::bounded(1);
        if self
            .sender
            .send(DbRequest::Metadata {
                identifiers,
                expression,
                reply,
            })
            .is_err()
        {
            let _ = self.active_operation.compare_exchange(
                operation,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            return MetadataResponse {
                show_excerpts: false,
                rows: vec![None; identifier_count],
            };
        }
        let mut response = self.wait_for_reply(operation, receiver).await;
        if response.rows.len() != identifier_count {
            response.show_excerpts = false;
            response.rows = vec![None; identifier_count];
        }
        response
    }

    fn interrupt_current(&self) {
        if let Some(handle) = self.interrupt.lock().unwrap().as_ref() {
            handle.interrupt();
        }
    }

    async fn wait_for_reply<T: Default + Send + 'static>(
        &self,
        operation: u64,
        receiver: async_channel::Receiver<T>,
    ) -> T {
        let timeout_operation = Arc::clone(&self.active_operation);
        let completed_operation = Arc::clone(&self.active_operation);
        let interrupt = Arc::clone(&self.interrupt);
        #[cfg(test)]
        let interrupt_count = Arc::clone(&self.interrupt_count);
        let result = futures_lite::future::race(
            async move { receiver.recv().await.unwrap_or_default() },
            async move {
                async_io::Timer::after(self.deadline).await;
                if timeout_operation.load(Ordering::Acquire) == operation {
                    #[cfg(test)]
                    interrupt_count.fetch_add(1, Ordering::AcqRel);
                    if let Some(handle) = interrupt.lock().unwrap().as_ref() {
                        handle.interrupt();
                    }
                }
                T::default()
            },
        )
        .await;
        let _ =
            completed_operation.compare_exchange(operation, 0, Ordering::AcqRel, Ordering::Acquire);
        result
    }
}

fn run_worker(
    receiver: mpsc::Receiver<DbRequest>,
    db_path: PathBuf,
    app_id: String,
    generation: Arc<AtomicU64>,
    interrupt: Arc<Mutex<Option<ShellSearchInterrupt>>>,
    connection_open_count: Arc<AtomicUsize>,
) {
    use gio::prelude::SettingsExt;

    let mut connection: Option<ShellSearchConnection> = None;
    while let Ok(request) = receiver.recv() {
        match request {
            DbRequest::Search {
                generation: request_generation,
                expression,
                previous_results,
                reply,
            } => {
                if generation.load(Ordering::Acquire) != request_generation {
                    let _ = reply.try_send(Vec::new());
                    continue;
                }
                if connection.is_none() {
                    connection_open_count.fetch_add(1, Ordering::AcqRel);
                    tracing::debug!(path = %db_path.display(), "opening shell search database");
                    match ShellSearchConnection::open_read_only(&db_path) {
                        Ok(Some((opened, handle))) => {
                            *interrupt.lock().unwrap() = Some(handle);
                            connection = Some(opened);
                        }
                        Ok(None) => {
                            let _ = reply.try_send(Vec::new());
                            continue;
                        }
                        Err(error) => {
                            tracing::warn!(%error, "failed to open shell search database");
                            let _ = reply.try_send(Vec::new());
                            continue;
                        }
                    }
                }
                let database = connection.as_ref().expect("connection opened above");
                let result = match previous_results {
                    Some(previous) => subsearch_session_ids(database, &expression, &previous),
                    None => search_session_ids(database, &expression),
                };
                let mut ids = result.unwrap_or_else(|error| {
                    tracing::debug!(%error, "Shell result query failed quietly");
                    Vec::new()
                });
                if generation.load(Ordering::Acquire) != request_generation {
                    ids.clear();
                }
                let _ = reply.try_send(ids);
            }
            DbRequest::Metadata {
                identifiers,
                expression,
                reply,
            } => {
                let settings = gio::Settings::new(&app_id);
                let show_excerpts =
                    settings.boolean("search-provider-show-excerpts") && expression.is_some();
                drop(settings);

                if connection.is_none() {
                    connection_open_count.fetch_add(1, Ordering::AcqRel);
                    tracing::debug!(path = %db_path.display(), "opening shell search database");
                    match ShellSearchConnection::open_read_only(&db_path) {
                        Ok(Some((opened, handle))) => {
                            *interrupt.lock().unwrap() = Some(handle);
                            connection = Some(opened);
                        }
                        Ok(None) | Err(_) => {
                            let _ = reply.try_send(MetadataResponse {
                                show_excerpts: false,
                                rows: vec![None; identifiers.len()],
                            });
                            continue;
                        }
                    }
                }
                let rows = connection
                    .as_ref()
                    .expect("connection opened above")
                    .load_metadata(&identifiers, show_excerpts, expression.as_deref())
                    .unwrap_or_else(|error| {
                        tracing::debug!(%error, "Shell metadata query failed quietly");
                        vec![None; identifiers.len()]
                    });
                let _ = reply.try_send(MetadataResponse {
                    show_excerpts,
                    rows,
                });
            }
            #[cfg(test)]
            DbRequest::Block { started, release } => {
                let _ = started.try_send(());
                let _ = release.recv_blocking();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl DbWorker {
        fn connection_open_count(&self) -> usize {
            self.connection_open_count.load(Ordering::Acquire)
        }

        fn interrupt_count(&self) -> usize {
            self.interrupt_count.load(Ordering::Acquire)
        }

        async fn block_worker(&self) -> async_channel::Sender<()> {
            let (started_sender, started_receiver) = async_channel::bounded(1);
            let (release_sender, release_receiver) = async_channel::bounded(1);
            self.sender
                .send(DbRequest::Block {
                    started: started_sender,
                    release: release_receiver,
                })
                .unwrap();
            started_receiver.recv().await.unwrap();
            release_sender
        }
    }

    #[test]
    fn invalid_terms_do_not_open_database() {
        async_io::block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let worker = DbWorker::new(
                directory.path().join("missing.db"),
                "dev.maciz.sessionschronicle.Devel".into(),
                Duration::from_millis(20),
            );
            let response = worker.search_terms(&["ak".into()], None).await;
            assert!(response.expression.is_none());
            assert!(response.ids.is_empty());
            assert_eq!(worker.connection_open_count(), 0);
        });
    }

    #[test]
    fn stale_generation_is_discarded() {
        async_io::block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let worker = DbWorker::new(
                directory.path().join("missing.db"),
                "dev.maciz.sessionschronicle.Devel".into(),
                Duration::from_millis(100),
            );
            let release = worker.block_worker().await;
            let first_terms = ["first".into()];
            let second_terms = ["second".into()];
            let first = worker.search_terms(&first_terms, None);
            let second = worker.search_terms(&second_terms, None);
            let _ = release.send(()).await;
            let (first, second) = futures_lite::future::zip(first, second).await;
            assert!(first.ids.is_empty());
            assert!(second.ids.is_empty());
        });
    }

    #[test]
    fn deadline_returns_default_response() {
        async_io::block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let worker = DbWorker::new(
                directory.path().join("missing.db"),
                "dev.maciz.sessionschronicle.Devel".into(),
                Duration::from_millis(10),
            );
            let release = worker.block_worker().await;
            let response = worker.search_terms(&["timeout".into()], None).await;
            assert!(response.ids.is_empty());
            let _ = release.send(()).await;
        });
    }

    #[test]
    fn deadline_interrupts_current_operation_once() {
        async_io::block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let worker = DbWorker::new(
                directory.path().join("missing.db"),
                "dev.maciz.sessionschronicle.Devel".into(),
                Duration::from_millis(10),
            );
            worker.active_operation.store(1, Ordering::Release);
            let (_sender, receiver) = async_channel::bounded::<()>(1);
            let fallback = worker.wait_for_reply(1, receiver).await;
            assert_eq!(fallback, ());
            assert_eq!(worker.interrupt_count(), 1);
        });
    }
}
