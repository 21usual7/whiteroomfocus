// WHITE ROOM 1.0: «Сегодня», цель, план и блок, прогресс, материалы, «что WHITE ROOM видит».
// Все правила — в ядре (whiteroom_core::learn); окно показывает ответы команд и шлёт действия.
const learnView = $("learn-view");
const learnMessage = $("learn-message");
const STEP = { diag: "Диагностика", review: "Повторение", new: "Новое задание", check: "Итоговая проверка", break: "Перерыв" };
const STATUS = { answered: "отвечено", skipped: "отложено", done: "пройдено", current: "сейчас", todo: "впереди" };
const STAGE = { worked: "разобранный пример", faded: "пример с пропуском", solo: "самостоятельно", explain: "объясни своими словами", transfer: "перенос" };
const DOMAIN = { cyber: "Кибербезопасность", math: "Математика", code: "Код", german: "Немецкий", research: "Исследования", custom: "Своё" };

const when = (t, time) => t == null ? "" : new Date(t * 1000).toLocaleDateString("ru-RU",
  time ? { day: "numeric", month: "long", hour: "2-digit", minute: "2-digit" } : { day: "numeric", month: "long" });

function labeled(label, node, value) {
  if (value !== undefined) node.value = value ?? "";
  const wrap = element("label", label + " ");
  wrap.append(node);
  return wrap;
}

function actions(...nodes) {
  const box = element("div");
  box.className = "tutor-actions";
  box.append(...nodes.filter(Boolean));
  return box;
}

function info(title, detail) {
  const box = element("div");
  box.append(element("strong", title));
  if (detail) box.append(element("small", detail));
  return box;
}

function heading(text) {
  const node = element("h2", text);
  node.tabIndex = -1;
  return node;
}

function list(items, render) {
  const node = element("ol");
  node.className = "learn-list";
  node.append(...items.map((item, i) => {
    const row = element("li");
    row.append(...render(item, i));
    return row;
  }));
  return node;
}

// Кнопки блокируются до ответа ядра (fieldset disabled): двойной клик не шлёт второй запрос.
async function go(task, view = learnView, message = learnMessage) {
  const next = await run(view, message, task);
  if (!view.closest("section").hidden) (next ?? view.querySelector("button, input, textarea, select"))?.focus();
}

function startWith(task) {
  const tab = document.querySelector('nav [aria-controls="start"]');
  startTask = task;
  tab.getAttribute("aria-pressed") === "true" ? load.start() : tab.click();
}
let startTask = null;

function openModule(id) {
  matTask = async () => openEditor(await invoke("module_get", { id }));
  document.querySelector('nav [aria-controls="materials"]').click();
}
let matTask = null;

// Выбор темы — главный вход в учёбу: каждая тема — «дверь» со встроенной траекторией из ядра.
function themes(domains) {
  const box = element("ul");
  box.className = "learn-themes";
  box.append(...domains.map(d => {
    const door = button("", () => go(async () => {
      const id = await read("goal_from_domain", { domain: d.domain });
      const next = await showPlan(id);
      if (d.domain === "custom") {
        const note = element("div");
        note.className = "learn-note";
        note.append(element("p", `Это шаблон «${d.title}»: навыки и вопросы в нём — заготовка. Впиши свою тему, навыки и вопросы в «Материалах».`),
          button("Открыть в «Материалах»", () => openModule(d.module)));
        learnView.querySelector(".learn-head, h2").after(note);
      }
      return next;
    }));
    door.className = "learn-door";
    door.dataset.domain = d.domain;
    if (d.goals) door.dataset.goals = d.goals;
    const parts = [["strong", d.label], ["span", d.title, "learn-door-title"], ["span", "", "learn-door-skills"],
      ...(d.goals ? [["span", d.goals === 1 ? "Цель уже есть" : `Целей: ${d.goals}`, "learn-door-mark"]] : [])]
      .map(([tag, text, cls]) => Object.assign(element(tag, text), cls ? { className: cls } : {}));
    parts[2].append(...d.skills.map(s => element("span", s)));
    door.append(...parts);
    const row = element("li");
    row.append(door);
    return row;
  }));
  return box;
}

// ───── Сегодня ─────
async function showToday() {
  const [t, domains] = await Promise.all([read("today", { offset: -new Date().getTimezoneOffset() * 60 }),
    read("domains").catch(() => [])]);
  const head = element("div");
  head.className = "learn-head";
  head.append(heading("Сегодня"), element("p", new Date().toLocaleDateString("ru-RU", { weekday: "long", day: "numeric", month: "long" })));
  const nodes = [head];
  const spread = t.actions.find(a => a.spread);
  if (t.gap_note || spread) {
    const note = element("div");
    note.className = "learn-note";
    if (t.gap_note) note.append(element("p", t.gap_note));
    if (spread) {
      const days = Math.min(30, Math.max(2, Math.ceil(spread.count / 20)));
      note.append(button(`Распределить на ${days} ${plural(days, "день", "дня", "дней")}`, () => go(async () => {
        const n = await invoke("spread_reviews", { days });
        const next = await showToday();
        learnMessage.textContent = `Повторения (${n}) распределены на ${days} ${plural(days, "день", "дня", "дней")}.`;
        return next;
      })));
    }
    nodes.push(note);
  }
  const verbs = {
    block: (a) => [button("Продолжить блок", () => go(() => showBlock(a.block)))],
    test: () => [button("Открыть проверку", () => act(["", "test"]))],
    reviews: () => [button("Повторить", () => act(["", "reviews"]))],
    goal: (a) => [button("План блока", () => go(() => showPlan(a.goal))), button("Цель", () => go(() => showGoal(a.goal)))],
  };
  if (t.actions.length) {
    const todo = list(t.actions, (a) => [info(a.title, [a.reason, a.due != null && "срок: " + when(a.due)].filter(Boolean).join(" · ")), actions(...verbs[a.kind](a))]);
    todo.classList.add("learn-todo");
    nodes.push(todo);
  } else if (t.empty) {
    nodes.push(element("h3", "Выбери тему"),
      element("p", "WHITE ROOM соберёт цель, план и первый блок. Темы встроены в приложение и работают без сети."),
      themes(domains),
      actions(button("Создать цель из своего модуля", () => document.querySelector('nav [aria-controls="materials"]').click())));
  } else {
    nodes.push(element("p", "На сегодня дел нет. Ниже — когда появятся следующие."));
  }
  if (!t.empty && domains.length) {
    const other = element("details");
    other.className = "learn-more";
    other.append(element("summary", "Новая цель: другая тема"), themes(domains));
    nodes.push(other);
  }
  const goals = t.goals.filter(g => g.archived == null);
  if (goals.length && !t.empty) {
    const all = element("details");
    all.append(element("summary", `Все цели (${goals.length})`),
      list(goals, (g) => [info(g.title, `новых заданий ${g.left}`), actions(button("Цель", () => go(() => showGoal(g.id))))]));
    nodes.push(all);
  }
  if (t.upcoming.length) {
    nodes.push(element("h3", "Дальше"), list(t.upcoming, (u) => [info(u.title, when(u.at, true))]));
  }
  learnView.replaceChildren(...nodes);
  return learnView.querySelector(".learn-todo button, #learn-view > .learn-themes .learn-door, .learn-list button, .tutor-actions button");
}

// ───── Цель ─────
function dateValue(t) {
  if (t == null) return "";
  const d = new Date(t * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

async function showGoal(id) {
  const g = (await read("goals")).find(x => x.id === id);
  if (!g) throw new Error("Цели нет.");
  const skills = g.skills.map(s => ({ ...s, needs: [...s.needs] }));
  const title = labeled("Название", element("input"), g.title);
  const skillList = element("div");
  const drawSkills = (focus) => {
    skillList.replaceChildren(list(skills, (s, i) => {
      const move = (d) => {
        skills.splice(i + d, 0, skills.splice(i, 1)[0]);
        drawSkills([s.id, d]);
      };
      const up = button("↑", () => move(-1));
      const down = button("↓", () => move(1));
      up.setAttribute("aria-label", `Выше: ${s.title}`);
      down.setAttribute("aria-label", `Ниже: ${s.title}`);
      up.disabled = i === 0;
      down.disabled = i === skills.length - 1;
      const needs = element("fieldset");
      needs.append(element("legend", "Нужно раньше"), ...skills.filter(o => o.id !== s.id).map(o => {
        const box = element("input");
        box.type = "checkbox";
        box.checked = s.needs.includes(o.id);
        box.addEventListener("change", () => {
          s.needs = box.checked ? [...s.needs, o.id] : s.needs.filter(n => n !== o.id);
        });
        const label = element("label");
        label.append(box, " " + o.title);
        return label;
      }));
      if (focus?.[0] === s.id) setTimeout(() => (focus[1] < 0 && !up.disabled ? up : down.disabled ? up : down).focus());
      return [info(s.title, `пройдено ${s.seen}/${s.practice} · диагностика: ${s.diag ? (s.prepared ? "пройдена" : "есть") : "нет"}`), actions(up, down), needs];
    }));
  };
  drawSkills();
  const materials = element("div");
  const addMaterial = (m = { label: "", src: "", scope: "" }) => {
    const row = element("div");
    row.className = "learn-material";
    const fields = [["Название", "label"], ["Ссылка https://… или путь", "src"], ["Что используется", "scope"]]
      .map(([label, key]) => labeled(label, element("input"), m[key]));
    const remove = button("Убрать", () => { row.remove(); addButton.focus(); });
    row.append(...fields, remove);
    materials.append(row);
    return fields[0].querySelector("input");
  };
  const addButton = button("Добавить материал", () => addMaterial().focus());
  g.materials.forEach(m => addMaterial(m));
  const deadline = element("input");
  deadline.type = "date";
  const minutes = element("input");
  minutes.type = "number";
  minutes.min = 10;
  minutes.max = 180;
  const save = button("Сохранить", () => go(async () => {
    const value = deadline.value;
    await invoke("goal_update", { id, edit: {
      title: title.querySelector("input").value,
      skills: skills.map(s => s.id),
      materials: [...materials.children].map(row => {
        const [label, src, scope] = [...row.querySelectorAll("input")].map(i => i.value.trim());
        return { label, src, scope };
      }),
      deadline: value ? Math.floor(new Date(value + "T23:59:59").getTime() / 1000) : null,
      block_min: Number(minutes.value),
    } });
    for (const s of skills) {
      const before = g.skills.find(x => x.id === s.id).needs;
      if (before.length !== s.needs.length || s.needs.some(n => !before.includes(n))) await invoke("prereq_set", { skill: s.id, needs: s.needs });
    }
    const next = await showGoal(id);
    learnMessage.textContent = "Цель сохранена.";
    return next;
  }));
  save.className = "primary";
  learnView.replaceChildren(heading("Цель"), title,
    element("h3", "Навыки по порядку"),
    element("p", "Навык всегда идёт после своих предпосылок, даже если в списке он выше."), skillList,
    element("h3", "Материалы"), materials, addButton,
    labeled("Срок (необязательно)", deadline, dateValue(g.deadline)),
    labeled("Длительность блока, мин", minutes, g.block_min),
    actions(save, button("План блока", () => go(() => showPlan(id))),
      button("В архив", () => go(async () => { await invoke("goal_archive", { id }); return showToday(); })),
      button("Назад", () => go(showToday))));
  return title.querySelector("input");
}

// ───── План и блок ─────
async function showPlan(goal) {
  const open = await invoke("block_open");
  if (open != null) {
    const next = await showBlock(open);
    learnMessage.textContent = "Уже идёт блок — продолжаем его.";
    return next;
  }
  const g = (await read("goals")).find(x => x.id === goal);
  const original = await read("plan_preview", { goal });
  const plan = original.map(s => ({ ...s }));
  const steps = element("div");
  const back = button("Назад", () => go(showToday));
  const drawPlan = (focus) => {
    steps.replaceChildren(plan.length ? list(plan, (s, i) => {
      const min = element("input");
      min.type = "number";
      min.min = 1;
      min.max = 60;
      min.addEventListener("change", () => { s.min = Number(min.value); });
      const move = (d) => {
        plan.splice(i + d, 0, plan.splice(i, 1)[0]);
        drawPlan([s, d]);
      };
      const up = button("↑", () => move(-1));
      const down = button("↓", () => move(1));
      const remove = button("Убрать", () => {
        plan.splice(i, 1);
        drawPlan();
        (steps.querySelectorAll("li")[i] ?? steps.querySelector("li:last-child"))?.querySelector("input")?.focus();
      });
      const name = STEP[s.kind] + (s.item ? " · " + s.item : "");
      up.setAttribute("aria-label", `Выше: ${name}`);
      down.setAttribute("aria-label", `Ниже: ${name}`);
      remove.setAttribute("aria-label", `Убрать: ${name}`);
      up.disabled = i === 0;
      down.disabled = i === plan.length - 1;
      if (focus?.[0] === s) setTimeout(() => (focus[1] < 0 && !up.disabled ? up : down.disabled ? up : down).focus());
      return [info(name, s.why), actions(labeled("мин", min, s.min), up, down, remove)];
    }) : element("p", "В плане не осталось шагов."));
  };
  drawPlan();
  const predicted = element("input");
  predicted.type = "number";
  predicted.min = 0;
  const start = button("Начать блок", () => go(async () => {
    const open = await invoke("block_open");
    if (open != null) return showBlock(open);
    const id = await read("block_start", { goal, predicted: predicted.value === "" ? null : Number(predicted.value) });
    if (JSON.stringify(plan) !== JSON.stringify(original)) await invoke("block_plan_set", { id, plan });
    return showBlock(id);
  }));
  start.className = "primary";
  if (!original.length) {
    learnView.replaceChildren(heading(`План · ${g?.title ?? ""}`),
      element("p", "На сегодня в цели нет заданий: повторений нет, новые задания пройдены."), actions(back));
    return back;
  }
  learnView.replaceChildren(heading(`План · ${g?.title ?? ""}`),
    element("p", `Длительность блока — ${g?.block_min ?? "?"} мин. Можно поменять порядок и минуты, убрать шаги и перерывы.`), steps,
    labeled("Сколько отвечу верно (необязательно)", predicted), actions(start, back));
  return start;
}

function exprPreview(node, text) {
  // Предпросмотр без библиотек: ** и ^ → верхний индекс, * → «·».
  const source = text.replace(/\*\*/g, "^").replace(/\*/g, "·");
  const re = /\^(\([^()]*\)|[\w.]+)/g;
  let last = 0;
  let m;
  node.replaceChildren("Формула: ");
  while ((m = re.exec(source))) {
    node.append(source.slice(last, m.index), element("sup", m[1].replace(/^\((.*)\)$/, "$1")));
    last = re.lastIndex;
  }
  node.append(source.slice(last));
}

function answerInput(kind) {
  const multi = kind === "code" || kind === "explain";
  const input = element(multi ? "textarea" : "input");
  input.autocomplete = "off";
  input.spellcheck = kind === "explain";
  if (multi) input.rows = kind === "code" ? 8 : 4;
  if (kind === "code") input.className = "code";
  return input;
}

async function showBlock(id) {
  const s = await read("block_state", { id });
  if (s.closed) return showSummary(await read("block_summary", { id }));
  const work = element("div");
  work.className = "learn-work";
  const tools = actions(
    button("Где остановился", () => ask("Где остановился", s.note, async (note) => { await invoke("block_note", { id, note }); s.note = note; return "Заметка сохранена."; })),
    button("Мысль на потом", () => ask("Мысль на потом", "", async (text) => { await invoke("block_thought", { id, text }); return "Мысль записана, вернёшься к ней в итоге."; })),
    button("Завершить", () => { work.replaceChildren(...finish(s)); work.querySelector("select").focus(); }));
  const overview = element("details");
  overview.open = matchMedia("(min-width: 761px)").matches;
  overview.append(element("summary", "План блока"), list(s.steps, (st) => {
    const row = info(`${STEP[st.kind]} · ${st.min} мин`, STATUS[st.status]);
    row.dataset.status = st.status;
    return [row];
  }));
  const step = s.step == null ? null : s.steps[s.step];
  const progress = element("p", step ? `Шаг ${s.step + 1} из ${s.steps.length} · осталось около ${s.minutes_left} мин` : "Все шаги пройдены");
  const head = element("div");
  head.className = "learn-head";
  head.append(heading(s.title), progress, overview);
  learnView.replaceChildren(head, work, tools);
  if (!step) {
    work.replaceChildren(...finish(s));
    return work.querySelector("select");
  }
  const why = element("p", "Почему этот шаг: " + step.why);
  why.className = "learn-why";
  if (step.kind === "break") {
    const end = Date.now() + step.min * 60000;
    const timer = element("p");
    timer.className = "learn-timer";
    const skip = button("Пропустить", () => go(async () => { await invoke("block_skip", { id, step: s.step }); return showBlock(id); }));
    const tick = () => {
      const left = Math.max(0, Math.round((end - Date.now()) / 1000));
      timer.textContent = left ? clock(left) : "Перерыв окончен";
      if (!left) skip.textContent = "Дальше";
      if (left && timer.isConnected) setTimeout(tick, 1000);
    };
    tick();
    work.replaceChildren(why, element("h3", `Перерыв · ${step.min} мин`), timer, skip);
    return skip;
  }
  const it = s.item;
  const opened = performance.now();
  const meta = [STEP[step.kind], STAGE[it.stage], it.source && "источник: " + it.source].filter(Boolean).join(" · ");
  const prompt = element("h2", it.prompt);
  prompt.className = "prose";
  const parts = [why, element("small", meta), prompt];
  if (it.code) parts.push(element("pre", it.code));
  const hint = element("p");
  hint.className = "prose learn-hint";
  hint.setAttribute("role", "status");
  const helped = () => { hint.dataset.note = "Подсказка открыта: этот ответ не войдёт в самостоятельные."; };
  const canHint = it.has_hint && step.kind !== "diag" && step.kind !== "check";
  if (canHint && it.hinted) {
    hint.textContent = await invoke("block_hint", { id, step: s.step });
    helped();
  }
  const hintButton = canHint && !it.hinted ? button("Подсказка", () => go(async () => {
    hint.textContent = await invoke("block_hint", { id, step: s.step });
    helped();
    hintButton.remove();
    return input ?? work.querySelector("button");
  })) : null;
  const send = (rating) => go(async () => {
    const r = await read("block_answer", { id, step: s.step, answer: input?.value ?? "", rating, ms: Math.round(performance.now() - opened) });
    if (r.ok) field.activate("recall");
    return feedback(s, r, work);
  });
  const later = button("Отложить", () => go(async () => { await invoke("block_skip", { id, step: s.step }); return showBlock(id); }));
  let input = null;
  if (it.grading === "self") {
    if (it.kind === "explain") input = answerInput("explain");
    const reveal = button("Показать эталон", () => go(async () => {
      const r = await read("block_reveal", { id, step: s.step });
      if (input) input.readOnly = true;
      const rate = [["again", "Не вспомнил"], ["hard", "С трудом"], ["good", "Вспомнил"]].map(([value, label]) => button(label, () => send(value)));
      reveal.replaceWith(element("p", "Эталон: " + r.answer.join(" | ")),
        ...(r.rubric ? [element("p", "Критерии: " + r.rubric)] : []), ...(r.explain ? [element("p", r.explain)] : []),
        element("p", "Оцени себя честно — от этого зависит, когда вопрос вернётся."), actions(...rate));
      return rate[2];
    }));
    work.replaceChildren(...parts, hintButton ?? "", hint, ...(input ? [labeled("Твоё объяснение", input)] : []), reveal, actions(later));
    return input ?? reveal;
  }
  input = answerInput(it.kind);
  input.required = true;
  const form = answerForm(it.kind === "code" ? "Код" : "Ответ", "Ответить", () => send(null), input);
  work.replaceChildren(...parts, hintButton ?? "", hint, form);
  if (it.kind === "expr") {
    const preview = element("p");
    preview.className = "learn-expr";
    input.addEventListener("input", () => exprPreview(preview, input.value));
    exprPreview(preview, "");
    form.after(preview);
  }
  work.append(actions(later));
  return input;
}

function feedback(s, r, work) {
  const next = button("Дальше", () => go(() => showBlock(s.block)));
  const out = [element("p", r.ok ? "✓ Верно" : "✗ Неверно")];
  out[0].className = "learn-verdict";
  out[0].dataset.ok = r.ok;
  out[0].setAttribute("role", "status");
  if (r.helped) out.push(element("p", "С подсказкой: в самостоятельные ответы не входит."));
  if (r.key != null) out.push(element("p", "Эталон: " + r.key));
  if (r.explain) out.push(element("p", r.explain));
  if (r.rubric) out.push(element("p", "Критерии: " + r.rubric));
  if (r.due) out.push(element("small", "Вернётся " + when(r.due, true)));
  let again = null;
  if (r.retry && s.item.grading === "auto") {
    again = button("Решить ещё раз", () => {
      const input = answerInput(s.item.kind);
      input.required = true;
      const opened = performance.now();
      const result = element("p");
      result.setAttribute("role", "status");
      const form = answerForm("Ещё раз", "Проверить", (answer) => go(async () => {
        const ok = await invoke("block_retry", { id: s.block, step: s.step, answer, ms: Math.round(performance.now() - opened) });
        result.textContent = ok ? "✓ Теперь верно. Повтор после разбора в зачёт не идёт." : "✗ Пока неверно — посмотри эталон и пояснение.";
        return ok ? next : input;
      }), input);
      again.replaceWith(form, result);
      input.focus();
    });
    out.push(again);
  }
  work.replaceChildren(...out, next);
  return again ?? next;
}

function finish(s) {
  const energy = element("select");
  energy.append(new Option("не указывать", ""), ...[1, 2, 3, 4, 5].map(n => new Option(String(n), n)));
  const reflection = element("textarea");
  reflection.rows = 3;
  const close = button("Завершить блок", () => go(async () => showSummary(await read("block_close", {
    id: s.block, energy: energy.value ? Number(energy.value) : null, reflection: reflection.value.trim() || null,
  }))));
  close.className = "primary";
  return [element("h3", "Завершить блок"), element("p", "Оставшиеся шаги не засчитываются как ошибки. Энергия и рефлексия — по желанию."),
    labeled("Энергия, 1–5", energy), labeled("Что получилось и что мешало", reflection),
    actions(close, s.step == null ? null : button("Вернуться к шагу", () => go(() => showBlock(s.block))))];
}

function showSummary(r) {
  field.pulse();
  const wrong = r.answers.filter(a => !a.ok).length;
  const nodes = [heading("Итог блока"),
    element("p", `Самостоятельно верно: ${r.independent_correct} из ${r.answers.length} · с подсказкой: ${r.helped} · ошибок: ${wrong} · отложено: ${r.skipped}`),
    list(r.answers, (a) => [info(a.prompt, `${a.ok ? "✓ верно" : "✗ неверно"}${a.helped ? " · с подсказкой" : ""} · твой ответ: ${a.answer || "—"} · эталон: ${a.key ?? "—"}`),
      ...(a.explain ? [element("p", a.explain)] : []), ...(a.due ? [element("small", "Вернётся " + when(a.due, true))] : [])])];
  if (r.note) nodes.push(element("p", "Где остановился: " + r.note));
  if (r.thoughts?.length) nodes.push(element("h3", "Мысли на потом"), list(r.thoughts, (t) => [element("p", t.text)]));
  const done = button("К делам на сегодня", () => go(showToday));
  nodes.push(actions(done));
  learnView.replaceChildren(...nodes);
  return done;
}

// Заметка и мысль: модальный <dialog>, Esc закрывает его, фокус возвращается на кнопку.
function ask(title, value, save) {
  const dialog = $("learn-dialog");
  const opener = document.activeElement;
  $("learn-dialog-title").textContent = title;
  $("learn-dialog-text").value = value ?? "";
  dialog.returnValue = "";
  dialog.onclose = () => {
    if (dialog.returnValue !== "save") return opener?.focus();
    const text = $("learn-dialog-text").value;
    go(async () => {
      const done = await save(text);
      learnMessage.textContent = done;
      return opener;
    });
  };
  dialog.showModal();
  $("learn-dialog-text").focus();
}

// ───── Прогресс ─────
function share(title, x, rowText) {
  const head = info(title, x.den ? `${x.num}/${x.den} · ${Math.round(x.num / x.den * 100)}%` : `${x.num}/${x.den}`);
  if (x.low) {
    const low = element("span", "мало данных");
    low.className = "learn-low";
    head.append(low);
  }
  const more = element("details");
  more.append(element("summary", "Как считается"), element("p", x.rule));
  if (x.rows?.length) more.append(list(x.rows, (row) => [element("small", rowText(row))]));
  return [head, more];
}

function stats(pairs) {
  const box = element("dl");
  box.className = "dash-stats";
  box.append(...pairs.map(([name, value]) => {
    const cell = element("div");
    cell.append(element("dt", name), element("dd", String(value)));
    return cell;
  }));
  return box;
}

async function loadProgress() {
  const view = $("progress-view");
  await run(view, $("progress-message"), async () => {
    const p = await read("progress");
    const verdict = (row) => `${row.item} · ${row.ok ? "верно" : "неверно"} · ${when(row.at, true)}`;
    const { fsrs, self } = p.forecast;
    view.replaceChildren(heading("Прогресс"),
      element("p", `«Мало данных» — меньше ${p.low_n} наблюдений: доля ещё ничего не говорит.`),
      element("h3", "Удержание: отложенная проверка"),
      p.retention.length ? list(p.retention, (r) => share(`Через ${r.horizon} дн. · ${DOMAIN[r.domain] ?? r.domain}`, r,
        (row) => `${verdict(row)} · после срока ${row.delay_h} ч`)) : element("p", "Закрытых проверок пока нет."),
      element("h3", "Успех на новых задачах"),
      p.skills.length ? list(p.skills, (s) => share(`${s.title} · ${DOMAIN[s.domain] ?? s.domain}`, s, verdict)) : element("p", "Самостоятельных первых попыток пока нет."),
      element("h3", "Прогноз против факта"),
      list([["FSRS", { ...fsrs, num: fsrs.actual, den: fsrs.n }, `ожидалось верных ${fsrs.expected} из ${fsrs.n}, факт ${fsrs.actual}`],
        ["Твой прогноз", { ...self, num: self.actual, den: self.predicted }, `блоков ${self.n}: прогноз ${self.predicted}, факт ${self.actual}`]],
      ([title, x, text]) => {
        const [head, more] = share(title, x);
        head.querySelector("small").textContent = text;
        return [head, more];
      }),
      element("h3", "Очередь повторений"),
      stats([["Сейчас", p.queue.now], ["24 ч", p.queue.day], ["7 дней", p.queue.week]]),
      element("h3", "Объём данных"),
      stats([["Ответов", p.volume.attempts], ["Заданий", p.volume.items], ["Карточек", p.volume.cards],
        ["Целей", p.volume.goals], ["Блоков", p.volume.blocks], ["Показов проверок", p.volume.checks]]),
      element("h3", "Время в комнате · контекст"),
      stats([["Всего", `${p.room_minutes.total} мин`], ["7 дней", `${p.room_minutes.week} мин`]]),
      element("p", p.room_minutes.note));
  });
}

// ───── Материалы ─────
const matView = $("materials-view");
const matMessage = $("materials-message");
const matGo = (task) => go(task, matView, matMessage);
const ITEM_FIELDS = [
  ["prompt", "Вопрос", "textarea"], ["answer", "Ответ (варианты — с новой строки)", "textarea"], ["explain", "Пояснение", "textarea"],
  ["hint", "Подсказка", "input"], ["kind", "Тип", ["number", "text", "expr", "code", "card", "explain"]],
  ["role", "Роль", ["practice", "diag", "test7", "test30"]], ["stage", "Шаг маршрута", ["", "worked", "faded", "solo", "explain", "transfer"]],
  ["source", "Источник", "input"], ["rubric", "Критерии самооценки", "input"], ["code", "Код или журнал", "textarea"],
  ["tests", "Тесты Python (для code)", "textarea"], ["domain", "Область переменных (для expr)", ["", "positive", "real"]],
];

async function renderModules() {
  const mods = await read("modules");
  const file = element("input");
  file.type = "file";
  file.accept = ".toml,.tsv,.txt";
  file.addEventListener("change", () => matGo(async () => {
    const f = file.files[0];
    if (!f) return;
    const text = await f.text();
    const base = f.name.replace(/\.[^.]+$/, "");
    const m = /\.toml$/i.test(f.name) ? await invoke("module_parse", { text })
      : await invoke("anki_import", { tsv: text, id: base.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "anki", title: base });
    return openEditor(m);
  }));
  const tsv = element("textarea");
  tsv.readOnly = true;
  tsv.rows = 6;
  tsv.hidden = true;
  tsv.setAttribute("aria-label", "TSV для Anki");
  matView.replaceChildren(heading("Материалы"),
    mods.length ? list(mods, (m) => [info(m.title, `${DOMAIN[m.domain] ?? m.domain} · навыков ${m.skills} · редакций ${m.revisions}`),
      actions(button("Открыть", () => matGo(async () => openEditor(await invoke("module_get", { id: m.id })))),
        button("Создать цель из модуля", () => matGo(async () => {
          const goal = await read("goal_from_module", { id: m.id });
          startWith(() => showGoal(goal));
        })),
        button("Экспорт TSV", () => matGo(async () => {
          tsv.value = await invoke("anki_export", { id: m.id });
          tsv.hidden = false;
          return tsv;
        })))]) : element("p", "Модулей пока нет. Импортируй файл TOML или колоду Anki (TSV)."),
    tsv, labeled("Импорт: модуль TOML или Anki TSV", file));
}

function openEditor(m) {
  const save = button("Сохранить", () => matGo(async () => {
    await invoke("module_save", { module: m });
    matMessage.textContent = `Модуль «${m.title}» сохранён.`;
    save.disabled = true;
    return undo;
  }));
  save.className = "primary";
  save.disabled = true;
  const preview = element("div");
  preview.className = "learn-note";
  preview.hidden = true;
  const items = element("div");
  const itemEditor = (it, open) => {
    const box = element("details");
    box.open = open;
    const summary = element("summary", `${it.id} · ${it.prompt.slice(0, 60) || "новый вопрос"}`);
    const objective = element("select");
    objective.append(...m.objective.map(o => new Option(o.title, o.id)));
    const fields = [labeled("Навык", objective, it.objective)];
    objective.addEventListener("change", () => { it.objective = objective.value; });
    for (const [key, label, type] of ITEM_FIELDS) {
      const node = element(Array.isArray(type) ? "select" : type);
      if (Array.isArray(type)) node.append(...type.map(v => new Option(v || "—", v)));
      if (type === "textarea") node.rows = key === "code" || key === "tests" ? 4 : 2;
      if (key === "code" || key === "tests") node.className = "code";
      const value = key === "answer" ? it.answer.join("\n") : it[key] ?? "";
      node.addEventListener("input", () => {
        if (key === "answer") it.answer = node.value.split("\n").map(a => a.trim()).filter(Boolean);
        else it[key] = node.value === "" && !["prompt", "explain", "kind", "role"].includes(key) ? null : node.value;
        if (key === "prompt") summary.textContent = `${it.id} · ${it.prompt.slice(0, 60)}`;
      });
      fields.push(labeled(label, node, value));
    }
    const remove = button("Удалить вопрос", () => {
      m.item.splice(m.item.indexOf(it), 1);
      const next = box.nextElementSibling ?? box.previousElementSibling;
      box.remove();
      save.disabled = true;
      (next?.querySelector("summary") ?? add).focus();
    });
    box.append(summary, ...fields, remove);
    return box;
  };
  items.append(...m.item.map(it => itemEditor(it, false)));
  items.addEventListener("input", () => { save.disabled = true; });
  items.addEventListener("change", () => { save.disabled = true; });
  const add = button("Добавить вопрос", () => {
    let n = m.item.length + 1;
    while (m.item.some(i => i.id === `${m.id}-q${n}`)) n += 1;
    const it = { id: `${m.id}-q${n}`, v: 1, objective: m.objective[0]?.id ?? "", role: "practice", kind: "text", prompt: "", answer: [], explain: "", run: false };
    m.item.push(it);
    const box = itemEditor(it, true);
    items.append(box);
    save.disabled = true;
    box.querySelector("textarea").focus();
  });
  const check = button("Проверить изменения", () => matGo(async () => {
    const p = await read("module_preview", { module: m });
    preview.replaceChildren(element("p", `Новых: ${p.added.length} · изменённых: ${p.changed.length} · без изменений: ${p.unchanged}`),
      list(p.added, (x) => [element("small", `+ ${x.id}: ${x.prompt}`)]),
      list(p.changed, (x) => [element("small", `~ ${x.id}: ${x.prompt} — было v${x.from_v}, попыток ${x.attempts}; они останутся при прежней версии`)]));
    if (p.removed_kept_in_history.length) preview.append(element("p", "Нет в файле, остаются в истории: " + p.removed_kept_in_history.join(", ")));
    if (p.pilot_objectives.length) preview.append(element("p", "Навыки уже в пилоте, группа не меняется: " + p.pilot_objectives.join(", ")));
    preview.hidden = false;
    save.disabled = false;
    return save;
  }));
  const undo = button("Отменить последнее сохранение", () => matGo(async () => {
    await invoke("module_undo", { id: m.id });
    const next = openEditor(await invoke("module_get", { id: m.id }));
    matMessage.textContent = "Предыдущая редакция восстановлена.";
    return next;
  }));
  const title = labeled("Название модуля", element("input"), m.title);
  title.querySelector("input").addEventListener("input", (e) => { m.title = e.target.value; save.disabled = true; });
  matView.replaceChildren(heading(`Модуль ${m.id}`), title, element("h3", `Вопросы (${m.item.length})`), items, add,
    actions(check, save), preview,
    actions(undo, button("Создать цель из модуля", () => matGo(async () => {
      const goal = await read("goal_from_module", { id: m.id });
      startWith(() => showGoal(goal));
    })), button("К списку", () => matGo(renderModules))));
  return title.querySelector("input");
}

// ───── Что WHITE ROOM видит ─────
const size = (n) => n == null ? "нет файла" : n < 1024 ? `${n} Б` : n < 1048576 ? `${(n / 1024).toFixed(1)} КБ` : `${(n / 1048576).toFixed(1)} МБ`;
const yes = (b) => b ? "да" : "нет";

async function renderSeen() {
  const [s, v] = await Promise.all([read("seen"), read("versions")]);
  const c = s.capture;
  const versions = element("p", `Окно ${v.app} · схема базы ${v.schema} · CLI ${v.cli ?? "не найден"}`);
  const seenGo = (task) => go(task, $("seen-view"), $("seen-message"));
  const copy = button("Сделать копию", () => seenGo(async () => {
    const path = await invoke("backup");
    await renderSeen();
    $("seen-message").textContent = "Копия сохранена: " + path;
  }));
  const r = s.stored.rows;
  // Очистка: сначала apply:false (только счётчики), удаление — после «Удалить» в диалоге.
  const clear = (a) => {
    const open = button("Очистить записи активности", () => seenGo(async () => {
      const p = await read("activity_clear", { session: a.session, apply: false });
      const dialog = $("clear-dialog");
      $("clear-dialog-body").replaceChildren(
        element("p", `${a.topic}: окон ${p.windows}, страниц ${p.pages}, выдержек ${p.excerpts}.`),
        element("p", p.kept), element("p", p.elsewhere));
      dialog.returnValue = "";
      dialog.onclose = () => {
        if (dialog.returnValue !== "apply") return open.focus();
        seenGo(async () => {
          await read("activity_clear", { session: a.session, apply: true });
          await renderSeen();
          $("seen-message").textContent = `Удалено: окон ${p.windows}, страниц ${p.pages}, выдержек ${p.excerpts}.`;
          return $("seen-view").querySelector('[aria-label^="Очистить"]');
        });
      };
      dialog.showModal();
      return dialog.querySelector('[value=""]');
    }));
    open.setAttribute("aria-label", `Очистить записи активности: ${a.topic}`);
    return open;
  };
  $("seen-view").replaceChildren(heading("Что WHITE ROOM видит"),
    ...(v.match ? [] : [Object.assign(element("p", v.message), { className: "notice" })]),
    element("h3", "Запись сейчас"),
    element("p", c.session == null ? "Сессии нет." : `Сессия «${c.topic}»: окна — ${c.windows ? "записываются" : "не записываются"}, вкладки — ${c.browser ? "записываются" : "не записываются"}.`),
    element("p", "Как отозвать: " + c.revoke),
    element("h3", "Что хранится"),
    element("p", `База: ${s.stored.db} · ${size(s.stored.bytes)} · журнал: ${size(s.stored.journal)} · WAL: ${size(s.stored.wal)}`),
    stats([["Окна", r.windows], ["Страницы", r.pages], ["Ответы", r.attempts], ["Сообщения тьютору", r.tutor_messages], ["Черновики Claude", r.drafts]]),
    element("h3", `Копии (${s.stored.copies.length})`),
    s.stored.copies.length ? list(s.stored.copies, (x) => [info(x.path, size(x.bytes))]) : element("p", "Копий пока нет."),
    copy,
    element("h3", "Записи активности"),
    s.activity.length ? list(s.activity, (a) => [info(a.topic, [a.started && when(a.started, true), `окон ${a.windows} · страниц ${a.pages}`].filter(Boolean).join(" · ")), actions(clear(a))])
      : element("p", "Записей окон и страниц нет."),
    element("h3", "Интеграции"),
    list([["Мост расширения Chromium", s.integrations.native_host], ["Таймер синхронизации с D1", s.integrations.d1_sync_timer],
      ["Ключ Claude в окружении", s.integrations.claude_key_in_env]], ([name, on]) => [info(name, yes(on))]),
    list(s.notes, (n) => [element("p", n)]),
    versions);
}

Object.assign(load, {
  start: () => {
    const task = startTask ?? showToday;
    startTask = null;
    return go(task);
  },
  progress: loadProgress,
  materials: () => {
    const task = matTask ?? renderModules;
    matTask = null;
    return matGo(task);
  },
  seen: () => go(renderSeen, $("seen-view"), $("seen-message")),
});
load.start();
