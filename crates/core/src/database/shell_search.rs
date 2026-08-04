use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const SQLITE_BUSY_TIMEOUT_SECS: u64 = 5;

pub struct ShellSearchConnection {
    pub connection: Connection,
}

#[derive(Clone)]
pub struct ShellSearchInterrupt {
    handle: Arc<rusqlite::InterruptHandle>,
}

impl ShellSearchConnection {
    pub fn open_read_only(path: &Path) -> Result<Option<Self>> {
        if !path.is_file() {
            return Ok(None);
        }

        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags)?;
        connection.busy_timeout(Duration::from_secs(SQLITE_BUSY_TIMEOUT_SECS))?;

        Ok(Some(Self { connection }))
    }

    pub fn interrupt(&self) -> ShellSearchInterrupt {
        ShellSearchInterrupt {
            handle: Arc::new(self.connection.get_interrupt_handle()),
        }
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
}
