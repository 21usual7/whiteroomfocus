use super::{Db, Res, e};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

#[derive(Serialize)]
pub struct Mode {
    pub id: &'static str,
    pub title: &'static str,
    pub options: &'static [&'static str],
}

pub const MODES: &[Mode] = &[
    Mode {
        id: "study",
        title: "Изучение",
        options: &["Сети и протоколы", "Криптография", "Безопасность ОС"],
    },
    Mode {
        id: "writeup",
        title: "Врайтап",
        options: &["План разбора", "Описание наблюдений", "Редактура выводов"],
    },
    Mode {
        id: "bounty",
        title: "Баг-баунти",
        options: &["Правила программы", "Оценка влияния", "Подготовка отчёта"],
    },
    Mode {
        id: "hunt",
        title: "Хантинг угроз",
        options: &[
            "Формулировка гипотезы",
            "Анализ журналов",
            "Хронология событий",
        ],
    },
    Mode {
        id: "ctf",
        title: "CTF",
        options: &["Форензика", "Криптография", "Разбор результатов"],
    },
    Mode {
        id: "research",
        title: "Исследование",
        options: &["Обзор публикаций", "Сравнение источников", "Синтез выводов"],
    },
];

impl Db {
    pub fn focus_start(&self, mode: &str, option: &str, topic: &str, now: i64) -> Res<()> {
        self.focus_start_with_sources(mode, option, topic, now, false, false)
            .map(|_| ())
    }

    pub fn focus_start_with_sources(
        &self,
        mode: &str,
        option: &str,
        topic: &str,
        now: i64,
        windows: bool,
        browser: bool,
    ) -> Res<i64> {
        let selected = MODES
            .iter()
            .find(|m| m.id == mode && m.options.contains(&option))
            .ok_or("Выбери направление из текущего режима.")?;
        let tx = self.c.unchecked_transaction().map_err(e)?;
        let topic = topic.trim();
        let id = self.insert_study(
            if topic.is_empty() { option } else { topic },
            now,
            windows,
            browser,
        )?;
        tx.execute(
            "INSERT INTO study_focus(study_id, mode, direction) VALUES(?, ?, ?)",
            params![id, selected.id, option],
        )
        .map_err(e)?;
        tx.commit().map_err(e)?;
        Ok(id)
    }

    pub fn active_focus(
        &self,
    ) -> Res<Option<(i64, String, Option<(String, String)>, (i64, bool, bool))>> {
        self.c
            .query_row(
                "SELECT s.id,started,topic,mode,direction,coalesce(windows,0),coalesce(browser,0)
            FROM study s LEFT JOIN study_focus f ON s.id=f.study_id
            LEFT JOIN capture c ON s.id=c.study_id WHERE ended IS NULL ORDER BY s.id DESC LIMIT 1",
                [],
                |r| {
                    let mode = r.get::<_, Option<String>>(3)?;
                    Ok((
                        r.get(1)?,
                        r.get(2)?,
                        mode.zip(r.get(4)?),
                        (r.get(0)?, r.get(5)?, r.get(6)?),
                    ))
                },
            )
            .optional()
            .map_err(e)
    }

    pub fn focus_selection(&self, id: i64) -> Res<Option<(String, String)>> {
        self.c
            .query_row(
                "SELECT mode, direction FROM study_focus WHERE study_id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(e)
    }

    pub fn focus_history(&self) -> Res<Vec<(i64, i64, String, String, String)>> {
        let mut q = self
            .c
            .prepare(
                "SELECT started, ended, topic, mode, direction FROM study
            JOIN study_focus ON study.id=study_focus.study_id WHERE ended IS NOT NULL
            ORDER BY started DESC, study.id DESC LIMIT 5",
            )
            .map_err(e)?;
        q.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)
    }

    /// Активность по дням текущего месяца (локальное время): (дата, минуты фокуса, сессии, ответы).
    /// Незавершённая сессия считается до `now`.
    pub fn month_activity(&self, now: i64) -> Res<Vec<(String, i64, i64, i64)>> {
        let mut q = self
            .c
            .prepare(
                "WITH RECURSIVE d(day) AS (
                   SELECT date(?1, 'unixepoch', 'localtime', 'start of month')
                   UNION ALL SELECT date(day, '+1 day') FROM d
                   WHERE day < date(?1, 'unixepoch', 'localtime', 'start of month', '+1 month', '-1 day'))
                 SELECT day,
                   (SELECT coalesce(sum(max(min(coalesce(ended, ?1), ?1) - started, 0)), 0) / 60
                      FROM study WHERE date(started, 'unixepoch', 'localtime') = day),
                   (SELECT count(*) FROM study WHERE date(started, 'unixepoch', 'localtime') = day),
                   (SELECT count(*) FROM attempt WHERE date(ts, 'unixepoch', 'localtime') = day)
                 FROM d",
            )
            .map_err(e)?;
        q.query_map([now], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)
    }
}
