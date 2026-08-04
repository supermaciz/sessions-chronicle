pub mod config;
pub mod db_worker;
pub mod interface;
pub mod lifecycle;

pub use interface::SearchProvider;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use config::{APP_ID, bus_name, provider_object_path};
use db_worker::DbWorker;
use lifecycle::ActivityTracker;

pub async fn run(database_path: PathBuf) -> Result<()> {
    let worker = Arc::new(DbWorker::new(
        database_path,
        APP_ID.to_string(),
        Duration::from_millis(250),
    ));
    let provider = SearchProvider::new(
        worker,
        APP_ID.to_string(),
        ActivityTracker::new(Duration::from_secs(30)),
    );
    let _connection = zbus::connection::Builder::session()?
        .name(bus_name(APP_ID))?
        .serve_at(provider_object_path(APP_ID), provider.clone())?
        .build()
        .await?;

    loop {
        async_io::Timer::after(Duration::from_millis(250)).await;
        // The object remains registered while calls are in flight. The timer is
        // checked here rather than in a background task so shutdown is orderly.
        // The connection is kept alive until the idle period has elapsed.
        // A provider restart is safe because result IDs are session IDs.
        //
        // This loop is intentionally driven through the registered interface;
        // zbus dispatches calls while this future yields.
        if provider.is_idle() {
            break;
        }
    }
    Ok(())
}
