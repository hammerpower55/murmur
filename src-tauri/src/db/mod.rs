//! Transcript history in SQLite at %APPDATA%\Murmur\history.db (spec 6.6).
//! One connection behind a mutex, shared by the dictation thread (inserts)
//! and the UI commands (reads, deletes).

pub mod insights;
pub mod schema;
pub mod stats;
pub mod transcripts;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;

pub const FILE: &str = "history.db";

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        let mut conn =
            Connection::open(path).with_context(|| format!("could not open {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        schema::migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        let mut conn = Connection::open_in_memory().unwrap();
        schema::migrate(&mut conn).unwrap();
        Self {
            conn: Mutex::new(conn),
        }
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow!("database lock poisoned"))
    }
}
