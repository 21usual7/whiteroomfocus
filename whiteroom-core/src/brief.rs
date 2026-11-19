//! Утренний брифинг и панели главного экрана: только то, что уже записано в базе.

use super::{DAY, Db, Item, Res, TEST_DELAY, e};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

impl Db {
    /// `offset` — смещение местного времени от UTC в секундах: границы дня считаются по часам пользователя.
    pub fn briefing(&self, now: i64, offset: i64) -> Res<Value> {
        let today = (now + offset).div_euclid(DAY) * DAY - offset;
        // Последний день с занятиями до сегодняшнего: если вчера был пропуск, показываем прошлый раз.
        let last: Option<i64> = self
            .c
            .query_row(
                "SELECT max(t) FROM (
                   SELECT max(started) t FROM study WHERE ended IS NOT NULL AND started<?1
                   UNION ALL SELECT max(ts) FROM attempt WHERE ts<?1
                   UNION ALL SELECT max(solved_at) FROM challenge WHERE solved_at<?1)",
                [today],
                |r| r.get(0),
            )
            .map_err(e)?;
        let day = match last {
            Some(t) => {
                let start = (t + offset).div_euclid(DAY) * DAY - offset;
                self.day(start, start + DAY)?
            }
            None => Value::Null,
        };

        let notes: Vec<(String, String, String)> = self.list(
            "SELECT domain, title, substr(excerpt, 1, 240) FROM page
             WHERE excerpt IS NOT NULL AND trim(excerpt)!='' ORDER BY ts DESC LIMIT 3",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let working: Vec<String> = self.list(
            "SELECT title FROM challenge c WHERE solved_at IS NULL
               AND EXISTS (SELECT 1 FROM challenge_attempt a WHERE a.challenge_id=c.id) ORDER BY id LIMIT 3",
            [],
            |r| r.get(0),
        )?;
        let tests: Vec<String> = self.list(
            "SELECT title FROM objective o WHERE done_at IS NOT NULL AND done_at+?1<=?2
               AND NOT EXISTS (SELECT 1 FROM attempt a JOIN item i ON i.id=a.item_id
                               WHERE i.objective=o.id AND a.role='test7') ORDER BY id",
            [TEST_DELAY, now],
            |r| r.get(0),
        )?;
        let (due, soon, week): (i64, i64, i64) = self
            .c
            .query_row(
                "SELECT coalesce(sum(c.due<=?1), 0), coalesce(sum(c.due>?1 AND c.due<=?1+?2), 0),
                        coalesce(sum(c.due>?1 AND c.due<=?1+7*?2), 0)
                 FROM card c JOIN item i ON i.id=c.item_id JOIN objective o ON o.id=i.objective
                 WHERE o.arm!='usual'",
                [now, DAY],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(e)?;
        let (minutes, days): (i64, i64) = self
            .c
            .query_row(
                "SELECT coalesce(sum(ended-started), 0)/60, count(DISTINCT (started+?2)/86400)
                 FROM study WHERE ended IS NOT NULL AND started>=?1",
                [today - 6 * DAY, offset],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(e)?;
        let (answered, correct): (i64, i64) = self
            .c
            .query_row(
                "SELECT count(*), coalesce(sum(correct), 0) FROM attempt WHERE role='review' AND ts>=?1",
                [now - 14 * DAY],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(e)?;
        Ok(json!({
            "today": today,
            "last": day,
            "notes": notes,
            "plan": {"due": due, "fresh": self.new_items(8)?.len(), "working": working, "tests": tests},
            "week": {"minutes": minutes, "days": days},
            "recall": {"answered": answered, "correct": correct},
            "upcoming": {"now": due, "day": soon, "week": week},
        }))
    }

    fn day(&self, start: i64, end: i64) -> Res<Value> {
        let studies: Vec<(String, String, String, i64)> = self.list(
            "SELECT s.topic, coalesce(f.mode, ''), coalesce(f.direction, ''), (s.ended-s.started)/60
             FROM study s LEFT JOIN study_focus f ON f.study_id=s.id
             WHERE s.ended IS NOT NULL AND s.started>=?1 AND s.started<?2 ORDER BY s.started",
            [start, end],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let (answered, correct): (i64, i64) = self
            .c
            .query_row(
                "SELECT count(*), coalesce(sum(correct), 0) FROM attempt WHERE ts>=?1 AND ts<?2",
                [start, end],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(e)?;
        let solved: Vec<String> = self.list(
            "SELECT title FROM challenge WHERE solved_at>=?1 AND solved_at<?2 ORDER BY solved_at",
            [start, end],
            |r| r.get(0),
        )?;
        // Факты — пояснения из карточек, на которые ты отвечал в тот день; иначе из последних повторённых.
        let mut facts = self.items(
            "SELECT i.data FROM item i WHERE i.id IN (SELECT item_id FROM attempt WHERE ts>=?1 AND ts<?2)
             ORDER BY i.ord LIMIT 2",
            params![start, end],
        )?;
        if facts.is_empty() {
            facts = self.items(
                "SELECT i.data FROM card c JOIN item i ON i.id=c.item_id ORDER BY c.last DESC LIMIT 2",
                [],
            )?;
        }
        let objective: Option<String> = self
            .c
            .query_row(
                "SELECT o.title FROM attempt a JOIN item i ON i.id=a.item_id JOIN objective o ON o.id=i.objective
                 WHERE a.ts>=?1 AND a.ts<?2 GROUP BY o.id ORDER BY count(*) DESC LIMIT 1",
                [start, end],
                |r| r.get(0),
            )
            .optional()
            .map_err(e)?;
        Ok(json!({
            "start": start,
            "studies": studies,
            "answered": answered,
            "correct": correct,
            "solved": solved,
            "objective": objective,
            "facts": facts.iter().map(|it: &Item| json!([it.prompt, it.explain])).collect::<Vec<_>>(),
        }))
    }

    fn list<T>(
        &self,
        sql: &str,
        p: impl rusqlite::Params,
        f: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>,
    ) -> Res<Vec<T>> {
        let mut st = self.c.prepare(sql).map_err(e)?;
        st.query_map(p, f)
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)
    }
}
