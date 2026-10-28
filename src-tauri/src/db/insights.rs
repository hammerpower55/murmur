//! The Insights dashboard (spec 6.7): six cards and four charts, all from
//! SQL. `today` and the typing speed are passed in so every number can be
//! tested against fixed data.

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::params;
use serde::Serialize;

use super::stats::week_start;
use super::Db;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordCount {
    pub word: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayWords {
    /// YYYY-MM-DD
    pub date: String,
    pub words: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppWords {
    /// Exe name, or None when the focused app could not be read.
    pub app: Option<String>,
    pub words: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    pub total_dictations: i64,
    pub words_this_week: i64,
    /// Last week over the same days (Monday through today's weekday).
    pub words_last_week: i64,
    pub total_words: i64,
    /// Words per minute of speech over the last 30 days.
    pub average_wpm: f64,
    /// Typing time minus speaking time, over all history. Never negative.
    pub minutes_saved: f64,
    pub streak_days: u32,
    pub most_used_word: Option<WordCount>,
    /// Last 30 days, oldest first, days without dictation included as 0.
    pub words_per_day: Vec<DayWords>,
    pub top_words: Vec<WordCount>,
    /// Last 30 days, most words first.
    pub words_by_app: Vec<AppWords>,
    /// 24 entries: dictations started in each hour of the day, all history.
    pub dictations_by_hour: Vec<i64>,
}

const DAYS: i64 = 30;
const TOP_WORDS: u32 = 10;
const TOP_APPS: u32 = 8;

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

impl Db {
    /// Words dictated on days from..=to.
    fn words_between(&self, from: NaiveDate, to: NaiveDate) -> Result<i64> {
        Ok(self.conn()?.query_row(
            "SELECT coalesce(sum(word_count), 0) FROM transcripts
             WHERE created_at >= ?1 AND created_at < ?2",
            [ymd(from), ymd(to + Duration::days(1))],
            |r| r.get(0),
        )?)
    }

    pub fn insights(&self, today: NaiveDate, typing_wpm: f64) -> Result<Insights> {
        let monday = week_start(today);
        let days_in = today.weekday().num_days_from_monday() as i64;
        let last_monday = monday - Duration::days(7);
        let first_day = today - Duration::days(DAYS - 1);
        let tomorrow = ymd(today + Duration::days(1));

        let (total_dictations, total_words, total_audio_ms): (i64, i64, i64) = self.conn()?.query_row(
            "SELECT count(*), coalesce(sum(word_count), 0), coalesce(sum(audio_ms), 0) FROM transcripts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

        let (words_30, audio_ms_30): (i64, i64) = self.conn()?.query_row(
            "SELECT coalesce(sum(word_count), 0), coalesce(sum(audio_ms), 0) FROM transcripts
             WHERE created_at >= ?1 AND created_at < ?2",
            [ymd(first_day), tomorrow.clone()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let average_wpm = if audio_ms_30 > 0 {
            words_30 as f64 / (audio_ms_30 as f64 / 60_000.0)
        } else {
            0.0
        };
        let minutes_saved = if typing_wpm > 0.0 {
            (total_words as f64 / typing_wpm - total_audio_ms as f64 / 60_000.0).max(0.0)
        } else {
            0.0
        };

        let conn = self.conn()?;
        let top_words: Vec<WordCount> = conn
            .prepare("SELECT word, count FROM word_counts ORDER BY count DESC, word ASC LIMIT ?1")?
            .query_map([TOP_WORDS], |r| {
                Ok(WordCount {
                    word: r.get(0)?,
                    count: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let per_day: Vec<(String, i64)> = conn
            .prepare(
                "SELECT substr(created_at, 1, 10) AS day, sum(word_count) FROM transcripts
                 WHERE created_at >= ?1 AND created_at < ?2 GROUP BY day",
            )?
            .query_map(params![ymd(first_day), tomorrow], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let words_per_day = (0..DAYS)
            .map(|i| {
                let date = ymd(first_day + Duration::days(i));
                let words = per_day
                    .iter()
                    .find(|(d, _)| *d == date)
                    .map_or(0, |(_, w)| *w);
                DayWords { date, words }
            })
            .collect();

        let words_by_app: Vec<AppWords> = conn
            .prepare(
                "SELECT app_name, sum(word_count) AS words FROM transcripts
                 WHERE created_at >= ?1 AND created_at < ?2
                 GROUP BY lower(coalesce(app_name, ''))
                 ORDER BY words DESC, app_name ASC LIMIT ?3",
            )?
            .query_map(
                params![ymd(first_day), ymd(today + Duration::days(1)), TOP_APPS],
                |r| {
                    Ok(AppWords {
                        app: r.get(0)?,
                        words: r.get(1)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<_>>()?;

        let mut dictations_by_hour = vec![0i64; 24];
        let hours: Vec<(i64, i64)> = conn
            .prepare(
                "SELECT CAST(substr(created_at, 12, 2) AS INTEGER) AS hour, count(*)
                 FROM transcripts GROUP BY hour",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (hour, count) in hours {
            if let Some(slot) = dictations_by_hour.get_mut(hour as usize) {
                *slot = count;
            }
        }
        drop(conn);

        Ok(Insights {
            total_dictations,
            words_this_week: self.words_between(monday, today)?,
            words_last_week: self
                .words_between(last_monday, last_monday + Duration::days(days_in))?,
            total_words,
            average_wpm,
            minutes_saved,
            streak_days: self.streak(today)?,
            most_used_word: top_words.first().cloned(),
            words_per_day,
            top_words,
            words_by_app,
            dictations_by_hour,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::transcripts::tests::{at, new};
    use crate::db::transcripts::NewTranscript;
    use chrono::Timelike;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    // 28 October 2026 is a Wednesday.
    const TODAY: (i32, u32, u32) = (2026, 10, 28);

    fn today() -> NaiveDate {
        day(TODAY.0, TODAY.1, TODAY.2)
    }

    #[test]
    fn empty_history_is_all_zeros() {
        let i = Db::in_memory().insights(today(), 40.0).unwrap();
        assert_eq!(i.total_dictations, 0);
        assert_eq!(i.total_words, 0);
        assert_eq!(i.average_wpm, 0.0);
        assert_eq!(i.minutes_saved, 0.0);
        assert_eq!(i.most_used_word, None);
        assert_eq!(i.words_per_day.len(), 30);
        assert!(i.words_per_day.iter().all(|d| d.words == 0));
        assert_eq!(i.dictations_by_hour, vec![0; 24]);
        assert!(i.top_words.is_empty() && i.words_by_app.is_empty());
    }

    #[test]
    fn small_hand_checked_history() {
        let db = Db::in_memory();
        // Today 09:00, Notepad: 4 words in 3 s.
        db.insert_transcript(&new("deck review deck changes", at(2026, 10, 28, 9), 3000))
            .unwrap();
        // Monday 14:00, Code: 2 words in 1.2 s.
        let mut code = new("ship deck", at(2026, 10, 26, 14), 1200);
        code.app_name = Some("Code.exe".into());
        db.insert_transcript(&code).unwrap();
        // Last Tuesday 09:00 (inside last week's Mon to Wed): 3 words.
        db.insert_transcript(&new("budget looks fine", at(2026, 10, 20, 9), 1800))
            .unwrap();
        // Last Thursday: after last week's Mon to Wed, so not compared.
        db.insert_transcript(&new("lunch plans", at(2026, 10, 22, 9), 1000))
            .unwrap();

        let i = db.insights(today(), 40.0).unwrap();
        assert_eq!(i.total_dictations, 4);
        assert_eq!(i.words_this_week, 6);
        assert_eq!(i.words_last_week, 3);
        assert_eq!(i.total_words, 11);
        // 11 words over 7 s of speech.
        assert!((i.average_wpm - 11.0 / (7000.0 / 60_000.0)).abs() < 1e-9);
        // 11 / 40 minutes typing minus 7 s speaking.
        assert!((i.minutes_saved - (11.0 / 40.0 - 7.0 / 60.0)).abs() < 1e-9);
        assert_eq!(i.streak_days, 1);
        assert_eq!(
            i.most_used_word,
            Some(WordCount {
                word: "deck".into(),
                count: 3
            })
        );
        assert_eq!(i.words_per_day.last().unwrap().words, 4);
        assert_eq!(i.words_per_day[27].date, "2026-10-26");
        assert_eq!(i.words_per_day[27].words, 2);
        assert_eq!(
            i.words_by_app,
            vec![
                AppWords {
                    app: Some("Notepad.exe".into()),
                    words: 9
                },
                AppWords {
                    app: Some("Code.exe".into()),
                    words: 2
                },
            ]
        );
        assert_eq!(i.dictations_by_hour[9], 3);
        assert_eq!(i.dictations_by_hour[14], 1);
    }

    #[test]
    fn time_saved_never_goes_negative() {
        let db = Db::in_memory();
        // 2 words in a whole minute: speaking was slower than typing.
        db.insert_transcript(&new("slow words", at(2026, 10, 28, 9), 60_000))
            .unwrap();
        assert_eq!(db.insights(today(), 40.0).unwrap().minutes_saved, 0.0);
    }

    #[test]
    fn older_than_30_days_counts_in_totals_only() {
        let db = Db::in_memory();
        db.insert_transcript(&new("ancient history notes", at(2026, 8, 1, 9), 1000))
            .unwrap();
        let i = db.insights(today(), 40.0).unwrap();
        assert_eq!(i.total_words, 3);
        assert_eq!(i.average_wpm, 0.0);
        assert!(i.words_by_app.is_empty());
        assert!(i.words_per_day.iter().all(|d| d.words == 0));
        assert_eq!(i.dictations_by_hour[9], 1);
    }

    /// The same 200 transcript fixture as tests/seed_insights.py, checked
    /// against totals computed by plain loops instead of SQL.
    #[test]
    fn two_hundred_transcript_fixture() {
        let db = Db::in_memory();
        let rows = fixture(today());
        for r in &rows {
            db.insert_transcript(r).unwrap();
        }
        let i = db.insights(today(), 40.0).unwrap();
        let words = |r: &NewTranscript| crate::text::stopwords::word_count(&r.final_text) as i64;
        let on = |r: &NewTranscript, from: NaiveDate, to: NaiveDate| {
            let d = r.created_at.date();
            d >= from && d <= to
        };

        assert_eq!(i.total_dictations, 200);
        assert_eq!(i.total_words, rows.iter().map(words).sum::<i64>());
        let monday = week_start(today());
        assert_eq!(
            i.words_this_week,
            rows.iter()
                .filter(|r| on(r, monday, today()))
                .map(words)
                .sum::<i64>()
        );
        let last_monday = monday - Duration::days(7);
        assert_eq!(
            i.words_last_week,
            rows.iter()
                .filter(|r| on(r, last_monday, last_monday + Duration::days(2)))
                .map(words)
                .sum::<i64>()
        );
        let first = today() - Duration::days(29);
        let recent: Vec<_> = rows.iter().filter(|r| on(r, first, today())).collect();
        let w30: i64 = recent.iter().map(|r| words(r)).sum();
        let a30: u64 = recent.iter().map(|r| r.audio_ms).sum();
        assert!((i.average_wpm - w30 as f64 / (a30 as f64 / 60_000.0)).abs() < 1e-9);
        assert_eq!(i.words_per_day.iter().map(|d| d.words).sum::<i64>(), w30);
        assert_eq!(i.words_by_app.iter().map(|a| a.words).sum::<i64>(), w30);
        let mut hours = vec![0i64; 24];
        for r in &rows {
            hours[r.created_at.hour() as usize] += 1;
        }
        assert_eq!(i.dictations_by_hour, hours);
        // The fixture dictates every day for the last 12 days.
        assert_eq!(i.streak_days, 12);
        assert_eq!(i.top_words.len(), 10);
        assert_eq!(
            i.most_used_word.as_ref().map(|w| w.word.as_str()),
            Some("deck")
        );
    }

    /// Deterministic: transcript n is dictated (n mod 45) days ago at hour
    /// 7 + (n * 5) mod 14. Day 12 ago is moved to day 13, so the streak is
    /// exactly 12 days (today and the 11 before it).
    pub fn fixture(today: NaiveDate) -> Vec<NewTranscript> {
        const PHRASES: [&str; 8] = [
            "send the deck to the finance team",
            "deck review moved to friday afternoon",
            "yaar kal meeting hai deck bhej do",
            "please check the invoice from last month",
            "the dashboard loads faster now",
            "schedule a call with the client on tuesday",
            "update the deck before lunch",
            "budget approved for the next quarter",
        ];
        const APPS: [&str; 5] = [
            "Code.exe",
            "Notepad.exe",
            "chrome.exe",
            "Slack.exe",
            "WINWORD.EXE",
        ];
        (0..200u32)
            .map(|n| {
                let days_ago = match n % 45 {
                    12 => 13,
                    d => d,
                };
                let date = today - Duration::days(days_ago as i64);
                let when = date.and_hms_opt(7 + (n * 5) % 14, (n * 7) % 60, 0).unwrap();
                let mut t = new(
                    PHRASES[(n % 8) as usize],
                    when,
                    1500 + (n as u64 * 37) % 4000,
                );
                t.app_name = Some(APPS[(n % 5) as usize].into());
                t
            })
            .collect()
    }
}
