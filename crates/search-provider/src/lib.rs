pub mod activation;
pub mod config;
pub mod db_worker;
pub mod interface;
pub mod lifecycle;

pub use interface::SearchProvider;

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use config::{bus_name, provider_object_path};
use db_worker::DbWorker;
use lifecycle::ActivityTracker;

pub async fn run(database_path: PathBuf, app_id: String, idle_timeout: Duration) -> Result<()> {
    let worker = Arc::new(DbWorker::new(
        database_path,
        app_id.clone(),
        Duration::from_millis(250),
    ));
    let provider = SearchProvider::new(worker, app_id.clone(), ActivityTracker::new(idle_timeout));
    let _connection = zbus::connection::Builder::session()?
        .name(bus_name(&app_id))?
        .serve_at(provider_object_path(&app_id), provider.clone())?
        .build()
        .await?;

    loop {
        async_io::Timer::after(Duration::from_millis(10)).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_exits_after_configured_idle_timeout() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("skipping runtime test: DBUS_SESSION_BUS_ADDRESS is absent");
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let app_id = "dev.maciz.sessionschronicle.RuntimeTest".to_string();
        let started = std::time::Instant::now();
        async_io::block_on(run(
            temp.path().join("sessions.db"),
            app_id,
            Duration::from_millis(20),
        ))
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
