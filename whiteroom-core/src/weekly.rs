//! Недельная проверка: блок вопросов по цели. Показ фиксируется до выдачи вопроса, формы с ключами
//! замораживаются при создании блока, разбор выдаётся только после последнего ответа.
use crate::{Db, Item, Res, DAY, TEST_DELAY, answer, e, grade, show};

/// Срок цели: `horizon` дней после `done_at` (7 по D5, 30 — для отдельной группы целей).
const DUE: &str = "o.done_at+o.horizon*86400";
use rusqlite::{OptionalExtension, params};
use std::io::{BufRead, Write};

/// Окно допуска: ответ позже срока + 2 дня — опоздание, в «первые ответы в окне» не входит.
///
/// Правила отчёта пилота (зафиксированы до запуска с людьми):
/// - группа — `arm` цели на момент открытия блока (копируется в `test_block`); цели раздаются
///   поровну случайно при первом импорте и не перераспределяются;
/// - срок — `done_at + 7 дней`; ответ в `[срок, срок + TEST_WINDOW]` — «в окне», позже — опоздание;
/// - первый самостоятельный ответ — ответ на вопрос, показанный ровно один раз (без помощи: в блоке
///   нет подсказок); ответ после повторного показа (выход до ответа, EOF) — отдельно, не «первый»;
/// - пропуск — вопрос без ответа, когда окно уже закрылось (открытый или не начатый блок);
///   поздний ответ на него переводит его из пропусков в опоздания, но не в первые ответы;
/// - вспоминание — самостоятельные (`hint=0`) ответы роли `review`; новые сопоставимые задачи — test7.
pub const TEST_WINDOW: i64 = 2 * DAY;

/// Попытки в ещё не закрытых блоках: до разбора их правильность нигде не показывается.
pub(crate) const OPEN_TEST: &str = "SELECT x.attempt_id FROM test_exposure x JOIN test_block b ON b.id=x.block_id
     WHERE b.closed_at IS NULL AND x.attempt_id IS NOT NULL";

impl Db {
    /// Открытый блок или новый — для первой цели, срок которой настал. Формы заданий замораживаются здесь.
    fn test_block_open(&self, now: i64) -> Res<Option<i64>> {
        let open = |db: &Db| -> Res<Option<i64>> {
            db.c.query_row("SELECT id FROM test_block WHERE closed_at IS NULL ORDER BY id LIMIT 1", [], |r| r.get(0))
                .optional()
                .map_err(e)
        };
        if let Some(id) = open(self)? {
            return Ok(Some(id));
        }
        // IMMEDIATE: второй клиент ждёт записи первого, а не падает на повышении блокировки.
        let tx = rusqlite::Transaction::new_unchecked(&self.c, rusqlite::TransactionBehavior::Immediate).map_err(e)?;
        let due: Option<(String, String, i64)> = self
            .c
            .query_row(
                &format!("SELECT o.id, o.arm, {DUE} FROM objective o
                 WHERE o.done_at IS NOT NULL AND {DUE}<=?1
                   AND NOT EXISTS (SELECT 1 FROM test_block b WHERE b.objective=o.id)
                   AND EXISTS (SELECT 1 FROM item i WHERE i.objective=o.id AND i.role='test7'
                               AND i.id NOT IN (SELECT item_id FROM attempt WHERE role='test7'))
                 ORDER BY o.id LIMIT 1"),
                [now],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(e)?;
        let Some((objective, arm, due_at)) = due else {
            return Ok(None);
        };
        // OR IGNORE: второй клиент не создаёт блок повторно, а подхватывает уже созданный.
        let made = self
            .c
            .execute(
                "INSERT OR IGNORE INTO test_block(objective, arm, due_at, opened_at) VALUES (?,?,?,?)",
                params![objective, arm, due_at, now],
            )
            .map_err(e)?;
        if made > 0 {
            let block = self.c.last_insert_rowid();
            self.c
                .execute(
                    "INSERT INTO test_exposure(block_id, item_id, form, ord)
                     SELECT ?1, i.id, i.data, i.ord FROM item i
                     WHERE i.objective=?2 AND i.role='test7'
                       AND i.id NOT IN (SELECT item_id FROM attempt WHERE role='test7')",
                    params![block, objective],
                )
                .map_err(e)?;
        }
        tx.commit().map_err(e)?;
        open(self)
    }

    /// Следующий вопрос блока без ответа (по замороженной форме) и число оставшихся.
    fn test_next(&self, block: i64) -> Res<Option<(Item, i64, i64)>> {
        let left: i64 = self
            .c
            .query_row("SELECT count(*) FROM test_exposure WHERE block_id=? AND attempt_id IS NULL", [block], |r| r.get(0))
            .map_err(e)?;
        let total: i64 = self
            .c
            .query_row("SELECT count(*) FROM test_exposure WHERE block_id=?", [block], |r| r.get(0))
            .map_err(e)?;
        let form: Option<String> = self
            .c
            .query_row(
                "SELECT form FROM test_exposure WHERE block_id=? AND attempt_id IS NULL ORDER BY ord LIMIT 1",
                [block],
                |r| r.get(0),
            )
            .optional()
            .map_err(e)?;
        form.map(|f| Ok((serde_json::from_str(&f).map_err(e)?, left, total))).transpose()
    }

    /// Показ записывается до выдачи вопроса в интерфейс.
    fn test_shown(&self, block: i64, item: &str, now: i64) -> Res<()> {
        self.c
            .execute(
                "UPDATE test_exposure SET shown_at=coalesce(shown_at, ?3), shows=shows+1 WHERE block_id=?1 AND item_id=?2",
                params![block, item, now],
            )
            .map_err(e)?;
        Ok(())
    }

    /// Ответ засчитывается один раз и только на показанный вопрос; оценивается по замороженной форме.
    /// Ответ на вопрос, показанный повторно, не считается первым.
    fn test_answer(&self, block: i64, item: &str, ans: &str, ms: i64, now: i64) -> Res<()> {
        let tx = rusqlite::Transaction::new_unchecked(&self.c, rusqlite::TransactionBehavior::Immediate).map_err(e)?;
        let form: Option<String> = self
            .c
            .query_row(
                "SELECT form FROM test_exposure WHERE block_id=? AND item_id=? AND attempt_id IS NULL AND shown_at IS NOT NULL",
                params![block, item],
                |r| r.get(0),
            )
            .optional()
            .map_err(e)?;
        let Some(form) = form else {
            return Err("На этот вопрос ответ уже записан или он ещё не показан.".into());
        };
        let it: Item = serde_json::from_str(&form).map_err(e)?;
        self.record(None, &it, "test7", ans, grade(&it, ans), false, ms, now)?;
        self.c
            .execute(
                "UPDATE test_exposure SET attempt_id=?3, first=(shows=1) WHERE block_id=?1 AND item_id=?2",
                params![block, item, self.c.last_insert_rowid()],
            )
            .map_err(e)?;
        tx.commit().map_err(e)
    }

    /// Для GUI: следующий вопрос недельной проверки без ключа и пояснения, или `null`.
    /// Каждый вызов — показ (записывается до возврата); повторный вызов без ответа — повторный показ.
    /// `{"block", "objective", "left", "total", "resumed", "item": {"id", "kind", "prompt", "code"}}`
    pub fn test_current(&self, now: i64) -> Res<serde_json::Value> {
        while let Some(block) = self.test_block_open(now)? {
            let Some((it, left, total)) = self.test_next(block)? else {
                self.test_close(block, now)?;
                continue;
            };
            let resumed: bool = self
                .c
                .query_row("SELECT count(*)>0 FROM test_exposure WHERE block_id=? AND shown_at IS NOT NULL", [block], |r| r.get(0))
                .map_err(e)?;
            self.test_shown(block, &it.id, now)?;
            return Ok(serde_json::json!({
                "block": block, "objective": it.objective, "left": left, "total": total, "resumed": resumed,
                "item": {"id": it.id, "kind": it.kind, "prompt": it.prompt, "code": it.code},
            }));
        }
        Ok(serde_json::Value::Null)
    }

    /// Для GUI: ответ на показанный вопрос. Пока блок открыт — `{"closed": false, "left", "total"}`
    /// без правильности; после последнего ответа — `{"closed": true, "correct", "total", "review": [...]}`
    /// с ключами и пояснениями. Повторная отправка того же ответа — ошибка, второй записи нет.
    pub fn test_submit(&self, block: i64, item: &str, ans: &str, ms: i64, now: i64) -> Res<serde_json::Value> {
        self.test_answer(block, item, ans, ms, now)?;
        if let Some((_, left, total)) = self.test_next(block)? {
            return Ok(serde_json::json!({"closed": false, "left": left, "total": total}));
        }
        let res = self.test_close(block, now)?;
        let review: Vec<_> = res
            .iter()
            .map(|(it, a, ok)| {
                serde_json::json!({"id": it.id, "prompt": it.prompt, "answer": a, "correct": ok,
                                   "key": it.answer[0], "explain": it.explain})
            })
            .collect();
        Ok(serde_json::json!({"closed": true, "correct": res.iter().filter(|r| r.2).count(), "total": res.len(), "review": review}))
    }

    /// Закрывает блок, если ответы есть на все вопросы; возвращает данные для разбора.
    fn test_close(&self, block: i64, now: i64) -> Res<Vec<(Item, String, bool)>> {
        self.c
            .execute(
                "UPDATE test_block SET closed_at=?2 WHERE id=?1 AND closed_at IS NULL
                 AND NOT EXISTS (SELECT 1 FROM test_exposure WHERE block_id=?1 AND attempt_id IS NULL)",
                params![block, now],
            )
            .map_err(e)?;
        let mut st = self
            .c
            .prepare(
                "SELECT x.form, a.answer, a.correct FROM test_exposure x JOIN attempt a ON a.id=x.attempt_id
                 JOIN test_block b ON b.id=x.block_id WHERE x.block_id=? AND b.closed_at IS NOT NULL ORDER BY x.ord",
            )
            .map_err(e)?;
        let rows = st
            .query_map([block], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?)))
            .map_err(e)?;
        rows.map(|r| {
            let (form, a, ok) = r.map_err(e)?;
            Ok((serde_json::from_str(&form).map_err(e)?, a, ok))
        })
        .collect()
    }

    /// Вопросы, на которые ещё нет ответа: незакрытые блоки (замороженные формы) и цели без блока.
    pub fn tests_due(&self, now: i64) -> Res<Vec<Item>> {
        let mut v = self.items(
            "SELECT x.form FROM test_exposure x JOIN test_block b ON b.id=x.block_id
             WHERE b.closed_at IS NULL AND x.attempt_id IS NULL ORDER BY b.id, x.ord",
            [],
        )?;
        v.extend(self.items(
            &format!("SELECT i.data FROM item i JOIN objective o ON o.id=i.objective
             WHERE i.role='test7' AND o.done_at IS NOT NULL AND {DUE}<=?
               AND NOT EXISTS (SELECT 1 FROM test_block b WHERE b.objective=o.id)
               AND i.id NOT IN (SELECT item_id FROM attempt WHERE role='test7') ORDER BY o.id, i.ord"),
            [now],
        )?);
        Ok(v)
    }
}

pub fn run_test(db: &Db, inp: &mut impl BufRead, out: &mut impl Write, now: i64) -> Res<()> {
    let mut any = false;
    while let Some(block) = db.test_block_open(now)? {
        let Some((first, left, total)) = db.test_next(block)? else {
            db.test_close(block, now)?;
            continue;
        };
        if any {
            writeln!(out).map_err(e)?;
        }
        any = true;
        if left < total {
            writeln!(out, "Продолжаем начатый блок: осталось {left} из {total}. Без подсказок.").map_err(e)?;
        } else {
            writeln!(out, "Проверка: {total} вопросов. Без подсказок, разбор после последнего ответа.").map_err(e)?;
        }
        let mut next = Some((first, left, total));
        while let Some((it, left, total)) = next {
            writeln!(out, "\n── {}/{total}", total - left + 1).map_err(e)?;
            db.test_shown(block, &it.id, now)?;
            show(out, &it)?;
            let Some((a, _, ms)) = answer(inp, out, &it, false)? else {
                writeln!(out, "\nБлок не закончен: осталось {left} из {total}. Продолжи: whiteroom test. Разбор появится после последнего ответа.").map_err(e)?;
                return Ok(());
            };
            db.test_answer(block, &it.id, &a, ms, now)?;
            next = db.test_next(block)?;
        }
        let res = db.test_close(block, now)?;
        writeln!(out, "\n══ Разбор").map_err(e)?;
        for (it, a, _) in res.iter().filter(|r| !r.2) {
            writeln!(out, "✗ {} [{}]: твой ответ «{a}», верно {}. {}", it.id, it.objective, it.answer[0], it.explain).map_err(e)?;
        }
        let ok = res.iter().filter(|r| r.2).count();
        writeln!(out, "Итог: {ok}/{}.", res.len()).map_err(e)?;
    }
    if !any {
        writeln!(out, "Проверок пока нет. Срок смотри в whiteroom status.").map_err(e)?;
    }
    Ok(())
}

impl Db {
    /// Отчёт пилота по группам. Первый самостоятельный ответ в окне считается отдельно от
    /// повторных показов и опозданий; пропуск — вопрос без ответа после окна допуска.
    pub fn pilot_report(&self, now: i64) -> Res<serde_json::Value> {
        let arms: Vec<String> = self.list("SELECT DISTINCT arm FROM objective ORDER BY arm", [], |r| r.get(0))?;
        let mut groups = Vec::new();
        for arm in arms {
            let n = |sql: &str| -> Res<i64> {
                let mut st = self.c.prepare(sql).map_err(e)?;
                let all: [&dyn rusqlite::ToSql; 3] = [&arm, &now, &TEST_WINDOW];
                st.query_row(&all[..st.parameter_count()], |r| r.get(0)).map_err(e)
            };
            let joined = "FROM test_exposure x JOIN test_block b ON b.id=x.block_id LEFT JOIN attempt a ON a.id=x.attempt_id
                          WHERE b.arm=?1";
            // Пока блок открыт и окно не прошло, правильность не раскрывается: такие ответы — `pending`.
            let settled = "(b.closed_at IS NOT NULL OR b.due_at+?3<?2)";
            let first = n(&format!("SELECT count(*) {joined} AND x.first=1 AND a.ts<=b.due_at+?3 AND {settled}"))?;
            let first_ok = n(&format!("SELECT coalesce(sum(a.correct),0) {joined} AND x.first=1 AND a.ts<=b.due_at+?3 AND {settled}"))?;
            let pending = n(&format!("SELECT count(*) {joined} AND a.id IS NOT NULL AND NOT {settled}"))?;
            let late = n(&format!("SELECT count(*) {joined} AND a.ts>b.due_at+?3"))?;
            let repeat = n(&format!("SELECT count(*) {joined} AND x.first=0 AND a.ts<=b.due_at+?3"))?;
            let reshows = n(&format!("SELECT coalesce(sum(max(x.shows-1,0)),0) {joined}"))?;
            let unanswered = n(&format!("SELECT count(*) {joined} AND x.attempt_id IS NULL AND x.shown_at IS NOT NULL"))?;
            let missed_blocks = n(&format!("SELECT count(*) {joined} AND x.attempt_id IS NULL AND b.due_at+?3<?2"))?;
            let missed_unopened = n(&format!(
                "SELECT count(*) FROM item i JOIN objective o ON o.id=i.objective
                 WHERE o.arm=?1 AND i.role='test7' AND o.done_at IS NOT NULL AND {DUE}+?3<?2
                   AND NOT EXISTS (SELECT 1 FROM test_block b WHERE b.objective=o.id)
                   AND i.id NOT IN (SELECT item_id FROM attempt WHERE role='test7')"),
            )?;
            let delay_h: Option<f64> =
                self.c
                    .query_row(&format!("SELECT avg(a.ts-b.due_at)/3600.0 {joined} AND a.id IS NOT NULL"), params![arm], |r| r.get(0))
                    .map_err(e)?;
            let planned = n("SELECT count(*) FROM item i JOIN objective o ON o.id=i.objective
                             WHERE o.arm=?1 AND i.role='test7' AND o.done_at IS NOT NULL")?;
            let (recall, recall_ok): (i64, i64) = self
                .c
                .query_row(
                    "SELECT count(*), coalesce(sum(a.correct),0) FROM attempt a JOIN item i ON i.id=a.item_id
                     JOIN objective o ON o.id=i.objective WHERE o.arm=?1 AND a.role='review' AND a.hint=0",
                    [&arm],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(e)?;
            // Облегчённые ответы (после подсказки) — отдельно: в вспоминание и проверку не входят.
            let helped = n("SELECT count(*) FROM attempt a JOIN item i ON i.id=a.item_id JOIN objective o ON o.id=i.objective
                            WHERE o.arm=?1 AND a.hint=1")?;
            let (goals, goals_done): (i64, i64) = self
                .c
                .query_row(
                    "SELECT count(*), count(done_at) FROM objective WHERE arm=?1",
                    [&arm],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(e)?;
            let (app_min, outside_min): (i64, i64) = self
                .c
                .query_row(
                    "SELECT coalesce((SELECT sum(a.ms) FROM attempt a JOIN item i ON i.id=a.item_id JOIN objective o ON o.id=i.objective
                                      WHERE o.arm=?1 AND a.role!='test7'),0)/60000, coalesce(sum(outside_min),0)
                     FROM objective WHERE arm=?1",
                    [&arm],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(e)?;
            let legacy = n("SELECT count(*) FROM attempt a JOIN item i ON i.id=a.item_id JOIN objective o ON o.id=i.objective
                            WHERE o.arm=?1 AND a.role='test7'
                              AND a.id NOT IN (SELECT attempt_id FROM test_exposure WHERE attempt_id IS NOT NULL)")?;
            groups.push(serde_json::json!({
                "arm": arm, "goals": goals, "goals_done": goals_done, "planned_checks": planned,
                "first_answers": first, "first_correct": first_ok, "pending_open_block": pending,
                "late": late, "repeat_answers": repeat, "reshows": reshows,
                "shown_unanswered": unanswered, "missed": missed_blocks + missed_unopened,
                "avg_delay_hours": delay_h.map(|h| (h * 10.0).round() / 10.0),
                "recall_independent": recall, "recall_correct": recall_ok, "helped_answers": helped,
                "study_minutes_in_app": app_min, "study_minutes_outside": outside_min,
                "incomplete_legacy_answers": legacy,
            }));
        }
        Ok(serde_json::json!({"window_hours": TEST_WINDOW / 3600, "delay_days": TEST_DELAY / DAY, "groups": groups}))
    }

    /// Текст отчёта: у каждой доли числитель и знаменатель, пропуски и опоздания отдельно.
    pub fn pilot_text(&self, now: i64) -> Res<String> {
        let r = self.pilot_report(now)?;
        let mut out = format!(
            "Отчёт пилота. Проверка через {} дн.; окно допуска {} ч после срока. Пропуск = нет ответа после окна.\n",
            r["delay_days"], r["window_hours"]
        );
        for g in r["groups"].as_array().into_iter().flatten() {
            let delay = g["avg_delay_hours"].as_f64().map_or("нет ответов".to_string(), |h| format!("{h} ч"));
            out += &format!(
                "\n{}: целей {} (завершено {}), проверок запланировано {}\n  новые сопоставимые задачи (test7), первые ответы в окне: {}/{} верно (ещё {} в открытом блоке — после разбора); опозданий {}; после повторного показа {}; повторных показов {}; показано без ответа {}; пропусков {}\n  средняя задержка после срока: {delay}\n  вспоминание (самостоятельное): {}/{}; ответов после подсказки (не в зачёте): {}\n  учебное время: {} мин в приложении, {} мин вне по самоотчёту (сравнивать группы по самоотчёту; время в приложении измеряется только в wr)\n  без данных о первом показе: {}\n",
                g["arm"].as_str().unwrap_or("?"), g["goals"], g["goals_done"], g["planned_checks"],
                g["first_correct"], g["first_answers"], g["pending_open_block"], g["late"], g["repeat_answers"], g["reshows"],
                g["shown_unanswered"], g["missed"], g["recall_correct"], g["recall_independent"], g["helped_answers"],
                g["study_minutes_in_app"], g["study_minutes_outside"], g["incomplete_legacy_answers"],
            );
        }
        Ok(out)
    }

    /// Обезличенная выгрузка для анализа: без ключей, текстов ответов и заданий, журналов окон и страниц.
    /// `correct` скрыт (null), пока блок открыт и окно не прошло, как и в отчёте.
    pub fn pilot_export(&self, now: i64) -> Res<serde_json::Value> {
        let rows = self.list(
            "SELECT b.arm, b.objective, x.item_id, b.due_at, x.shown_at, x.shows, x.first, a.ts,
               CASE WHEN b.closed_at IS NOT NULL OR b.due_at+?2<?1 THEN a.correct END, a.ms
             FROM test_exposure x JOIN test_block b ON b.id=x.block_id LEFT JOIN attempt a ON a.id=x.attempt_id
             ORDER BY b.id, x.ord",
            [now, TEST_WINDOW],
            |r| {
                Ok(serde_json::json!({
                    "arm": r.get::<_, String>(0)?, "objective": r.get::<_, String>(1)?, "item": r.get::<_, String>(2)?,
                    "due_at": r.get::<_, i64>(3)?, "shown_at": r.get::<_, Option<i64>>(4)?, "shows": r.get::<_, i64>(5)?,
                    "first": r.get::<_, Option<bool>>(6)?, "answered_at": r.get::<_, Option<i64>>(7)?,
                    "correct": r.get::<_, Option<bool>>(8)?, "ms": r.get::<_, Option<i64>>(9)?,
                }))
            },
        )?;
        // Цели — чтобы пересчитать запланированные проверки и пропуски не начатых блоков.
        let objectives = self.list(
            "SELECT o.arm, o.id, o.done_at, (SELECT count(*) FROM item i WHERE i.objective=o.id AND i.role='test7')
             FROM objective o WHERE o.arm IN ('wr','usual') ORDER BY o.id",
            [],
            |r| {
                Ok(serde_json::json!({"arm": r.get::<_, String>(0)?, "objective": r.get::<_, String>(1)?,
                    "done_at": r.get::<_, Option<i64>>(2)?, "test_items": r.get::<_, i64>(3)?}))
            },
        )?;
        let recall = self.list(
            "SELECT o.arm, o.id, a.item_id, a.role, a.ts, a.correct, a.hint, a.session_id FROM attempt a
             JOIN item i ON i.id=a.item_id JOIN objective o ON o.id=i.objective
             WHERE o.arm IN ('wr','usual') AND a.role IN ('new','review','retry') ORDER BY a.id",
            [],
            |r| {
                Ok(serde_json::json!({"arm": r.get::<_, String>(0)?, "objective": r.get::<_, String>(1)?,
                    "item": r.get::<_, String>(2)?, "role": r.get::<_, String>(3)?, "at": r.get::<_, i64>(4)?,
                    "correct": r.get::<_, bool>(5)?, "helped": r.get::<_, bool>(6)?, "session": r.get::<_, Option<i64>>(7)?}))
            },
        )?;
        let sessions = self.list(
            "SELECT id, planned, predicted, correct FROM session WHERE ended IS NOT NULL ORDER BY id",
            [],
            |r| {
                Ok(serde_json::json!({"session": r.get::<_, i64>(0)?, "planned": r.get::<_, Option<i64>>(1)?,
                    "predicted": r.get::<_, Option<i64>>(2)?, "correct_independent": r.get::<_, Option<i64>>(3)?}))
            },
        )?;
        Ok(serde_json::json!({"window_hours": TEST_WINDOW / 3600, "delay_days": TEST_DELAY / DAY,
            "objectives": objectives, "checks": rows, "practice": recall, "sessions": sessions}))
    }
}
