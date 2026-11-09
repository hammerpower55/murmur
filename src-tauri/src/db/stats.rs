//! Numbers for the Home page: words this week and the current streak. The
//! full Insights dashboard (Phase 7) builds on these. `today` is passed in
//! so the queries can be tested against fixed dates.

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;

use super::transcripts::TranscriptRow;
use super::Db;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeSummary {
    pub words_this_week: i64,
    pub streak_days: u32,
    pub recent: Vec<TranscriptRow>,
}

/// Monday of the week containing `day`.
pub fn week_start(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_monday() as i64)
}

impl Db {
    /// (model id, inference ms, audio ms) of recent dictations, newest
    /// first, for measuring each model's speed on this PC.
    pub fn model_timings(&self) -> Result<Vec<(String, u64, u64)>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT model_id, inference_ms, audio_ms FROM transcripts
             ORDER BY id DESC LIMIT 1000",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?.max(0) as u64, r.get::<_, i64>(2)?.max(0) as u64))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Words dictated from Monday through `today`.
    pub fn words_this_week(&self, today: NaiveDate) -> Result<i64> {
        let from = week_start(today).format("%Y-%m-%d").to_string();
        let to = (today + Duration::days(1)).format("%Y-%m-%d").to_string();
        Ok(self.conn()?.query_row(
            "SELECT coalesce(sum(word_count), 0) FROM transcripts
             WHERE created_at >= ?1 AND created_at < ?2",
            [from, to],
            |r| r.get(0),
        )?)
    }

    /// Consecutive days ending today with at least one transcript. A day
    /// without dictation today means a streak of 0.
    pub fn streak(&self, today: NaiveDate) -> Result<u32> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT substr(created_at, 1, 10) AS day FROM transcripts
             WHERE created_at < ?1 ORDER BY day DESC",
        )?;
        let tomorrow = (today + Duration::days(1)).format("%Y-%m-%d").to_string();
        let days = stmt
            .query_map([tomorrow], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut expected = today;
        let mut streak = 0;
        for day in days {
            if day != expected.format("%Y-%m-%d").to_string() {
                break;
            }
            streak += 1;
            expected -= Duration::days(1);
        }
        Ok(streak)
    }

    pub fn home_summary(&self, today: NaiveDate) -> Result<HomeSummary> {
        Ok(HomeSummary {
            words_this_week: self.words_this_week(today)?,
            streak_days: self.streak(today)?,
            recent: self.list_transcripts("", 5, 0)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::transcripts::tests::{at, new};

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn week_starts_on_monday() {
        // 28 October 2026 is a Wednesday.
        assert_eq!(week_start(day(2026, 10, 28)), day(2026, 10, 26));
        assert_eq!(week_start(day(2026, 10, 26)), day(2026, 10, 26));
        assert_eq!(week_start(day(2026, 11, 1)), day(2026, 10, 26));
    }

    #[test]
    fn words_this_week_counts_monday_to_today_only() {
        let db = Db::in_memory();
        db.insert_transcript(&new("one two three", at(2026, 10, 25, 23), 1000))
            .unwrap(); // Sunday, last week
        db.insert_transcript(&new("one two", at(2026, 10, 26, 0), 1000))
            .unwrap(); // Monday
        db.insert_transcript(&new("one two three four", at(2026, 10, 28, 9), 1000))
            .unwrap(); // today
        db.insert_transcript(&new("future words here", at(2026, 10, 29, 9), 1000))
            .unwrap(); // tomorrow
        assert_eq!(db.words_this_week(day(2026, 10, 28)).unwrap(), 6);
    }

    #[test]
    fn streak_counts_back_from_today() {
        let db = Db::in_memory();
        for (d, h) in [(28, 9), (28, 15), (27, 9), (26, 9), (24, 9)] {
            db.insert_transcript(&new("hello there", at(2026, 10, d, h), 1000))
                .unwrap();
        }
        assert_eq!(db.streak(day(2026, 10, 28)).unwrap(), 3);
        // Nothing yet on the 29th: the streak is broken.
        assert_eq!(db.streak(day(2026, 10, 29)).unwrap(), 0);
        assert_eq!(db.streak(day(2026, 10, 24)).unwrap(), 1);
    }

    #[test]
    fn empty_history() {
        let db = Db::in_memory();
        let s = db.home_summary(day(2026, 10, 28)).unwrap();
        assert_eq!(
            (s.words_this_week, s.streak_days, s.recent.len()),
            (0, 0, 0)
        );
    }

    #[test]
    fn summary_has_the_last_five() {
        let db = Db::in_memory();
        for h in 1..=7 {
            db.insert_transcript(&new(&format!("note {h}"), at(2026, 10, 28, h), 1000))
                .unwrap();
        }
        let s = db.home_summary(day(2026, 10, 28)).unwrap();
        assert_eq!(s.recent.len(), 5);
        assert_eq!(s.recent[0].final_text, "note 7");
    }
}
