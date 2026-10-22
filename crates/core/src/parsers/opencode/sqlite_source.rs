//! Shared source connection: probing, enumeration and parsing see one snapshot.

use std::{path::Path, rc::Rc, time::Duration};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};

pub(crate) fn open_read_snapshot(path: &Path) -> Result<Rc<Connection>> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("Failed to open OpenCode DB: {}", path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch("BEGIN")
        .context("Failed to begin OpenCode read snapshot")?;
    // Closing the last reference rolls back the read transaction. Individual
    // readers must not roll it back while another reader still uses it.
    Ok(Rc::new(conn))
}
