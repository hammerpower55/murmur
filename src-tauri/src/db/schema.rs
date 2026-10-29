//! Schema (spec 6.6), applied in order and tracked with PRAGMA user_version.

use anyhow::{Context, Result};
use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    // 1: transcripts and word counts.
    "CREATE TABLE transcripts (
        id INTEGER PRIMARY KEY,
        created_at TEXT NOT NULL,       -- ISO 8601 local time
        raw_text TEXT NOT NULL,
        final_text TEXT NOT NULL,
        word_count INTEGER NOT NULL,    -- of final_text
        audio_ms INTEGER NOT NULL,      -- speech duration
        inference_ms INTEGER NOT NULL,
        model_id TEXT NOT NULL,
        language TEXT,
        app_name TEXT,                  -- exe name of focused window, e.g. Code.exe
        app_title TEXT,
        wpm REAL NOT NULL               -- word_count / (audio_ms / 60000)
    );
    CREATE INDEX idx_transcripts_created ON transcripts(created_at);
    CREATE TABLE word_counts (word TEXT PRIMARY KEY, count INTEGER NOT NULL);",
    // 2: words the word fixer swapped, as a JSON list of {heard, meant}.
    "ALTER TABLE transcripts ADD COLUMN fixed_words TEXT NOT NULL DEFAULT '[]';",
];

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .context("could not read the database version")?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)
            .with_context(|| format!("database migration {} failed", i + 1))?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_once_and_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('transcripts', 'word_counts')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 2);
    }
}
