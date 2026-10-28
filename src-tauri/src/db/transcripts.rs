//! Transcript rows and the word_counts table that follows them.

use anyhow::Result;
use chrono::NaiveDateTime;
use rusqlite::{params, OptionalExtension, Row, Transaction};
use serde::Serialize;

use super::Db;
use crate::text::stopwords::{countable_words, word_count};

/// created_at format: ISO 8601 local time, sortable as text.
pub const TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

pub struct NewTranscript {
    pub created_at: NaiveDateTime,
    pub raw_text: String,
    pub final_text: String,
    pub audio_ms: u64,
    pub inference_ms: u64,
    pub model_id: String,
    pub language: Option<String>,
    pub app_name: Option<String>,
    pub app_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRow {
    pub id: i64,
    pub created_at: String,
    pub raw_text: String,
    pub final_text: String,
    pub word_count: i64,
    pub audio_ms: i64,
    pub inference_ms: i64,
    pub model_id: String,
    pub language: Option<String>,
    pub app_name: Option<String>,
    pub app_title: Option<String>,
    pub wpm: f64,
}

const COLUMNS: &str = "id, created_at, raw_text, final_text, word_count, audio_ms, inference_ms, \
                       model_id, language, app_name, app_title, wpm";

fn row(r: &Row) -> rusqlite::Result<TranscriptRow> {
    Ok(TranscriptRow {
        id: r.get(0)?,
        created_at: r.get(1)?,
        raw_text: r.get(2)?,
        final_text: r.get(3)?,
        word_count: r.get(4)?,
        audio_ms: r.get(5)?,
        inference_ms: r.get(6)?,
        model_id: r.get(7)?,
        language: r.get(8)?,
        app_name: r.get(9)?,
        app_title: r.get(10)?,
        wpm: r.get(11)?,
    })
}

/// word_count / (audio_ms / 60000); 0 when there was no audio.
pub fn words_per_minute(words: usize, audio_ms: u64) -> f64 {
    if audio_ms == 0 {
        return 0.0;
    }
    words as f64 / (audio_ms as f64 / 60_000.0)
}

fn adjust_word_counts(tx: &Transaction, text: &str, delta: i64) -> Result<()> {
    for word in countable_words(text) {
        tx.execute(
            "INSERT INTO word_counts (word, count) VALUES (?1, ?2)
             ON CONFLICT(word) DO UPDATE SET count = count + ?2",
            params![word, delta],
        )?;
    }
    tx.execute("DELETE FROM word_counts WHERE count <= 0", [])?;
    Ok(())
}

/// Escapes LIKE wildcards so a search for "50%" means the text "50%".
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

impl Db {
    pub fn insert_transcript(&self, t: &NewTranscript) -> Result<i64> {
        let words = word_count(&t.final_text);
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO transcripts (created_at, raw_text, final_text, word_count, audio_ms,
                 inference_ms, model_id, language, app_name, app_title, wpm)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                t.created_at.format(TIME_FORMAT).to_string(),
                t.raw_text,
                t.final_text,
                words as i64,
                t.audio_ms as i64,
                t.inference_ms as i64,
                t.model_id,
                t.language,
                t.app_name,
                t.app_title,
                words_per_minute(words, t.audio_ms),
            ],
        )?;
        let id = tx.last_insert_rowid();
        adjust_word_counts(&tx, &t.final_text, 1)?;
        tx.commit()?;
        Ok(id)
    }

    /// Newest first. An empty query lists everything.
    pub fn list_transcripts(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TranscriptRow>> {
        let conn = self.conn()?;
        let query = query.trim();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM transcripts
             WHERE ?1 = '' OR final_text LIKE ?2 ESCAPE '\\'
             ORDER BY created_at DESC, id DESC
             LIMIT ?3 OFFSET ?4"
        ))?;
        let rows = stmt
            .query_map(params![query, like_pattern(query), limit, offset], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Deletes one transcript and takes its words back out of word_counts.
    pub fn delete_transcript(&self, id: i64) -> Result<bool> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let text: Option<String> = tx
            .query_row(
                "SELECT final_text FROM transcripts WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(text) = text else {
            return Ok(false);
        };
        adjust_word_counts(&tx, &text, -1)?;
        tx.execute("DELETE FROM transcripts WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn clear_history(&self) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM transcripts", [])?;
        tx.execute("DELETE FROM word_counts", [])?;
        tx.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn transcript_count(&self) -> Result<i64> {
        Ok(self
            .conn()?
            .query_row("SELECT count(*) FROM transcripts", [], |r| r.get(0))?)
    }

    #[cfg(test)]
    pub fn word_count_of(&self, word: &str) -> i64 {
        self.conn()
            .unwrap()
            .query_row(
                "SELECT count FROM word_counts WHERE word = ?1",
                [word],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
            .unwrap_or(0)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use chrono::NaiveDate;

    pub fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    pub fn new(text: &str, when: NaiveDateTime, audio_ms: u64) -> NewTranscript {
        NewTranscript {
            created_at: when,
            raw_text: format!("um {text}"),
            final_text: text.into(),
            audio_ms,
            inference_ms: 120,
            model_id: "parakeet-tdt-0.6b-v2".into(),
            language: Some("en".into()),
            app_name: Some("Notepad.exe".into()),
            app_title: Some("notes.txt - Notepad".into()),
        }
    }

    #[test]
    fn insert_fills_every_column() {
        let db = Db::in_memory();
        let id = db
            .insert_transcript(&new(
                "Send the quarterly report today.",
                at(2026, 10, 28, 9),
                3000,
            ))
            .unwrap();
        let rows = db.list_transcripts("", 10, 0).unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.id, id);
        assert_eq!(r.created_at, "2026-10-28T09:00:00");
        assert_eq!(r.word_count, 5);
        assert_eq!(r.wpm, 100.0);
        assert_eq!(r.raw_text, "um Send the quarterly report today.");
        assert_eq!(r.app_name.as_deref(), Some("Notepad.exe"));
    }

    #[test]
    fn lists_newest_first_with_paging() {
        let db = Db::in_memory();
        for h in 1..=5 {
            db.insert_transcript(&new(&format!("note {h}"), at(2026, 10, 28, h), 1000))
                .unwrap();
        }
        let page: Vec<_> = db
            .list_transcripts("", 2, 1)
            .unwrap()
            .into_iter()
            .map(|r| r.final_text)
            .collect();
        assert_eq!(page, vec!["note 4", "note 3"]);
    }

    #[test]
    fn search_is_case_insensitive_and_escapes_wildcards() {
        let db = Db::in_memory();
        db.insert_transcript(&new("Budget is 50% done", at(2026, 10, 28, 9), 1000))
            .unwrap();
        db.insert_transcript(&new("Budget is 500 dollars", at(2026, 10, 28, 10), 1000))
            .unwrap();
        db.insert_transcript(&new("Lunch at noon", at(2026, 10, 28, 11), 1000))
            .unwrap();
        assert_eq!(db.list_transcripts("budget", 10, 0).unwrap().len(), 2);
        let pct = db.list_transcripts("50%", 10, 0).unwrap();
        assert_eq!(pct.len(), 1);
        assert_eq!(pct[0].final_text, "Budget is 50% done");
        assert!(db.list_transcripts("missing", 10, 0).unwrap().is_empty());
    }

    #[test]
    fn word_counts_follow_inserts_and_deletes() {
        let db = Db::in_memory();
        let a = db
            .insert_transcript(&new(
                "Deck review, deck changes.",
                at(2026, 10, 28, 9),
                2000,
            ))
            .unwrap();
        db.insert_transcript(&new("Ship the deck.", at(2026, 10, 28, 10), 2000))
            .unwrap();
        assert_eq!(db.word_count_of("deck"), 3);
        assert_eq!(db.word_count_of("the"), 0, "stopword");
        assert!(db.delete_transcript(a).unwrap());
        assert_eq!(db.word_count_of("deck"), 1);
        assert_eq!(db.word_count_of("review"), 0);
        assert!(!db.delete_transcript(a).unwrap());
    }

    #[test]
    fn clear_history_empties_both_tables() {
        let db = Db::in_memory();
        db.insert_transcript(&new("Deck review", at(2026, 10, 28, 9), 1000))
            .unwrap();
        db.clear_history().unwrap();
        assert_eq!(db.transcript_count().unwrap(), 0);
        assert_eq!(db.word_count_of("deck"), 0);
    }

    #[test]
    fn wpm_handles_zero_audio() {
        assert_eq!(words_per_minute(10, 0), 0.0);
        assert_eq!(words_per_minute(10, 6000), 100.0);
    }
}
