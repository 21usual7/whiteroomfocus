//! WHITE ROOM 1.0: цели, план, учебный блок с возобновлением, экраны «Сегодня», «Прогресс»
//! и «Что WHITE ROOM видит сейчас», резервная копия и редактор модулей. CLI и окно вызывают
//! одни и те же функции и получают JSON.
use crate::{DAY, Db, Item, Module, Res, SCHEMA, e, grade, grading};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// Встроенные траектории по доменам 1.0: работают офлайн без импорта, выбор темы на «Сегодня».
pub const DOMAINS: [(&str, &str, &str); 6] = [
    ("cyber", "Кибербезопасность", include_str!("../../modules/log-analysis.toml")),
    ("math", "Математика", include_str!("../../modules/math-derivatives.toml")),
    ("code", "Программирование", include_str!("../../modules/code-python.toml")),
    ("german", "Немецкий", include_str!("../../modules/german-a2.toml")),
    ("research", "Исследования", include_str!("../../modules/research-sources.toml")),
    ("custom", "Своя тема", include_str!("../../modules/template.toml")),
];

/// Оценка длительности шага, минут: по ней план укладывается в длительность блока.
const MIN_QUESTION: i64 = 2;
const MIN_REVIEW: i64 = 1;
const BREAK_MIN: i64 = 5;
/// Работа без перерыва, минут; перерыв предлагается, его можно убрать из плана или пропустить.
const WORK_STRETCH: i64 = 25;
/// Меньше стольких наблюдений — показатель помечается «мало данных».
pub const LOW_N: i64 = 5;

pub(crate) fn migrate(c: &mut Connection) -> Res<()> {
    let v: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(e)?;
    if v >= 2 {
        return Ok(());
    }
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(e)?;
    if tx.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).map_err(e)? >= 2 {
        return Ok(());
    }
    tx.execute_batch(
        "ALTER TABLE objective ADD COLUMN module TEXT;
         ALTER TABLE objective ADD COLUMN horizon INT NOT NULL DEFAULT 7;
         ALTER TABLE attempt ADD COLUMN graded TEXT NOT NULL DEFAULT 'auto';
         ALTER TABLE attempt ADD COLUMN p_recall REAL;
         CREATE TABLE item_version(id TEXT NOT NULL, v INT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(id, v));
         INSERT OR IGNORE INTO item_version SELECT id, v, data FROM item;
         CREATE TABLE module(id TEXT PRIMARY KEY, title TEXT NOT NULL, domain TEXT NOT NULL, loaded INT NOT NULL);
         CREATE TABLE module_rev(id TEXT NOT NULL, rev INTEGER NOT NULL, src TEXT NOT NULL, ts INT NOT NULL,
           PRIMARY KEY(id, rev));
         CREATE TABLE prereq(skill TEXT NOT NULL, needs TEXT NOT NULL, PRIMARY KEY(skill, needs));
         CREATE TABLE goal(id INTEGER PRIMARY KEY, module TEXT NOT NULL, title TEXT NOT NULL, skills TEXT NOT NULL,
           materials TEXT NOT NULL DEFAULT '[]', deadline INT, block_min INT NOT NULL DEFAULT 25,
           created INT NOT NULL, updated INT NOT NULL, archived INT);
         CREATE TABLE learn_block(id INTEGER PRIMARY KEY, goal_id INT NOT NULL REFERENCES goal(id),
           session_id INT NOT NULL REFERENCES session(id), created INT NOT NULL, plan TEXT NOT NULL,
           edited INT NOT NULL DEFAULT 0, hints TEXT NOT NULL DEFAULT '[]', note TEXT NOT NULL DEFAULT '',
           thoughts TEXT NOT NULL DEFAULT '[]', energy INT, reflection TEXT, updated INT NOT NULL, closed_at INT);
         CREATE UNIQUE INDEX learn_block_open ON learn_block(closed_at IS NULL) WHERE closed_at IS NULL;
         CREATE TABLE block_step(block_id INT NOT NULL REFERENCES learn_block(id), step INT NOT NULL,
           status TEXT NOT NULL, attempt_id INT REFERENCES attempt(id), ts INT NOT NULL, PRIMARY KEY(block_id, step));
         PRAGMA user_version=2;",
    )
    .map_err(e)?;
    // Старые модули — кибербезопасность (D1): цели привязываются к модулю по префиксу задания.
    tx.execute_batch(
        "INSERT OR IGNORE INTO module(id, title, domain, loaded)
           SELECT DISTINCT 'legacy', 'Модули до 1.0', 'cyber', 0 FROM objective WHERE arm IN ('wr','usual');
         UPDATE objective SET module='legacy' WHERE module IS NULL AND arm IN ('wr','usual');",
    )
    .map_err(e)?;
    tx.commit().map_err(e)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Step {
    /// diag, review, new, check, break.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    pub min: i64,
    /// Причина выбора, показывается рядом с шагом.
    #[serde(default)]
    pub why: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Material {
    pub label: String,
    /// Ссылка https://… или путь к локальному файлу.
    pub src: String,
    /// Что из материала используется: «глава 2», «весь файл».
    pub scope: String,
}

#[derive(Deserialize)]
pub struct GoalEdit {
    pub title: String,
    pub skills: Vec<String>,
    #[serde(default)]
    pub materials: Vec<Material>,
    pub deadline: Option<i64>,
    pub block_min: i64,
}

fn js<T: Serialize>(v: &T) -> Res<String> {
    serde_json::to_string(v).map_err(e)
}

fn tx(db: &Db) -> Res<rusqlite::Transaction<'_>> {
    rusqlite::Transaction::new_unchecked(&db.c, rusqlite::TransactionBehavior::Immediate).map_err(e)
}

impl Db {
    /// Импорт модуля без распределения по группам пилота: цели получают группу `goal`.
    /// Цели модуля, уже распределённые в пилот (`wr`, `usual`), остаются в своей группе.
    pub fn module_put(&self, m: &Module, now: i64) -> Res<()> {
        let t = tx(self)?;
        for o in &m.objective {
            self.c
                .execute(
                    "INSERT OR IGNORE INTO objective(id, title, arm, assigned_at) VALUES (?,?,'goal',?)",
                    params![o.id, o.title, now],
                )
                .map_err(e)?;
        }
        self.load_known(m, now)?;
        let src = js(m)?;
        let last: Option<String> = self
            .c
            .query_row("SELECT src FROM module_rev WHERE id=? ORDER BY rev DESC LIMIT 1", [&m.id], |r| r.get(0))
            .optional()
            .map_err(e)?;
        if last.as_deref() != Some(src.as_str()) {
            self.c
                .execute(
                    "INSERT INTO module_rev(id, rev, src, ts) VALUES (?1, (SELECT coalesce(max(rev),0)+1 FROM module_rev WHERE id=?1), ?2, ?3)",
                    params![m.id, src, now],
                )
                .map_err(e)?;
        }
        t.commit().map_err(e)
    }

    /// Что изменит импорт: новые, изменённые (с числом попыток — они остаются при своей версии),
    /// исключённые из файла (остаются в истории) и цели, уже распределённые в пилот.
    pub fn module_preview(&self, m: &Module) -> Res<Value> {
        let (mut added, mut changed, mut same) = (vec![], vec![], 0);
        for it in &m.item {
            let cur: Option<(i64, String)> = self
                .c
                .query_row("SELECT v, data FROM item WHERE id=?", [&it.id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()
                .map_err(e)?;
            match cur {
                None => added.push(json!({"id": it.id, "prompt": it.prompt})),
                Some((v, data)) => {
                    let old: Item = serde_json::from_str(&data).map_err(e)?;
                    if js(&Item { v: it.v.max(v), ..old })? == js(&Item { v: it.v.max(v), ..it.clone() })? {
                        same += 1;
                        continue;
                    }
                    let n: i64 = self
                        .c
                        .query_row("SELECT count(*) FROM attempt WHERE item_id=?", [&it.id], |r| r.get(0))
                        .map_err(e)?;
                    changed.push(json!({"id": it.id, "prompt": it.prompt, "from_v": v, "attempts": n}));
                }
            }
        }
        let ids: HashSet<_> = m.item.iter().map(|i| i.id.as_str()).collect();
        let objs: Vec<&str> = m.objective.iter().map(|o| o.id.as_str()).collect();
        let mut removed = vec![];
        for o in &objs {
            for id in self.list("SELECT id FROM item WHERE objective=?", [o], |r| r.get::<_, String>(0))? {
                if !ids.contains(id.as_str()) {
                    removed.push(id);
                }
            }
        }
        let pilot = self.list(
            &format!(
                "SELECT id FROM objective WHERE arm IN ('wr','usual') AND id IN ({})",
                objs.iter().map(|_| "?").collect::<Vec<_>>().join(",")
            ),
            rusqlite::params_from_iter(&objs),
            |r| r.get::<_, String>(0),
        )?;
        Ok(json!({"module": m.id, "added": added, "changed": changed, "unchanged": same,
                  "removed_kept_in_history": removed, "pilot_objectives": pilot}))
    }

    pub fn modules(&self) -> Res<Value> {
        Ok(self
            .list(
                "SELECT m.id, m.title, m.domain, (SELECT count(*) FROM objective o WHERE o.module=m.id),
                   (SELECT count(*) FROM module_rev r WHERE r.id=m.id) FROM module m ORDER BY m.domain, m.id",
                [],
                |r| {
                    Ok(json!({"id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?, "domain": r.get::<_, String>(2)?,
                              "skills": r.get::<_, i64>(3)?, "revisions": r.get::<_, i64>(4)?}))
                },
            )?
            .into())
    }

    /// Темы для выбора цели: встроенные траектории по доменам и число активных целей по каждой.
    pub fn domains(&self) -> Res<Value> {
        let mut out = vec![];
        for (domain, label, src) in DOMAINS {
            let m = self.domain_module(src)?;
            let goals: i64 = self
                .c
                .query_row("SELECT count(*) FROM goal WHERE module=? AND archived IS NULL", [&m.id], |r| r.get(0))
                .map_err(e)?;
            out.push(json!({"domain": domain, "label": label, "module": m.id, "title": m.title,
                            "skills": m.objective.iter().map(|o| o.title.as_str()).collect::<Vec<_>>(), "goals": goals}));
        }
        Ok(out.into())
    }

    /// Цель по теме. Модуль, уже сохранённый в базе, берётся в своей последней редакции — правки не теряются.
    pub fn goal_from_domain(&self, domain: &str, now: i64) -> Res<i64> {
        let (_, _, src) = DOMAINS.iter().find(|d| d.0 == domain).ok_or(format!("темы «{domain}» нет"))?;
        self.goal_create(&self.domain_module(src)?, now)
    }

    fn domain_module(&self, src: &str) -> Res<Module> {
        let m = Module::parse(src)?;
        Ok(self.module_src(&m.id).unwrap_or(m))
    }

    /// Последняя сохранённая редакция модуля (JSON для редактора окна).
    pub fn module_src(&self, id: &str) -> Res<Module> {
        let src: String = self
            .c
            .query_row("SELECT src FROM module_rev WHERE id=? ORDER BY rev DESC LIMIT 1", [id], |r| r.get(0))
            .map_err(|_| format!("модуля {id} нет в редакторе: импортируй его файл"))?;
        serde_json::from_str(&src).map_err(e)
    }

    /// Отмена последнего сохранения: предыдущая редакция импортируется как новая, история не теряется.
    pub fn module_undo(&self, id: &str, now: i64) -> Res<()> {
        let revs = self.list("SELECT src FROM module_rev WHERE id=? ORDER BY rev DESC LIMIT 2", [id], |r| {
            r.get::<_, String>(0)
        })?;
        let prev = revs.get(1).ok_or("отменять нечего: у модуля одна редакция")?;
        let mut m: Module = serde_json::from_str(prev).map_err(e)?;
        Module::check(&mut m)?;
        self.module_put(&m, now)
    }

    /// Цель из модуля: навыки — цели модуля в порядке файла с учётом предпосылок.
    pub fn goal_create(&self, m: &Module, now: i64) -> Res<i64> {
        self.module_put(m, now)?;
        let usual: Vec<String> = m
            .objective
            .iter()
            .filter(|o| self.arm(&o.id).ok().flatten().as_deref() == Some("usual"))
            .map(|o| o.id.clone())
            .collect();
        let skills: Vec<String> = m.objective.iter().map(|o| o.id.clone()).filter(|o| !usual.contains(o)).collect();
        if skills.is_empty() {
            return Err("все навыки модуля в пилоте идут в группе «учишь сам»: для цели WHITE ROOM их нет".into());
        }
        self.c
            .execute(
                "INSERT INTO goal(module, title, skills, created, updated) VALUES (?,?,?,?,?)",
                params![m.id, m.title, js(&skills)?, now, now],
            )
            .map_err(e)?;
        Ok(self.c.last_insert_rowid())
    }

    pub fn goal_update(&self, id: i64, g: &GoalEdit, now: i64) -> Res<()> {
        let module: String = self
            .c
            .query_row("SELECT module FROM goal WHERE id=?", [id], |r| r.get(0))
            .map_err(|_| "цели нет")?;
        if g.title.trim().is_empty() {
            return Err("у цели должно быть название".into());
        }
        if !(10..=180).contains(&g.block_min) {
            return Err("длительность блока — от 10 до 180 минут".into());
        }
        let known: HashSet<String> = self
            .list("SELECT id FROM objective WHERE module=? AND arm!='usual'", [&module], |r| r.get(0))?
            .into_iter()
            .collect();
        if g.skills.is_empty() || g.skills.iter().any(|s| !known.contains(s)) || g.skills.iter().collect::<HashSet<_>>().len() != g.skills.len() {
            return Err("навыки — без повторов и только из модуля цели".into());
        }
        for m in &g.materials {
            let src = m.src.trim();
            let ok = src.starts_with("https://") || src.starts_with("http://") || std::path::Path::new(src).is_absolute();
            if m.label.trim().is_empty() || m.scope.trim().is_empty() || !ok {
                return Err(format!("материал «{}»: нужны название, область использования и ссылка http(s) или абсолютный путь", m.label));
            }
        }
        self.c
            .execute(
                "UPDATE goal SET title=?, skills=?, materials=?, deadline=?, block_min=?, updated=? WHERE id=?",
                params![g.title.trim(), js(&g.skills)?, js(&g.materials)?, g.deadline, g.block_min, now, id],
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn goal_archive(&self, id: i64, now: i64) -> Res<()> {
        self.c.execute("UPDATE goal SET archived=? WHERE id=?", params![now, id]).map_err(e)?;
        Ok(())
    }

    /// Предпосылки навыка (редактируемый граф). Цикл отклоняется.
    pub fn prereq_set(&self, skill: &str, needs: &[String]) -> Res<()> {
        let mut g: HashMap<String, Vec<String>> = HashMap::new();
        for (s, n) in self.list("SELECT skill, needs FROM prereq WHERE skill!=?", [skill], |r| Ok((r.get(0)?, r.get(1)?)))? {
            g.entry(s).or_default().push(n);
        }
        g.insert(skill.into(), needs.to_vec());
        let mut stack: Vec<String> = needs.to_vec();
        let mut seen = HashSet::new();
        while let Some(x) = stack.pop() {
            if x == skill {
                return Err("предпосылки образуют цикл".into());
            }
            if seen.insert(x.clone()) {
                stack.extend(g.get(&x).cloned().unwrap_or_default());
            }
        }
        let t = tx(self)?;
        self.c.execute("DELETE FROM prereq WHERE skill=?", [skill]).map_err(e)?;
        for n in needs {
            self.c.execute("INSERT INTO prereq(skill, needs) VALUES (?,?)", params![skill, n]).map_err(e)?;
        }
        t.commit().map_err(e)
    }

    fn goal_skills(&self, goal: i64) -> Res<(Vec<String>, i64)> {
        let (skills, min): (String, i64) = self
            .c
            .query_row("SELECT skills, block_min FROM goal WHERE id=?", [goal], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| "цели нет")?;
        let order: Vec<String> = serde_json::from_str(&skills).map_err(e)?;
        // Порядок пользователя, но навык идёт после своих предпосылок из той же цели.
        let needs: Vec<(String, String)> = self.list("SELECT skill, needs FROM prereq", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut out: Vec<String> = vec![];
        fn visit(s: &str, order: &[String], needs: &[(String, String)], out: &mut Vec<String>, depth: usize) {
            if out.iter().any(|o| o == s) || depth > order.len() {
                return;
            }
            for (_, n) in needs.iter().filter(|(k, n)| k == s && order.contains(n)) {
                visit(n, order, needs, out, depth + 1);
            }
            if !out.iter().any(|o| o == s) {
                out.push(s.to_string());
            }
        }
        for s in &order {
            visit(s, &order, &needs, &mut out, 0);
        }
        Ok((out, min))
    }

    /// Навык готов к работе без разобранного примера: диагностика по нему пройдена верно.
    fn prepared(&self, skill: &str) -> Res<bool> {
        self.c
            .query_row(
                "SELECT coalesce(min(a.correct),0) FROM attempt a JOIN item i ON i.id=a.item_id
                 WHERE i.objective=? AND a.role='diag'",
                [skill],
                |r| r.get(0),
            )
            .map_err(e)
    }

    /// Воспроизводимый план: диагностика → повторения по сроку → новая работа по навыкам →
    /// перерывы → итоговая проверка на переносе (задания разных навыков чередуются).
    pub fn plan_build(&self, goal: i64, now: i64) -> Res<Vec<Step>> {
        let (skills, budget) = self.goal_skills(goal)?;
        let title = |id: &str| -> String {
            self.c.query_row("SELECT title FROM objective WHERE id=?", [id], |r| r.get(0)).unwrap_or_else(|_| id.to_string())
        };
        let mut plan = vec![];
        for s in &skills {
            for it in self.items("SELECT data FROM item WHERE objective=? AND role='diag' AND id NOT IN (SELECT item_id FROM attempt WHERE role='diag') ORDER BY ord", [s])? {
                plan.push(Step { kind: "diag".into(), item: Some(it.id), min: MIN_QUESTION,
                    why: format!("входная диагностика «{}»: по ней план решит, нужен ли разобранный пример", title(s)) });
            }
        }
        let ph = skills.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let rows: Vec<(String, i64)> = self.list(
            &format!("SELECT c.item_id, c.due FROM card c JOIN item i ON i.id=c.item_id WHERE i.objective IN ({ph}) AND c.due<=? ORDER BY c.due, c.item_id LIMIT 20"),
            rusqlite::params_from_iter(skills.iter().map(|s| s as &dyn rusqlite::ToSql).chain([&now as &dyn rusqlite::ToSql])),
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        for (id, due) in rows {
            let late = (now - due) / DAY;
            plan.push(Step { kind: "review".into(), item: Some(id), min: MIN_REVIEW,
                why: if late > 0 { format!("повторение: срок прошёл {late} дн. назад, вспоминание ниже цели 90%") }
                     else { "повторение: сегодня прогноз вспоминания опустился до 90%".into() } });
        }
        let mut used: i64 = plan.iter().map(|s| s.min).sum();
        let mut checks: Vec<Vec<Step>> = vec![];
        for s in &skills {
            let prepared = self.prepared(s)?;
            let fresh = self.items(
                "SELECT data FROM item WHERE objective=? AND role='practice' AND id NOT IN (SELECT item_id FROM card) ORDER BY ord",
                [s],
            )?;
            let mut chk = vec![];
            for it in fresh {
                let stage = it.stage.clone().unwrap_or_default();
                if prepared && (stage == "worked" || stage == "faded") {
                    continue;
                }
                if stage == "transfer" {
                    chk.push(Step { kind: "check".into(), item: Some(it.id), min: MIN_QUESTION,
                        why: format!("итоговая проверка: перенос «{}» на новый пример", title(s)) });
                    continue;
                }
                if used + MIN_QUESTION > budget {
                    break;
                }
                used += MIN_QUESTION;
                let why = match stage.as_str() {
                    "worked" => format!("«{}»: разобранный пример — для начала навыка", title(s)),
                    "faded" => format!("«{}»: тот же метод, один шаг делаешь сам", title(s)),
                    "explain" => format!("«{}»: объясни ключевой шаг своими словами", title(s)),
                    _ if prepared => format!("«{}»: диагностика пройдена — сразу самостоятельная задача", title(s)),
                    _ => format!("«{}»: самостоятельная задача", title(s)),
                };
                plan.push(Step { kind: "new".into(), item: Some(it.id), min: MIN_QUESTION, why });
            }
            if !chk.is_empty() && plan.iter().any(|p| p.kind == "new" && self.c.query_row("SELECT objective FROM item WHERE id=?", [p.item.as_ref().unwrap()], |r| r.get::<_, String>(0)).ok().as_deref() == Some(s)) {
                checks.push(chk);
            }
        }
        for i in 0.. {
            let row: Vec<Step> = checks.iter().filter_map(|c| c.get(i).cloned()).collect();
            if row.is_empty() {
                break;
            }
            plan.extend(row);
        }
        // Перерыв после каждых WORK_STRETCH минут работы, если после него ещё есть шаги.
        let mut out = vec![];
        let mut run = 0;
        let n = plan.len();
        for (k, st) in plan.into_iter().enumerate() {
            run += st.min;
            out.push(st);
            if run >= WORK_STRETCH && k + 1 < n {
                out.push(Step { kind: "break".into(), item: None, min: BREAK_MIN, why: format!("перерыв после {run} мин работы; можно пропустить") });
                run = 0;
            }
        }
        Ok(out)
    }

    /// Открытый блок (любой цели) или новый. Одновременно открыт один блок: второй клиент получает тот же.
    pub fn block_start(&self, goal: i64, predicted: Option<i64>, now: i64) -> Res<i64> {
        self.help_allowed()?;
        if let Some(id) = self.block_open()? {
            return Ok(id);
        }
        let plan = self.plan_build(goal, now)?;
        if plan.iter().all(|s| s.kind == "break") {
            return Err("в цели нет заданий на сегодня: повторений нет, новые задания пройдены. Загляни в «Сегодня» за сроками проверок.".into());
        }
        let planned = plan.iter().filter(|s| s.kind != "break").count() as i64;
        let t = tx(self)?;
        if let Some(id) = self.block_open()? {
            return Ok(id);
        }
        self.c
            .execute("INSERT INTO session(started, planned, predicted) VALUES (?,?,?)", params![now, planned, predicted])
            .map_err(e)?;
        let sid = self.c.last_insert_rowid();
        self.c
            .execute(
                "INSERT INTO learn_block(goal_id, session_id, created, plan, updated) VALUES (?,?,?,?,?)",
                params![goal, sid, now, js(&plan)?, now],
            )
            .map_err(e)?;
        let id = self.c.last_insert_rowid();
        t.commit().map_err(e)?;
        Ok(id)
    }

    pub fn block_open(&self) -> Res<Option<i64>> {
        self.c
            .query_row("SELECT id FROM learn_block WHERE closed_at IS NULL", [], |r| r.get(0))
            .optional()
            .map_err(e)
    }

    fn block_plan(&self, id: i64) -> Res<(Vec<Step>, HashMap<i64, (String, Option<i64>)>, bool)> {
        let (plan, closed): (String, Option<i64>) = self
            .c
            .query_row("SELECT plan, closed_at FROM learn_block WHERE id=?", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| "блока нет")?;
        let done = self
            .list("SELECT step, status, attempt_id FROM block_step WHERE block_id=?", [id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
            .into_iter()
            .collect();
        Ok((serde_json::from_str(&plan).map_err(e)?, done, closed.is_some()))
    }

    /// Состояние блока для любого клиента: план со статусом шагов, текущий шаг (без ключа),
    /// заметка «где остановился», буфер мыслей. Повторный вызов ничего не меняет.
    pub fn block_state(&self, id: i64) -> Res<Value> {
        let (plan, done, closed) = self.block_plan(id)?;
        let (goal, title, note, thoughts, hints, sid): (i64, String, String, String, String, i64) = self
            .c
            .query_row(
                "SELECT b.goal_id, g.title, b.note, b.thoughts, b.hints, b.session_id FROM learn_block b JOIN goal g ON g.id=b.goal_id WHERE b.id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .map_err(e)?;
        let hints: Vec<i64> = serde_json::from_str(&hints).map_err(e)?;
        let cur = (0..plan.len() as i64).find(|k| !done.contains_key(k));
        let steps: Vec<Value> = plan
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let st = done.get(&(k as i64)).map(|d| d.0.as_str()).unwrap_or(if Some(k as i64) == cur { "current" } else { "todo" });
                json!({"kind": s.kind, "item": s.item, "min": s.min, "why": s.why, "status": st})
            })
            .collect();
        let item = match cur.and_then(|k| plan[k as usize].item.clone()) {
            Some(i) => {
                let it = self.item(&i)?;
                json!({"id": it.id, "kind": it.kind, "prompt": it.prompt, "code": it.code, "stage": it.stage,
                       "source": it.source, "domain": it.domain, "grading": grading(&it.kind), "has_hint": it.hint.is_some(),
                       "hinted": hints.contains(&cur.unwrap())})
            }
            None => Value::Null,
        };
        Ok(json!({"block": id, "goal": goal, "title": title, "session": sid, "closed": closed, "step": cur,
                  "steps": steps, "item": item, "note": note, "thoughts": serde_json::from_str::<Value>(&thoughts).map_err(e)?,
                  "minutes_left": plan.iter().enumerate().filter(|(k, _)| !done.contains_key(&(*k as i64))).map(|(_, s)| s.min).sum::<i64>()}))
    }

    /// Правка плана: пройденные шаги неизменны, дальше — любой порядок, удаление и длительность.
    pub fn block_plan_set(&self, id: i64, plan: &[Step], now: i64) -> Res<()> {
        let t = tx(self)?;
        let (old, done, closed) = self.block_plan(id)?;
        if closed {
            return Err("блок уже закрыт".into());
        }
        let fixed = done.keys().copied().max().map_or(0, |k| k + 1) as usize;
        if plan.len() < fixed || plan[..fixed] != old[..fixed] {
            return Err("пройденные шаги менять нельзя".into());
        }
        for s in &plan[fixed..] {
            let ok = match s.kind.as_str() {
                "break" => s.item.is_none() && (1..=60).contains(&s.min),
                "diag" | "review" | "new" | "check" => s.item.as_deref().is_some_and(|i| self.item(i).is_ok()) && (1..=60).contains(&s.min),
                _ => false,
            };
            if !ok {
                return Err(format!("неверный шаг плана: {} {}", s.kind, s.item.as_deref().unwrap_or("")));
            }
        }
        self.c
            .execute("UPDATE learn_block SET plan=?, edited=1, updated=? WHERE id=?", params![js(&plan)?, now, id])
            .map_err(e)?;
        t.commit().map_err(e)
    }

    fn current(&self, id: i64, step: i64) -> Res<Step> {
        let (plan, done, closed) = self.block_plan(id)?;
        if closed {
            return Err("блок уже закрыт".into());
        }
        let cur = (0..plan.len() as i64).find(|k| !done.contains_key(k));
        if cur != Some(step) {
            return Err("этот шаг уже пройден или ещё не начат; обнови экран".into());
        }
        Ok(plan[step as usize].clone())
    }

    /// Подсказка текущего шага. Факт помощи хранится в блоке, а не в клиенте: ответ после неё
    /// не самостоятельный в любом клиенте.
    pub fn block_hint(&self, id: i64, step: i64) -> Res<String> {
        self.help_allowed()?;
        let st = self.current(id, step)?;
        if st.kind == "diag" || st.kind == "check" {
            return Err("в диагностике и итоговой проверке подсказок нет".into());
        }
        let it = self.item(st.item.as_deref().unwrap_or(""))?;
        let h = it.hint.ok_or("у этого задания подсказки нет")?;
        self.c
            .execute(
                "UPDATE learn_block SET hints=(SELECT json_group_array(value) FROM (SELECT value FROM json_each(hints) UNION SELECT ?2)) WHERE id=?1",
                params![id, step],
            )
            .map_err(e)?;
        Ok(h)
    }

    /// Эталон и рубрика для самооценки (card, explain). Ключ задач с автоматической оценкой
    /// выдаётся только после ответа.
    pub fn block_reveal(&self, id: i64, step: i64) -> Res<Value> {
        let st = self.current(id, step)?;
        let it = self.item(st.item.as_deref().unwrap_or(""))?;
        if grading(&it.kind) != "self" {
            return Err("эталон этого задания откроется после ответа".into());
        }
        Ok(json!({"answer": it.answer, "rubric": it.rubric, "explain": it.explain}))
    }

    /// Ответ на шаг. Повторная отправка того же шага (двойной клик, второй клиент) возвращает
    /// записанный результат и новую попытку не создаёт. `rating` — самооценка card/explain:
    /// again, hard или good. Задания с автоматической оценкой её игнорируют.
    pub fn block_answer(&self, id: i64, step: i64, ans: &str, rating: Option<&str>, ms: i64, now: i64) -> Res<Value> {
        if let Some(r) = self.step_result(id, step)? {
            return Ok(r);
        }
        let st = self.current(id, step)?;
        let it = self.item(st.item.as_deref().ok_or("на перерыв не отвечают: заверши или пропусти его")?)?;
        let selfg = grading(&it.kind) == "self";
        let rating = if selfg { rating.ok_or("оцени себя: again, hard или good")? } else { "" };
        if selfg && !["again", "hard", "good"].contains(&rating) {
            return Err("самооценка: again, hard или good".into());
        }
        // Оценка (SymPy, тесты кода) — вне транзакции записи: она может идти секунды.
        let ok = if selfg { rating != "again" } else { grade(&it, ans) };
        let t = tx(self)?;
        if let Some(r) = self.step_result(id, step)? {
            return Ok(r);
        }
        self.current(id, step)?;
        let (sid, hints): (i64, String) = self
            .c
            .query_row("SELECT session_id, hints FROM learn_block WHERE id=?", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(e)?;
        let hinted = serde_json::from_str::<Vec<i64>>(&hints).map_err(e)?.contains(&step);
        let carded: bool = self.c.query_row("SELECT count(*)>0 FROM card WHERE item_id=?", [&it.id], |r| r.get(0)).map_err(e)?;
        let role = match st.kind.as_str() {
            "diag" => "diag",
            _ if carded => "review",
            _ => "new",
        };
        self.record_rated(Some(sid), &it, role, ans.trim(), ok, hinted, rating == "hard", ms, now)?;
        // Не last_insert_rowid: record_rated после попытки ещё пишет card, и rowid был бы карточки.
        let aid: i64 = self.c.query_row("SELECT max(id) FROM attempt", [], |r| r.get(0)).map_err(e)?;
        self.c
            .execute("INSERT INTO block_step(block_id, step, status, attempt_id, ts) VALUES (?,?,'answered',?,?)", params![id, step, aid, now])
            .map_err(e)?;
        self.c.execute("UPDATE learn_block SET updated=? WHERE id=?", params![now, id]).map_err(e)?;
        // После диагностики остаток неотредактированного плана пересобирается под её результат.
        let (plan, done, _) = self.block_plan(id)?;
        let edited: bool = self.c.query_row("SELECT edited FROM learn_block WHERE id=?", [id], |r| r.get(0)).map_err(e)?;
        if st.kind == "diag" && !edited && !plan.iter().enumerate().any(|(k, s)| s.kind == "diag" && !done.contains_key(&(k as i64))) {
            let goal: i64 = self.c.query_row("SELECT goal_id FROM learn_block WHERE id=?", [id], |r| r.get(0)).map_err(e)?;
            let mut next = plan[..=step as usize].to_vec();
            next.extend(self.plan_build(goal, now)?.into_iter().filter(|s| s.kind != "diag"));
            self.c.execute("UPDATE learn_block SET plan=? WHERE id=?", params![js(&next)?, id]).map_err(e)?;
        }
        t.commit().map_err(e)?;
        Ok(self.step_result(id, step)?.unwrap_or(Value::Null))
    }

    /// Результат записанного шага: верно ли, эталон, пояснение, когда задание вернётся.
    fn step_result(&self, id: i64, step: i64) -> Res<Option<Value>> {
        self.c
            .query_row(
                "SELECT a.correct, a.hint, a.answer, a.item_id, a.graded, c.due FROM block_step s JOIN attempt a ON a.id=s.attempt_id
                 LEFT JOIN card c ON c.item_id=a.item_id WHERE s.block_id=? AND s.step=?",
                params![id, step],
                |r| Ok((r.get::<_, bool>(0)?, r.get::<_, bool>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, Option<i64>>(5)?)),
            )
            .optional()
            .map_err(e)?
            .map(|(ok, hint, ans, item, graded, due)| {
                let it = self.item(&item)?;
                Ok(json!({"already": true, "ok": ok, "helped": hint, "answer": ans, "key": it.answer.first(),
                          "explain": it.explain, "rubric": it.rubric, "graded": graded, "due": due, "retry": !ok}))
            })
            .transpose()
    }

    /// Повторное решение после разбора: отдельный факт `retry`, расписание и зачёт не меняет.
    pub fn block_retry(&self, id: i64, step: i64, ans: &str, ms: i64, now: i64) -> Res<bool> {
        let (sid, item): (i64, String) = self
            .c
            .query_row(
                "SELECT b.session_id, a.item_id FROM block_step s JOIN attempt a ON a.id=s.attempt_id
                 JOIN learn_block b ON b.id=s.block_id WHERE s.block_id=? AND s.step=? AND b.closed_at IS NULL",
                params![id, step],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| "повтор возможен для отвеченного шага открытого блока")?;
        let it = self.item(&item)?;
        let ok = grade(&it, ans);
        self.record(Some(sid), &it, "retry", ans.trim(), ok, false, ms, now)?;
        Ok(ok)
    }

    /// Перерыв пройден или пропущен, задание отложено без ответа. Повтор безопасен.
    pub fn block_skip(&self, id: i64, step: i64, now: i64) -> Res<()> {
        let t = tx(self)?;
        if self.c.query_row("SELECT count(*)>0 FROM block_step WHERE block_id=? AND step=?", params![id, step], |r| r.get::<_, bool>(0)).map_err(e)? {
            return Ok(());
        }
        let st = self.current(id, step)?;
        let status = if st.kind == "break" { "done" } else { "skipped" };
        self.c
            .execute("INSERT INTO block_step(block_id, step, status, ts) VALUES (?,?,?,?)", params![id, step, status, now])
            .map_err(e)?;
        t.commit().map_err(e)
    }

    pub fn block_note(&self, id: i64, note: &str, now: i64) -> Res<()> {
        self.c
            .execute("UPDATE learn_block SET note=?, updated=? WHERE id=?", params![note.trim(), now, id])
            .map_err(e)?;
        Ok(())
    }

    /// Буфер мыслей: отвлекающая мысль записывается и не прерывает шаг.
    pub fn block_thought(&self, id: i64, text: &str, now: i64) -> Res<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        self.c
            .execute(
                "UPDATE learn_block SET thoughts=json_insert(thoughts, '$[#]', json_object('ts', ?2, 'text', ?3)), updated=?2 WHERE id=?1",
                params![id, now, text.trim()],
            )
            .map_err(e)?;
        Ok(())
    }

    /// Завершение в любой момент. Энергия (1–5) и рефлексия — по желанию. Повтор возвращает тот же итог.
    pub fn block_close(&self, id: i64, energy: Option<i64>, reflection: Option<&str>, now: i64) -> Res<Value> {
        if energy.is_some_and(|x| !(1..=5).contains(&x)) {
            return Err("энергия — от 1 до 5".into());
        }
        let t = tx(self)?;
        let sid: i64 = self.c.query_row("SELECT session_id FROM learn_block WHERE id=?", [id], |r| r.get(0)).map_err(|_| "блока нет")?;
        let n = self
            .c
            .execute(
                "UPDATE learn_block SET closed_at=?2, energy=?3, reflection=?4, updated=?2 WHERE id=?1 AND closed_at IS NULL",
                params![id, now, energy, reflection.map(str::trim).filter(|s| !s.is_empty())],
            )
            .map_err(e)?;
        if n > 0 {
            self.session_end(sid, 0, now)?;
        }
        t.commit().map_err(e)?;
        self.block_summary(id)
    }

    /// Итог: ответы, помощь, ошибки, что вернётся и когда, мысли из буфера.
    pub fn block_summary(&self, id: i64) -> Res<Value> {
        let (sid, note, thoughts, energy, reflection, closed): (i64, String, String, Option<i64>, Option<String>, Option<i64>) = self
            .c
            .query_row(
                "SELECT session_id, note, thoughts, energy, reflection, closed_at FROM learn_block WHERE id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .map_err(|_| "блока нет")?;
        let rows = self.list(
            "SELECT a.item_id, a.role, a.correct, a.hint, a.answer, a.graded, c.due, i.data FROM attempt a JOIN item i ON i.id=a.item_id
             LEFT JOIN card c ON c.item_id=a.item_id WHERE a.session_id=? AND a.role!='retry' ORDER BY a.id",
            [sid],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?, r.get::<_, bool>(3)?, r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?, r.get::<_, Option<i64>>(6)?, r.get::<_, String>(7)?)),
        )?;
        let mut answers = vec![];
        for (item, role, ok, hint, ans, graded, due, data) in rows {
            let it: Item = serde_json::from_str(&data).map_err(e)?;
            answers.push(json!({"item": item, "role": role, "prompt": it.prompt, "answer": ans, "ok": ok, "helped": hint,
                                "graded": graded, "key": it.answer.first(), "explain": it.explain, "due": due}));
        }
        let ok = answers.iter().filter(|a| a["ok"] == true && a["helped"] == false).count();
        let helped = answers.iter().filter(|a| a["helped"] == true).count();
        let skipped: i64 = self.c.query_row("SELECT count(*) FROM block_step WHERE block_id=? AND status='skipped'", [id], |r| r.get(0)).map_err(e)?;
        Ok(json!({"block": id, "closed": closed.is_some(), "answers": answers, "independent_correct": ok, "helped": helped,
                  "skipped": skipped, "note": note, "thoughts": serde_json::from_str::<Value>(&thoughts).map_err(e)?,
                  "energy": energy, "reflection": reflection}))
    }

    /// Экран «Сегодня»: явные действия с причиной приоритета и сроком.
    /// `offset` — смещение часового пояса в секундах (как у брифинга).
    pub fn today(&self, now: i64, offset: i64) -> Res<Value> {
        let mut actions = vec![];
        if let Some(b) = self.block_open()? {
            let s = self.block_state(b)?;
            let total = s["steps"].as_array().map_or(0, |a| a.len());
            let at = s["step"].as_i64().map_or(total as i64, |x| x);
            actions.push(json!({"kind": "block", "block": b, "goal": s["goal"], "title": format!("Продолжить блок «{}»", s["title"].as_str().unwrap_or("")),
                "reason": format!("незаконченная работа: шаг {} из {total}{}", at + 1,
                    if s["note"].as_str().unwrap_or("").is_empty() { String::new() } else { format!(", заметка: «{}»", s["note"].as_str().unwrap_or("")) }),
                "due": Value::Null}));
        }
        let open_test: Option<(String, i64, i64)> = self
            .c
            .query_row(
                "SELECT o.title, b.due_at, (SELECT count(*) FROM test_exposure x WHERE x.block_id=b.id AND x.attempt_id IS NULL)
                 FROM test_block b JOIN objective o ON o.id=b.objective WHERE b.closed_at IS NULL ORDER BY b.id LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(e)?;
        let window = crate::weekly::TEST_WINDOW;
        if let Some((t, due, left)) = &open_test {
            actions.push(json!({"kind": "test", "title": format!("Продолжить проверку «{t}»"), "due": due + window,
                "reason": format!("проверка начата, осталось {left}; ответы в окне до срока + 2 дня считаются первыми, разбор — после последнего ответа")}));
        }
        let due_tests = self.list(
            "SELECT o.title, o.done_at+o.horizon*86400, o.horizon FROM objective o WHERE o.done_at IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM test_block b WHERE b.objective=o.id)
               AND EXISTS (SELECT 1 FROM item i WHERE i.objective=o.id AND i.role='test7'
                           AND i.id NOT IN (SELECT item_id FROM attempt WHERE role='test7'))
             ORDER BY 2",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
        )?;
        let mut upcoming = vec![];
        for (t, due, h) in due_tests {
            if due <= now && open_test.is_none() {
                let late = due + window < now;
                actions.push(json!({"kind": "test", "title": format!("Проверка через {h} дн.: «{t}»"), "due": due + window,
                    "reason": if late { "окно проверки прошло: ответ запишется как опоздание, не как первый ответ".to_string() }
                              else { format!("срок настал; окно открыто ещё {} ч", (due + window - now) / 3600) }}));
            } else if due > now {
                upcoming.push(json!({"kind": "test", "title": format!("Проверка «{t}»"), "at": due}));
            }
        }
        let (due_now, oldest): (i64, Option<i64>) = self
            .c
            .query_row(
                "SELECT count(*), min(c.due) FROM card c JOIN item i ON i.id=c.item_id JOIN objective o ON o.id=i.objective
                 WHERE o.arm!='usual' AND c.due<=?",
                [now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(e)?;
        let overdue_days = oldest.map_or(0, |o| (now - o) / DAY);
        if due_now > 0 {
            actions.push(json!({"kind": "reviews", "title": format!("Повторения: {due_now}"), "due": oldest, "count": due_now,
                "overdue_days": overdue_days,
                "reason": if overdue_days > 0 { format!("самое старое ждёт {overdue_days} дн.; FSRS выводит карточку, когда прогноз вспоминания падает до 90%") }
                          else { "сегодня прогноз вспоминания этих карточек опустился до 90%".into() },
                "spread": due_now > 20}));
        }
        let goals = self.goals()?;
        for g in goals.as_array().into_iter().flatten() {
            if g["archived"].is_null() && g["left"].as_i64().unwrap_or(0) > 0 {
                let dl = g["deadline"].as_i64();
                actions.push(json!({"kind": "goal", "goal": g["id"], "title": format!("Продолжить цель «{}»", g["title"].as_str().unwrap_or("")),
                    "due": dl, "reason": match dl {
                        Some(d) if d > now => format!("новых заданий {}; до срока {} дн.", g["left"], (d - now + DAY - 1) / DAY),
                        Some(_) => format!("срок цели прошёл; новых заданий {}", g["left"]),
                        None => format!("новых заданий {}; следующий навык — «{}»", g["left"], g["next_skill"].as_str().unwrap_or("")),
                    }}));
            }
        }
        let next_review: Option<i64> = self
            .c
            .query_row("SELECT min(due) FROM card WHERE due>?", [now], |r| r.get(0))
            .map_err(e)?;
        if let Some(t) = next_review {
            upcoming.push(json!({"kind": "reviews", "title": "Следующее повторение", "at": t}));
        }
        let last: Option<i64> = self
            .c
            .query_row("SELECT max(ts) FROM attempt", [], |r| r.get(0))
            .map_err(e)?;
        let day = |t: i64| (t + offset).div_euclid(DAY);
        let gap = last.map_or(0, |l| (day(now) - day(l) - 1).max(0));
        let empty = goals.as_array().is_none_or(|a| a.is_empty()) && actions.is_empty();
        Ok(json!({"actions": actions, "upcoming": upcoming, "gap_days": gap,
            "gap_note": if gap > 0 { format!("Перерыв {gap} дн. Ничего не потеряно: повторения ждут в очереди, их можно распределить по дням.") } else { String::new() },
            "empty": empty, "goals": goals}))
    }

    pub fn goals(&self) -> Res<Value> {
        let rows = self.list(
            "SELECT id, module, title, skills, materials, deadline, block_min, archived, created FROM goal ORDER BY archived IS NOT NULL, id",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?,
                    r.get::<_, Option<i64>>(5)?, r.get::<_, i64>(6)?, r.get::<_, Option<i64>>(7)?, r.get::<_, i64>(8)?)),
        )?;
        let mut out = vec![];
        for (id, module, title, skills, materials, deadline, block_min, archived, created) in rows {
            let (order, _) = self.goal_skills(id)?;
            let mut sk = vec![];
            let mut left = 0;
            let mut next = None;
            for s in &order {
                let (t, total, seen, diag, done, horizon): (String, i64, i64, i64, Option<i64>, i64) = self
                    .c
                    .query_row(
                        "SELECT o.title, (SELECT count(*) FROM item i WHERE i.objective=o.id AND i.role='practice'),
                           (SELECT count(*) FROM item i JOIN card c ON c.item_id=i.id WHERE i.objective=o.id),
                           (SELECT count(*) FROM item i WHERE i.objective=o.id AND i.role='diag'), o.done_at, o.horizon
                         FROM objective o WHERE o.id=?",
                        [s],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                    )
                    .map_err(e)?;
                let needs = self.list("SELECT needs FROM prereq WHERE skill=?", [s], |r| r.get::<_, String>(0))?;
                left += total - seen;
                if next.is_none() && total > seen {
                    next = Some(t.clone());
                }
                sk.push(json!({"id": s, "title": t, "practice": total, "seen": seen, "diag": diag, "prepared": self.prepared(s)?,
                               "needs": needs, "done_at": done, "horizon": horizon}));
            }
            out.push(json!({"id": id, "module": module, "title": title, "skills": sk, "order": serde_json::from_str::<Value>(&skills).map_err(e)?,
                "materials": serde_json::from_str::<Value>(&materials).map_err(e)?, "deadline": deadline, "block_min": block_min,
                "archived": archived, "created": created, "left": left, "next_skill": next}));
        }
        Ok(out.into())
    }

    /// Накопившиеся повторения равномерно по `days` дням начиная с сегодня. Меняется только срок
    /// показа, состояние памяти FSRS остаётся; при показе позже срока FSRS учтёт фактическую задержку.
    pub fn spread_reviews(&self, days: i64, now: i64) -> Res<i64> {
        if !(2..=30).contains(&days) {
            return Err("распределить можно на 2–30 дней".into());
        }
        let t = tx(self)?;
        let ids = self.list("SELECT item_id FROM card WHERE due<=? ORDER BY due, item_id", [now], |r| r.get::<_, String>(0))?;
        let per = (ids.len() as i64 + days - 1) / days;
        for (k, id) in ids.iter().enumerate() {
            let d = k as i64 / per.max(1);
            if d > 0 {
                self.c.execute("UPDATE card SET due=? WHERE item_id=?", params![now + d * DAY, id]).map_err(e)?;
            }
        }
        t.commit().map_err(e)?;
        Ok(ids.len() as i64)
    }

    /// Экран «Прогресс». У каждой доли — числитель, знаменатель, правило и строки для проверки.
    pub fn progress(&self, now: i64) -> Res<Value> {
        let window = crate::weekly::TEST_WINDOW;
        // Удержание: первые ответы в окне на невиданные формы закрытых или просроченных блоков.
        let ret_rows = self.list(
            "SELECT o.horizon, coalesce(m.domain,'cyber'), x.item_id, a.correct, a.ts, b.due_at FROM test_exposure x
             JOIN test_block b ON b.id=x.block_id JOIN objective o ON o.id=b.objective LEFT JOIN module m ON m.id=o.module
             JOIN attempt a ON a.id=x.attempt_id
             WHERE x.first=1 AND a.ts<=b.due_at+?1 AND (b.closed_at IS NOT NULL OR b.due_at+?1<?2) ORDER BY o.horizon, 2, a.ts",
            params![window, now],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?)),
        )?;
        let mut retention: Vec<Value> = vec![];
        for (h, d, item, ok, ts, due) in ret_rows {
            let k = retention.iter().position(|r| r["horizon"] == h && r["domain"] == d.as_str());
            let k = k.unwrap_or_else(|| {
                retention.push(json!({"horizon": h, "domain": d, "num": 0, "den": 0, "rows": [],
                    "rule": "первый ответ на ранее не показанное задание отложенной формы, данный не позже срока + 2 дня; блок закрыт или окно прошло"}));
                retention.len() - 1
            });
            let r = &mut retention[k];
            r["den"] = (r["den"].as_i64().unwrap() + 1).into();
            r["num"] = (r["num"].as_i64().unwrap() + ok as i64).into();
            r["rows"].as_array_mut().unwrap().push(json!({"item": item, "ok": ok, "at": ts, "delay_h": (ts - due) / 3600}));
        }
        for r in &mut retention {
            r["low"] = (r["den"].as_i64().unwrap() < LOW_N).into();
        }
        // Первые самостоятельные попытки на новых задачах: без подсказки, кроме разобранных примеров.
        let sk_rows = self.list(
            "SELECT o.id, o.title, coalesce(m.domain,'cyber'), a.item_id, a.correct, a.ts FROM attempt a JOIN item i ON i.id=a.item_id
             JOIN objective o ON o.id=i.objective LEFT JOIN module m ON m.id=o.module
             WHERE a.role='new' AND a.hint=0 AND a.graded='auto' AND coalesce(json_extract(i.data,'$.stage'),'') NOT IN ('worked','faded')
               AND a.id=(SELECT min(id) FROM attempt b WHERE b.item_id=a.item_id) ORDER BY o.id, a.ts",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, bool>(4)?, r.get::<_, i64>(5)?)),
        )?;
        let mut skills: Vec<Value> = vec![];
        for (id, title, d, item, ok, ts) in sk_rows {
            let k = skills.iter().position(|s| s["skill"] == id.as_str()).unwrap_or_else(|| {
                skills.push(json!({"skill": id, "title": title, "domain": d, "num": 0, "den": 0, "rows": [],
                    "rule": "первая попытка на задании, которое раньше не показывалось; без подсказки; разобранные примеры и самооценка не входят"}));
                skills.len() - 1
            });
            let s = &mut skills[k];
            s["den"] = (s["den"].as_i64().unwrap() + 1).into();
            s["num"] = (s["num"].as_i64().unwrap() + ok as i64).into();
            s["rows"].as_array_mut().unwrap().push(json!({"item": item, "ok": ok, "at": ts}));
        }
        for s in &mut skills {
            s["low"] = (s["den"].as_i64().unwrap() < LOW_N).into();
        }
        let (fs_n, fs_p, fs_ok): (i64, f64, i64) = self
            .c
            .query_row(
                "SELECT count(*), coalesce(sum(p_recall),0), coalesce(sum(correct),0) FROM attempt WHERE role='review' AND hint=0 AND p_recall IS NOT NULL",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(e)?;
        let (s_n, s_pred, s_fact): (i64, i64, i64) = self
            .c
            .query_row(
                "SELECT count(*), coalesce(sum(predicted),0), coalesce(sum(correct),0) FROM session WHERE ended IS NOT NULL AND predicted IS NOT NULL",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(e)?;
        let queue: (i64, i64, i64) = self
            .c
            .query_row(
                "SELECT coalesce(sum(due<=?1),0), coalesce(sum(due>?1 AND due<=?1+86400),0), coalesce(sum(due>?1 AND due<=?1+7*86400),0) FROM card",
                [now],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(e)?;
        let vol = |t: &str| -> Res<i64> { self.c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).map_err(e) };
        let room: (i64, i64) = self
            .c
            .query_row(
                "SELECT coalesce(sum(coalesce(ended,?1)-started),0)/60, coalesce(sum(CASE WHEN started>=?1-7*86400 THEN coalesce(ended,?1)-started END),0)/60 FROM study",
                [now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(e)?;
        Ok(json!({
            "retention": retention, "skills": skills,
            "forecast": {"fsrs": {"n": fs_n, "expected": (fs_p * 10.0).round() / 10.0, "actual": fs_ok, "low": fs_n < LOW_N,
                          "rule": "сумма прогнозов FSRS вероятности вспомнить перед каждым самостоятельным повторением против числа верных; это прогноз, а не измерение"},
                         "self": {"n": s_n, "predicted": s_pred, "actual": s_fact, "low": s_n < LOW_N,
                          "rule": "твой прогноз перед блоком против верных самостоятельных ответов в нём (сравнимые блоки — с прогнозом)"}},
            "queue": {"now": queue.0, "day": queue.1, "week": queue.2},
            "volume": {"attempts": vol("attempt")?, "items": vol("item")?, "cards": vol("card")?, "goals": vol("goal")?,
                       "blocks": vol("learn_block")?, "checks": vol("test_exposure")?},
            "room_minutes": {"total": room.0, "week": room.1, "note": "время в комнате — контекст: это активность, а не результат обучения"},
            "low_n": LOW_N,
        }))
    }

    /// «Что WHITE ROOM видит сейчас»: состояние записи по источнику и сессии, способ отзыва,
    /// локальные файлы и интеграции. Учёба офлайн от этого не зависит.
    pub fn seen(&self, db_path: &str) -> Res<Value> {
        let active = self.study_active()?;
        let cap = self.capture_state()?;
        let sources = match (active, cap) {
            (Some((id, started, topic)), Some((_, w, b))) => json!({"session": id, "topic": topic, "started": started,
                "windows": w, "browser": b,
                "revoke": "Кнопки «Отключить» в комнате или команда whiteroom stop; запись источника прекращается сразу, новые записи отвергаются при сохранении"}),
            _ => json!({"session": null, "windows": false, "browser": false, "revoke": "Сейчас сессии нет: окна и вкладки не записываются"}),
        };
        let size = |p: &str| std::fs::metadata(p).map(|m| m.len()).ok();
        let dir = std::path::Path::new(db_path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let name = std::path::Path::new(db_path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut copies = vec![];
        for f in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            if n.starts_with(&name) && n.ends_with(".bak") {
                copies.push(json!({"path": f.path(), "bytes": f.metadata().map(|m| m.len()).ok()}));
            }
        }
        let home = std::env::var("HOME").unwrap_or_default();
        let host = format!("{home}/.config/chromium/NativeMessagingHosts/com.whiteroom.host.json");
        let sync = format!("{home}/.config/systemd/user/whiteroom-progress-sync.timer");
        let counts = |t: &str| -> Res<i64> { self.c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).map_err(e) };
        // Завершённые сессии с записями окон и страниц: их можно очистить (`activity_clear`).
        let mut activity = self
            .c
            .prepare(
                "SELECT * FROM (SELECT id, topic, started, ended, (SELECT count(*) FROM focus WHERE study_id=study.id) w,
                   (SELECT count(*) FROM page WHERE study_id=study.id) p FROM study WHERE ended IS NOT NULL)
                 WHERE w+p>0 ORDER BY id DESC LIMIT 50",
            )
            .map_err(e)?
            .query_map([], |r| {
                Ok(json!({"session": r.get::<_, i64>(0)?, "topic": r.get::<_, String>(1)?, "started": r.get::<_, i64>(2)?,
                          "ended": r.get::<_, i64>(3)?, "windows": r.get::<_, i64>(4)?, "pages": r.get::<_, i64>(5)?}))
            })
            .map_err(e)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(e)?;
        let (w, p) = (counts("focus WHERE study_id IS NULL")?, counts("page WHERE study_id IS NULL")?);
        if w + p > 0 {
            activity.push(json!({"session": null, "topic": "Записи без сессии (до версии со связью с сессией)", "windows": w, "pages": p}));
        }
        Ok(json!({
            "capture": sources,
            "stored": {"db": db_path, "bytes": size(db_path), "journal": size(&format!("{db_path}-journal")), "wal": size(&format!("{db_path}-wal")),
                       "copies": copies, "rows": {"windows": counts("focus")?, "pages": counts("page")?, "attempts": counts("attempt")?,
                       "tutor_messages": counts("tutor_message")?, "drafts": counts("ai_draft")?}},
            "activity": activity,
            "integrations": {
                "native_host": std::path::Path::new(&host).exists(),
                "d1_sync_timer": std::path::Path::new(&sync).exists(),
                "claude_key_in_env": std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.is_empty()),
            },
            "notes": ["Учёба, проверки и отчёты работают без ключа Claude, без расширения и с выключенной записью.",
                      "Запрос к Claude уходит только после показа точного JSON и подтверждения.",
                      "Синхронизация с D1 (D11) копирует прогресс и журналы активности, если включён её таймер; локальное удаление облачную копию не трогает."],
        }))
    }

    /// Резервная копия всей базы в новый файл (согласованное состояние SQLite, без журнала).
    pub fn backup(&self, dest: &str) -> Res<()> {
        if std::path::Path::new(dest).exists() {
            return Err(format!("{dest} уже существует: выбери новое имя"));
        }
        self.c.execute("VACUUM INTO ?", [dest]).map_err(e)?;
        Ok(())
    }

    /// «Очистить записи активности» (решение владельца 2026-11-20): заголовки окон, страницы и
    /// выдержки одной завершённой сессии или записи без сессии (`None`). `apply=false` только
    /// считает. Время, тема и режим сессии, учёба, список блокировки и копии `.bak` остаются.
    /// Одна транзакция: прерывание ничего не удаляет, повтор безопасен.
    pub fn activity_clear(&self, session: Option<i64>, apply: bool) -> Res<Value> {
        if let Some(id) = session {
            let ended: Option<Option<i64>> =
                self.c.query_row("SELECT ended FROM study WHERE id=?", [id], |r| r.get(0)).optional().map_err(e)?;
            match ended {
                None => return Err(format!("Сессии {id} нет.")),
                Some(None) => return Err("Сессия ещё идёт: сначала заверши её.".into()),
                _ => {}
            }
        }
        let n = |q: &str| -> Res<i64> { self.c.query_row(q, [session], |r| r.get(0)).map_err(e) };
        let r = json!({"session": session,
            "windows": n("SELECT count(*) FROM focus WHERE study_id IS ?1")?,
            "pages": n("SELECT count(*) FROM page WHERE study_id IS ?1")?,
            "excerpts": n("SELECT count(*) FROM page WHERE study_id IS ?1 AND excerpt IS NOT NULL")?,
            "kept": "Время, тема и режим сессии, карточки, ответы, результаты и список блокировки остаются.",
            "elsewhere": "Копии .bak, сохранённые выгрузки и облачная копия D1 не меняются: их удаляешь сам."});
        if apply {
            // secure_delete затирает удалённый текст в файле базы, а не только помечает страницы свободными.
            self.c.execute_batch("PRAGMA secure_delete=ON").map_err(e)?;
            let tx = self.c.unchecked_transaction().map_err(e)?;
            tx.execute("DELETE FROM focus WHERE study_id IS ?1", [session]).map_err(e)?;
            tx.execute("DELETE FROM page WHERE study_id IS ?1", [session]).map_err(e)?;
            tx.commit().map_err(e)?;
            self.c.execute_batch("PRAGMA secure_delete=OFF; PRAGMA wal_checkpoint(TRUNCATE);").map_err(e)?;
        }
        Ok(r)
    }
}

/// Восстановление из копии: проверка целостности и версии схемы, текущая база сохраняется рядом
/// (`<db>.before-restore-<ts>.bak`), затем копия атомарно занимает её место.
/// Перед вызовом закрой окно и останови сессию: открытые соединения увидят старый файл.
pub fn restore(src: &str, db_path: &str, now: i64) -> Res<String> {
    let c = Connection::open_with_flags(src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(e)?;
    let ok: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0)).map_err(e)?;
    let ver: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(e)?;
    let ours: bool = c
        .query_row("SELECT count(*)>0 FROM sqlite_master WHERE name='objective'", [], |r| r.get(0))
        .map_err(e)?;
    if ok != "ok" || !ours {
        return Err(format!("{src} — не целая база WHITE ROOM ({ok})"));
    }
    if ver > SCHEMA {
        return Err(format!("копия от более новой версии (схема {ver}, поддерживается {SCHEMA})"));
    }
    drop(c);
    let mut saved = String::new();
    if std::path::Path::new(db_path).exists() {
        saved = format!("{db_path}.before-restore-{now}.bak");
        Connection::open(db_path).map_err(e)?.execute("VACUUM INTO ?", [&saved]).map_err(e)?;
    }
    let tmp = format!("{db_path}.restore-tmp");
    std::fs::copy(src, &tmp).map_err(e)?;
    std::fs::rename(&tmp, db_path).map_err(e)?;
    for side in ["-journal", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{db_path}{side}"));
    }
    Ok(saved)
}

/// Импорт карточек Anki «Notes in Plain Text» (поле1 — вопрос, поле2 — ответ, табуляция).
/// Переносятся только вопрос и ответ: история повторений и расписание Anki не переносятся.
pub fn anki_import(tsv: &str, id: &str, title: &str) -> Res<Module> {
    let mut items = vec![];
    for (n, line) in tsv.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut f = line.split('\t');
        let (Some(q), Some(a)) = (f.next(), f.next()) else {
            return Err(format!("строка {}: нужно два поля через табуляцию", n + 1));
        };
        let clean = |s: &str| s.replace("<br>", "\n").replace("&nbsp;", " ").trim().to_string();
        if clean(q).is_empty() || clean(a).is_empty() {
            return Err(format!("строка {}: пустой вопрос или ответ", n + 1));
        }
        items.push(Item {
            id: format!("{id}-{}", n + 1), v: 1, objective: id.into(), role: "practice".into(), kind: "card".into(),
            prompt: clean(q), code: None, run: false, answer: vec![clean(a)], hint: None, explain: String::new(),
            stage: None, source: Some("Anki".into()), rubric: Some("Вспомнил ответ целиком до показа?".into()), domain: None, tests: None,
        });
    }
    let mut m = Module {
        id: id.into(), title: title.into(), domain: "custom".into(),
        objective: vec![crate::Objective { id: id.into(), title: title.into(), needs: vec![], horizon: 7 }],
        item: items,
    };
    Module::check(&mut m)?;
    Ok(m)
}

/// Карточки и задания модуля в TSV для Anki: вопрос (с кодом) и первый эталон.
pub fn anki_export(m: &Module) -> String {
    let cell = |s: &str| s.replace('\t', " ").replace('\n', "<br>");
    let mut out = String::from("#separator:tab\n#html:true\n");
    for it in m.item.iter().filter(|i| i.role == "practice") {
        let q = match &it.code {
            Some(c) => format!("{}\n{c}", it.prompt),
            None => it.prompt.clone(),
        };
        out += &format!("{}\t{}\n", cell(&q), cell(it.answer.first().map_or("", |a| a)));
    }
    out
}

fn line(out: &mut impl std::io::Write, s: &str) -> Res<()> {
    writeln!(out, "{s}").map_err(e)
}

fn date(t: i64) -> String {
    let d = t.div_euclid(DAY);
    // Гражданская дата из дней с 1970-01-01 (UTC), алгоритм Хиннанта.
    let z = d + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (dd, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{m:02}-{dd:02}", yoe + era * 400 + (m <= 2) as i64)
}

/// Текст экрана «Сегодня» для CLI.
pub fn today_text(t: &Value, offset: i64) -> String {
    let mut s = String::new();
    if t["gap_days"].as_i64().unwrap_or(0) > 0 {
        s += &format!("{}\n\n", t["gap_note"].as_str().unwrap_or(""));
    }
    if t["empty"] == true {
        return s + "Целей пока нет. Выбери тему: whiteroom goal new (кибербезопасность, математика, программирование, немецкий, исследования, своя), затем whiteroom learn.\n";
    }
    for (k, a) in t["actions"].as_array().into_iter().flatten().enumerate() {
        let word = if a["kind"] == "reviews" { "ждут с" } else { "до" };
        let due = a["due"].as_i64().map_or(String::new(), |d| format!(" ({word} {})", date(d + offset)));
        s += &format!("{}. {}{due}\n   {}\n", k + 1, a["title"].as_str().unwrap_or(""), a["reason"].as_str().unwrap_or(""));
        if a["spread"] == true {
            s += "   распределить: whiteroom spread <дней>\n";
        }
    }
    if t["actions"].as_array().is_none_or(|a| a.is_empty()) {
        s += "На сегодня всё сделано.\n";
    }
    for u in t["upcoming"].as_array().into_iter().flatten() {
        s += &format!("Дальше: {} — {}\n", u["title"].as_str().unwrap_or(""), date(u["at"].as_i64().unwrap_or(0) + offset));
    }
    s
}

/// Текст экрана «Прогресс» для CLI: у каждой доли числитель/знаменатель и правило.
pub fn progress_text(p: &Value) -> String {
    let frac = |v: &Value| format!("{}/{}{}", v["num"], v["den"], if v["low"] == true { " (мало данных)" } else { "" });
    let mut s = String::from("Удержание (первые ответы на новые формы):\n");
    for r in p["retention"].as_array().into_iter().flatten() {
        s += &format!("  {} дн., {}: {}\n", r["horizon"], r["domain"].as_str().unwrap_or(""), frac(r));
    }
    s += "Первые самостоятельные попытки на новых задачах:\n";
    for r in p["skills"].as_array().into_iter().flatten() {
        s += &format!("  {}: {}\n", r["title"].as_str().unwrap_or(""), frac(r));
    }
    let f = &p["forecast"];
    s += &format!(
        "Прогноз FSRS против факта: ожидалось {} верных, верно {} из {}\nСамопрогноз против факта: {} против {} за {} блоков\n",
        f["fsrs"]["expected"], f["fsrs"]["actual"], f["fsrs"]["n"], f["self"]["predicted"], f["self"]["actual"], f["self"]["n"]
    );
    s += &format!(
        "Очередь: сейчас {}, за сутки {}, за неделю {}\nВремя в комнате: {} мин, за неделю {} — {}\n",
        p["queue"]["now"], p["queue"]["day"], p["queue"]["week"], p["room_minutes"]["total"], p["room_minutes"]["week"],
        p["room_minutes"]["note"].as_str().unwrap_or("")
    );
    s
}

/// Учебный блок в терминале. Команды вместо ответа: `?` — подсказка, `/пропуск`, `/заметка текст`,
/// `/мысль текст`, `/конец` — завершить. Конец ввода оставляет блок открытым: `whiteroom learn` продолжит.
pub fn run_block(db: &Db, inp: &mut impl std::io::BufRead, out: &mut impl std::io::Write, goal: Option<i64>, now: i64) -> Res<()> {
    let goal = match (db.block_open()?, goal) {
        (Some(_), _) => 0,
        (None, Some(g)) => g,
        (None, None) => db.goals()?.as_array().and_then(|a| a.iter().find(|g| g["archived"].is_null())).and_then(|g| g["id"].as_i64())
            .ok_or("целей нет: whiteroom goal new <тема> или whiteroom goal add <модуль.toml>")?,
    };
    let id = db.block_start(goal, None, now)?;
    let st = db.block_state(id)?;
    line(out, &format!("Блок «{}», осталось ~{} мин. Команды: ? подсказка, /пропуск, /заметка …, /мысль …, /конец.", st["title"].as_str().unwrap_or(""), st["minutes_left"]))?;
    if !st["note"].as_str().unwrap_or("").is_empty() {
        line(out, &format!("Где остановился: {}", st["note"].as_str().unwrap_or("")))?;
    }
    let read = |inp: &mut dyn std::io::BufRead, out: &mut dyn std::io::Write| -> Res<Option<String>> {
        write!(out, "> ").map_err(e)?;
        out.flush().map_err(e)?;
        let mut s = String::new();
        Ok((inp.read_line(&mut s).map_err(e)? > 0).then(|| s.trim().to_string()))
    };
    loop {
        let st = db.block_state(id)?;
        let Some(step) = st["step"].as_i64() else { break };
        let cur = &st["steps"][step as usize];
        line(out, &format!("\n── шаг {}/{}: {}", step + 1, st["steps"].as_array().map_or(0, |a| a.len()), cur["why"].as_str().unwrap_or("")))?;
        if cur["kind"] == "break" {
            line(out, &format!("Перерыв {} мин. Enter — дальше.", cur["min"]))?;
            let Some(_) = read(inp, out)? else { return Ok(()) };
            db.block_skip(id, step, now)?;
            continue;
        }
        let it = &st["item"];
        line(out, it["prompt"].as_str().unwrap_or(""))?;
        if let Some(c) = it["code"].as_str() {
            for l in c.trim_matches('\n').lines() {
                line(out, &format!("    {l}"))?;
            }
        }
        let t0 = std::time::Instant::now();
        let ans = loop {
            let Some(a) = read(inp, out)? else {
                line(out, "\nБлок сохранён. Продолжить: whiteroom learn")?;
                return Ok(());
            };
            match a.as_str() {
                "?" => line(out, &db.block_hint(id, step).unwrap_or_else(|x| x))?,
                "/пропуск" => break None,
                "/конец" => {
                    let s = db.block_close(id, None, None, now)?;
                    return summary(out, &s);
                }
                _ if a.starts_with("/заметка ") => db.block_note(id, &a[a.find(' ').unwrap()..], now)?,
                _ if a.starts_with("/мысль ") => {
                    db.block_thought(id, &a[a.find(' ').unwrap()..], now)?;
                    line(out, "Записал, вернёшься после блока.")?;
                }
                _ => break Some(a),
            }
        };
        let Some(ans) = ans else {
            db.block_skip(id, step, now)?;
            continue;
        };
        let rating = if it["grading"] == "self" {
            let r = db.block_reveal(id, step)?;
            line(out, &format!("Эталон: {}\nКритерии: {}", r["answer"][0].as_str().unwrap_or(""), r["rubric"].as_str().unwrap_or("сравни с эталоном")))?;
            line(out, "Оценка: 1 — не вспомнил, 2 — с трудом, 3 — вспомнил")?;
            match read(inp, out)?.as_deref() {
                Some("3") => Some("good"),
                Some("2") => Some("hard"),
                Some(_) => Some("again"),
                None => return Ok(()),
            }
        } else {
            None
        };
        let r = db.block_answer(id, step, &ans, rating, t0.elapsed().as_millis() as i64, now)?;
        if r["ok"] == true {
            line(out, "✓")?;
        } else {
            line(out, &format!("✗ верно: {}. {}", r["key"].as_str().unwrap_or(""), r["explain"].as_str().unwrap_or("")))?;
        }
    }
    let s = db.block_close(id, None, None, now)?;
    summary(out, &s)
}

fn summary(out: &mut impl std::io::Write, s: &Value) -> Res<()> {
    let n = s["answers"].as_array().map_or(0, |a| a.len());
    line(out, &format!("\n══ Итог: самостоятельно верно {} из {n}, с подсказкой {}, отложено {}.", s["independent_correct"], s["helped"], s["skipped"]))?;
    for a in s["answers"].as_array().into_iter().flatten().filter(|a| a["ok"] == false) {
        line(out, &format!("✗ {} — верно: {}", a["prompt"].as_str().unwrap_or("").lines().last().unwrap_or(""), a["key"].as_str().unwrap_or("")))?;
    }
    if let Some(d) = s["answers"].as_array().into_iter().flatten().filter_map(|a| a["due"].as_i64()).min() {
        line(out, &format!("Первое повторение вернётся {}.", date(d)))?;
    }
    for t in s["thoughts"].as_array().into_iter().flatten() {
        line(out, &format!("Мысль на потом: {}", t["text"].as_str().unwrap_or("")))?;
    }
    Ok(())
}
