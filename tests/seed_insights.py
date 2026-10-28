"""Seeds a history database with 200 transcripts and prints what the
Insights page must show for it.

    python tests/seed_insights.py [path/to/fixture.db]

Then run a debug build against it (your real history is untouched):

    $env:MURMUR_HISTORY_DB = "<path printed below>"; cargo tauri dev

The expected numbers are computed here in plain Python, independently of
the app's SQL, so the page is checked against a second implementation. The
fixture matches `two_hundred_transcript_fixture` in
src-tauri/src/db/insights.rs, anchored to today.
"""

import datetime as dt
import json
import os
import pathlib
import sqlite3
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
STOPWORDS = json.loads((ROOT / "src-tauri/src/text/stopwords.json").read_text(encoding="utf-8"))
STOP = {w.lower() for w in STOPWORDS["english"] + STOPWORDS["urdu"]}
TYPING_WPM = 40.0

PHRASES = [
    "send the deck to the finance team",
    "deck review moved to friday afternoon",
    "yaar kal meeting hai deck bhej do",
    "please check the invoice from last month",
    "the dashboard loads faster now",
    "schedule a call with the client on tuesday",
    "update the deck before lunch",
    "budget approved for the next quarter",
]
APPS = ["Code.exe", "Notepad.exe", "chrome.exe", "Slack.exe", "WINWORD.EXE"]

SCHEMA = """
CREATE TABLE transcripts (
    id INTEGER PRIMARY KEY, created_at TEXT NOT NULL, raw_text TEXT NOT NULL,
    final_text TEXT NOT NULL, word_count INTEGER NOT NULL, audio_ms INTEGER NOT NULL,
    inference_ms INTEGER NOT NULL, model_id TEXT NOT NULL, language TEXT,
    app_name TEXT, app_title TEXT, wpm REAL NOT NULL);
CREATE INDEX idx_transcripts_created ON transcripts(created_at);
CREATE TABLE word_counts (word TEXT PRIMARY KEY, count INTEGER NOT NULL);
PRAGMA user_version = 1;
"""


def word_count(text):
    """Same rule as text::stopwords::word_count."""
    return sum(1 for w in text.split() if any(c.isalnum() for c in w))


def countable_words(text):
    """Same rule as text::stopwords::countable_words."""
    out, cur = [], ""
    for c in text.lower() + " ":
        if c.isalnum() or c == "'":
            cur += c
        else:
            w = cur.strip("'")
            if sum(ch.isalpha() for ch in w) >= 2 and w not in STOP:
                out.append(w)
            cur = ""
    return out


def fixture(today):
    rows = []
    for n in range(200):
        days_ago = 13 if n % 45 == 12 else n % 45
        when = dt.datetime.combine(
            today - dt.timedelta(days=days_ago),
            dt.time(7 + (n * 5) % 14, (n * 7) % 60),
        )
        text = PHRASES[n % 8]
        rows.append(
            {
                "when": when,
                "text": text,
                "audio_ms": 1500 + (n * 37) % 4000,
                "app": APPS[n % 5],
            }
        )
    return rows


def seed(path, rows):
    if path.exists():
        path.unlink()
    db = sqlite3.connect(path)
    db.executescript(SCHEMA)
    counts = {}
    for r in rows:
        words = word_count(r["text"])
        db.execute(
            "INSERT INTO transcripts (created_at, raw_text, final_text, word_count, audio_ms,"
            " inference_ms, model_id, language, app_name, app_title, wpm)"
            " VALUES (?, ?, ?, ?, ?, 120, 'fixture', 'en', ?, NULL, ?)",
            (
                r["when"].strftime("%Y-%m-%dT%H:%M:%S"),
                r["text"],
                r["text"],
                words,
                r["audio_ms"],
                r["app"],
                words / (r["audio_ms"] / 60000),
            ),
        )
        for w in countable_words(r["text"]):
            counts[w] = counts.get(w, 0) + 1
    db.executemany("INSERT INTO word_counts (word, count) VALUES (?, ?)", counts.items())
    db.commit()
    db.close()
    return counts


def expected(rows, counts, today):
    def words_on(start, end):
        return sum(word_count(r["text"]) for r in rows if start <= r["when"].date() <= end)

    monday = today - dt.timedelta(days=today.weekday())
    last_monday = monday - dt.timedelta(days=7)
    first = today - dt.timedelta(days=29)
    recent = [r for r in rows if first <= r["when"].date() <= today]
    total_words = sum(word_count(r["text"]) for r in rows)
    total_min = sum(r["audio_ms"] for r in rows) / 60000
    w30 = sum(word_count(r["text"]) for r in recent)
    a30 = sum(r["audio_ms"] for r in recent) / 60000
    saved = max(0.0, total_words / TYPING_WPM - total_min)

    days = {r["when"].date() for r in rows}
    streak, d = 0, today
    while d in days:
        streak += 1
        d -= dt.timedelta(days=1)

    top = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:10]
    apps = {}
    for r in recent:
        apps[r["app"]] = apps.get(r["app"], 0) + word_count(r["text"])
    hours = [0] * 24
    for r in rows:
        hours[r["when"].hour] += 1
    per_day = [words_on(first + dt.timedelta(days=i), first + dt.timedelta(days=i)) for i in range(30)]

    this_week = words_on(monday, today)
    last_week = words_on(last_monday, last_monday + dt.timedelta(days=today.weekday()))
    return {
        "Words this week": f"{this_week:,} (last week, same days: {last_week:,}, change {this_week - last_week:+,})",
        "Total words": f"{total_words:,}",
        "Average speed": f"{round(w30 / a30)} wpm",
        "Time saved": f"{int(saved // 60)} h {round(saved % 60)} min ({saved:.2f} minutes)",
        "Streak": f"{streak} days",
        "Most used word": f"{top[0][0]} ({top[0][1]})",
        "Words per day, last 30": f"total {sum(per_day):,}, today {per_day[-1]}, peak {max(per_day)}",
        "Top 10 words": ", ".join(f"{w} {c}" for w, c in top),
        "Words by app, last 30": ", ".join(f"{a} {w}" for a, w in sorted(apps.items(), key=lambda kv: (-kv[1], kv[0]))),
        "Busiest hour": f"{max(range(24), key=lambda h: hours[h])}:00 with {max(hours)} dictations",
        "Dictations by hour": " ".join(str(h) for h in hours),
    }


def main():
    path = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path(tempfile.gettempdir()) / "murmur-insights-fixture.db"
    today = dt.date.today()
    rows = fixture(today)
    counts = seed(path, rows)
    print(f"Seeded {len(rows)} transcripts into {path}")
    print(f"Anchored to today, {today:%A %d %B %Y}\n")
    for k, v in expected(rows, counts, today).items():
        print(f"{k:<24} {v}")


if __name__ == "__main__":
    main()
