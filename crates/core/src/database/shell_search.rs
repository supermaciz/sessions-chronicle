use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const SQLITE_BUSY_TIMEOUT_SECS: u64 = 5;

pub struct ShellSearchConnection {
    #[allow(dead_code)]
    connection: Connection,
}

#[derive(Clone)]
pub struct ShellSearchInterrupt {
    handle: Arc<rusqlite::InterruptHandle>,
}

impl ShellSearchConnection {
    pub fn open_read_only(path: &Path) -> Result<Option<(Self, ShellSearchInterrupt)>> {
        if !path.is_file() {
            return Ok(None);
        }

        // This connection is owned by the dedicated search worker.
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags)
            .with_context(|| format!("Failed to open shell search database: {}", path.display()))?;
        connection
            .busy_timeout(Duration::from_secs(SQLITE_BUSY_TIMEOUT_SECS))
            .context("Failed to set shell search SQLite busy timeout")?;
        let interrupt = ShellSearchInterrupt {
            handle: Arc::new(connection.get_interrupt_handle()),
        };

        Ok(Some((Self { connection }, interrupt)))
    }
}

impl ShellSearchInterrupt {
    pub fn interrupt(&self) {
        self.handle.interrupt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn missing_database_is_not_created() {
        let temp_dir = tempfile::tempdir().unwrap();
        let database_path = temp_dir.path().join("missing.db");

        assert!(
            ShellSearchConnection::open_read_only(&database_path)
                .unwrap()
                .is_none()
        );
        assert!(!database_path.exists());

        let directory_path = temp_dir.path().join("database-dir");
        fs::create_dir(&directory_path).unwrap();
        assert!(
            ShellSearchConnection::open_read_only(&directory_path)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn opened_database_rejects_writes() {
        let temp_dir = tempfile::tempdir().unwrap();
        let database_path = temp_dir.path().join("existing.db");
        let setup = Connection::open(&database_path).unwrap();
        setup
            .execute_batch("CREATE TABLE sessions (id INTEGER PRIMARY KEY)")
            .unwrap();
        drop(setup);

        let (connection, _interrupt) = ShellSearchConnection::open_read_only(&database_path)
            .unwrap()
            .unwrap();

        assert!(
            connection
                .connection
                .execute("INSERT INTO sessions DEFAULT VALUES", [])
                .is_err()
        );
    }
}
