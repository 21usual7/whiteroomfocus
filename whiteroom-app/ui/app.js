const invoke = window.__TAURI__.core.invoke;
const $ = (id) => document.getElementById(id);

async function read(command, args) {
  return JSON.parse(await invoke(command, args));
}

function element(tag, text) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  return node;
}

function button(label, onClick) {
  const node = element("button", label);
  node.type = "button";
  node.addEventListener("click", onClick);
  return node;
}

function answerForm(label, submitLabel, onSubmit, input = element("input")) {
  const form = element("form");
  const field = element("label", label + " ");
  field.append(input);
  form.append(field, element("button", submitLabel));
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    onSubmit(input.value);
  });
  return form;
}

const errorText = (error) => error instanceof Error ? error.message : String(error);

// Пока открыт недельный блок, ядро закрывает тьютора, подсказки и ключи; окно прячет их кнопки.
let selfTest = false;
function syncHelp() {
  const why = selfTest ? "Идёт недельная проверка: тьютор, подсказки и разбор с Claude откроются после последнего ответа." : "";
  for (const node of [$("open-tutor"), $("review-session"), document.querySelector('nav [aria-controls="tutor"]')]) {
    node.disabled = selfTest;
    node.title = why;
  }
  $("help-lock").textContent = why;
  $("help-lock").hidden = !selfTest;
}

async function run(controls, message, task) {
  if (controls.disabled) return;
  controls.disabled = true;
  message.textContent = "";
  try {
    return await task();
  } catch (error) {
    message.textContent = error instanceof Error ? error.message : String(error);
  } finally {
    controls.disabled = false;
  }
}

const studyControls = $("study-controls");
const studyMessage = $("study-message");
let started = null;
let activeStudyId = null;
let modes = [];
const focusForm = $("start-study");
let historyRows = [];
let atlas = { challenges: [], due: 0, fresh: 0 };
let focusGoal = 0;
const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

// Цель по времени живёт только в окне: в базе её нет, поэтому храним по id сессии.
function goalKey(id) { return "wr-goal-" + id; }
function storedGoal(id) {
  try { return Number(localStorage.getItem(goalKey(id))) || 0; } catch { return 0; }
}
function storeGoal(id, minutes) {
  try { if (minutes) localStorage.setItem(goalKey(id), String(minutes)); } catch {}
}

// Поле строится только из реальных записей: история фокуса и очередь повторений.
function drawAtlas() {
  if (!modes.length) return;
  const minutes = modes.map(mode => historyRows
    .filter(row => row[3] === mode.id)
    .reduce((sum, [start, end]) => sum + Math.max(0, end - start) / 60, 0));
  field.setData(minutes, atlas.due);
  const solved = atlas.challenges.filter(row => row[5] === "решён").length;
  const total = Math.round(minutes.reduce((a, b) => a + b, 0));
  $("readouts").replaceChildren(...[
    ["К повторению", atlas.due, atlas.fresh ? `+${atlas.fresh} новых` : ""],
    ["Задачи", `${solved}/${atlas.challenges.length}`, "решено"],
    brief
      ? ["Фокус · 7 дней", brief.week.minutes, `мин · ${brief.week.days} ${plural(brief.week.days, "день", "дня", "дней")}`]
      : ["Фокус", total, `мин · ${historyRows.length} сесс.`],
  ].flatMap(([term, value, unit]) => {
    const data = element("dd");
    const figure = element("span", String(value));
    figure.className = "readout-value";
    data.append(figure);
    if (unit) data.append(element("small", unit));
    return [element("dt", term), data];
  }));
}

async function loadAtlas() {
  try {
    const [challenges, queue] = await Promise.all([read("challenges"), read("review_queue")]);
    atlas = {
      challenges,
      due: queue.filter(item => item[1] === "review").length,
      fresh: queue.filter(item => item[1] === "new").length,
    };
  } catch {
    // Показатели необязательны: без них форма фокуса работает как раньше.
  }
  drawAtlas();
}

const body = document.body;

function syncScene() {
  if (!$("awake").hidden && !$("awake").classList.contains("leaving")) return field.setScene("awake");
  if ($("study").hidden) return field.setScene("tab");
  if (body.classList.contains("in-room")) return field.setScene("room");
  field.setScene(body.classList.contains("setup") ? "setup" : "dash");
}

// Параметры сессии — по одному вопросу на экране, а не форма настроек.
const asks = [...focusForm.querySelectorAll(".ask")];
let askIndex = 0;
function showAsk(index) {
  askIndex = Math.max(0, Math.min(asks.length - 1, index));
  asks.forEach((ask, i) => ask.classList.toggle("current", i === askIndex));
  const last = askIndex === asks.length - 1;
  $("ask-back").hidden = askIndex === 0;
  $("ask-next").hidden = last;
  $("ask-start").hidden = !last;
  $("ask-dots").textContent = asks.map((_, i) => (i === askIndex ? "●" : "○")).join(" ");
  if (last) {
    const goal = $("focus-goal").selectedOptions[0].text.toLowerCase();
    $("commit-text").textContent = `Тема: «${focusForm.elements.topic.value.trim()}». Ты думаешь, что это займёт ${$("focus-estimate").value} мин. Лимит: ${goal}.`;
  }
  asks[askIndex].querySelector("input:checked, input, select")?.focus();
}
// Дальше — только после ответа: пустые обязательные поля не пускают.
function nextAsk() {
  if ([...asks[askIndex].querySelectorAll("input, select")].every(field => field.reportValidity())) showAsk(askIndex + 1);
}
$("ask-back").addEventListener("click", () => showAsk(askIndex - 1));
$("ask-next").addEventListener("click", nextAsk);
async function renderApps() {
  const apps = await read("blocked_apps");
  $("app-list").replaceChildren(...apps.map((name) => {
    const row = element("li");
    row.append(element("span", name), button("Убрать", () => changeApp("app_unblock", name)));
    return row;
  }));
}
async function changeApp(command, name) {
  await run(studyControls, studyMessage, async () => {
    await invoke(command, { name });
    $("app-input").value = "";
    await renderApps();
  });
}
$("app-add").addEventListener("click", () => changeApp("app_block", $("app-input").value));
$("app-input").closest(".ask").querySelectorAll("[data-app]").forEach(item =>
  item.addEventListener("click", () => changeApp("app_block", item.dataset.app)));
focusForm.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && event.target.id === "app-input") {
    event.preventDefault();
    changeApp("app_block", event.target.value);
    return;
  }
  if (event.key === "Enter" && askIndex < asks.length - 1 && event.target.tagName !== "BUTTON") {
    event.preventDefault();
    nextAsk();
  }
});

function openSetup() {
  body.classList.add("setup");
  showAsk(0);
  run(studyControls, studyMessage, renderApps);
  syncScene();
  focusForm.querySelector("[name=mode]:checked")?.focus();
}

function closeSetup() {
  body.classList.remove("setup");
  syncScene();
  $("open-setup").focus();
}

// Переход в комнату: интерфейс уходит, частицы выстраиваются в кольца. Esc или кнопка пропускают паузу.
let skipEntry = null;
function pause(ms) {
  if (reducedMotion()) return Promise.resolve();
  return new Promise((resolve) => {
    const done = () => { clearTimeout(timer); skipEntry = null; resolve(); };
    const timer = setTimeout(done, ms);
    skipEntry = done;
  });
}

function skip() {
  if (!skipEntry) return;
  field.skip();
  skipEntry();
}

$("open-setup").addEventListener("click", openSetup);
$("close-setup").addEventListener("click", closeSetup);
$("skip-entry").addEventListener("click", skip);
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (dialogTutor) closeTutorDialog();
  else if (skipEntry) skip();
  else if (!$("awake").hidden) closeAwake();
  else if (body.classList.contains("setup") && !body.classList.contains("in-room") && !studyControls.disabled) closeSetup();
});

const motionLabels = { live: "Фон: живой", quiet: "Фон: тихий", off: "Фон: выкл" };
let motion = "live";
try { motion = localStorage.getItem("wr-motion") in motionLabels ? localStorage.getItem("wr-motion") : "live"; } catch {}
function setMotion(value) {
  motion = value;
  field.setMotion(value);
  $("motion").textContent = motionLabels[value];
  try { localStorage.setItem("wr-motion", value); } catch {}
}
$("motion").addEventListener("click", () => {
  setMotion({ live: "quiet", quiet: "off", off: "live" }[motion]);
});
setMotion(motion);

// ───── Брифинг и панели: только записанные данные из команды briefing. ─────
let brief = null;
let awakeChecked = false;

function plural(n, one, few, many) {
  const mod10 = n % 10, mod100 = n % 100;
  if (mod10 === 1 && mod100 !== 11) return one;
  return mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14) ? few : many;
}

async function loadBriefing() {
  try {
    // shortcut: смещение берётся на сегодня; если между днями был переход на летнее время, граница сдвинется на час.
    brief = await read("briefing", { offset: -new Date().getTimezoneOffset() * 60 });
    selfTest = await read("self_test_active");
  } catch {
    brief = null;
  }
  syncHelp();
  renderPanels();
  drawAtlas();
}

function dayLabel(start) {
  const days = Math.round((brief.today - start) / 86400);
  if (days === 1) return "Вчера";
  return "В прошлый раз · " + new Date(start * 1000).toLocaleDateString("ru-RU", { day: "numeric", month: "long" });
}

function priorities() {
  const plan = brief.plan;
  const out = [];
  if (plan.due) out.push([`Повторить ${plan.due} ${plural(plan.due, "карточку", "карточки", "карточек")}`, "reviews"]);
  for (const title of plan.tests) out.push([`${selfTest ? "Продолжить блок" : "Недельная проверка"}: ${title}`, "test"]);
  for (const title of plan.working) out.push([`Вернуться к задаче «${title}»`, "practice"]);
  if (!plan.due && plan.fresh) out.push([`Новые вопросы: ${plan.fresh}`, "reviews"]);
  const last = brief.last?.studies.at(-1);
  if (last) out.push([`Продолжить тему «${last[0]}»`, "setup", last]);
  return out.slice(0, 4);
}

function act([, target, study]) {
  if (!$("awake").hidden) closeAwake();
  if (target === "setup") {
    if (study[1]) {
      const radio = [...focusForm.elements.mode].find(input => input.value === study[1]);
      if (radio) {
        radio.checked = true;
        radio.dispatchEvent(new Event("change"));
        focusForm.elements.direction.value = study[2];
      }
    }
    focusForm.elements.topic.value = study[0];
    openSetup();
    return;
  }
  if (target === "test") weeklyRequested = true;
  const tab = document.querySelector(`nav [aria-controls="${target === "test" ? "reviews" : target}"]`);
  if (tab?.getAttribute("aria-pressed") === "true") load[tab.getAttribute("aria-controls")]();
  else tab?.click();
}

function priorityList() {
  const items = priorities();
  if (!items.length) return [element("li", "На сегодня ничего не запланировано. Можно начать сессию.")];
  return items.map((item) => {
    const row = element("li");
    row.append(button(item[0], () => act(item)));
    return row;
  });
}

function renderPanels() {
  const panels = document.querySelector(".panels");
  panels.hidden = !brief;
  if (!brief) return;
  $("plan").replaceChildren(...priorityList());

  const notes = brief.notes.slice(0, 2).map(([domain, title, excerpt]) => {
    const note = element("article");
    note.append(element("small", domain), element("strong", title), element("p", excerpt));
    return note;
  });
  $("notes").replaceChildren(...(notes.length ? notes
    : [element("p", "Заметки появятся, когда отметишь сайт кнопкой «Учебный сайт» в расширении.")]));

  const { answered, correct } = brief.recall;
  const memory = [];
  if (answered) {
    const big = element("p", Math.round(correct / answered * 100) + "%");
    big.className = "figure";
    memory.push(big, element("p", `верно ${correct} из ${answered} ${plural(answered, "повторения", "повторений", "повторений")} за 14 дней`));
  } else {
    memory.push(element("p", "За 14 дней повторений не было."));
  }
  const up = brief.upcoming;
  const max = Math.max(1, up.now, up.day, up.week);
  const bars = element("dl");
  bars.className = "bars";
  for (const [label, value] of [["сейчас", up.now], ["24 ч", up.day], ["7 дней", up.week]]) {
    const item = element("dd");
    const track = element("span");
    track.className = "bar-track";
    track.setAttribute("aria-hidden", "true");
    track.style.setProperty("--w", String(value / max * 100));
    const count = element("span", String(value));
    count.className = "bar-value";
    item.append(track, count);
    bars.append(element("dt", label), item);
  }
  memory.push(element("small", "К повторению"), bars);
  $("memory").replaceChildren(...memory);
}

function seenToday() {
  try {
    return localStorage.getItem("wr-awake") === new Date().toDateString();
  } catch {
    // Без хранилища брифинг не навязываем: его всегда можно открыть кнопкой.
    return true;
  }
}

function chapter(title, ...content) {
  const node = element("section");
  node.className = "chapter";
  node.append(element("h3", title), ...content);
  return node;
}

function showAwake() {
  if (!brief || body.classList.contains("in-room")) return;
  const now = new Date();
  const hour = now.getHours();
  $("awake-date").textContent = now.toLocaleDateString("ru-RU", { weekday: "long", day: "numeric", month: "long" });
  $("awake-title").textContent = hour >= 5 && hour < 12 ? "Доброе утро." : hour < 18 && hour >= 12 ? "Добрый день."
    : hour >= 18 ? "Добрый вечер." : "Доброй ночи.";
  const parts = [];
  const last = brief.last;
  if (last) {
    const rows = last.studies.slice(-3).map(([topic, mode, direction, minutes]) => {
      const row = element("li");
      row.append(element("strong", topic), element("span",
        [mode && modeTitle(mode), direction !== topic && direction, `${minutes} мин`].filter(Boolean).join(" · ")));
      return row;
    });
    const list = element("ul");
    list.replaceChildren(...rows);
    const total = last.studies.reduce((sum, row) => sum + row[3], 0);
    const summary = [
      total && `${total} мин фокуса`,
      last.answered && `ответов ${last.answered}, верно ${last.correct}`,
      last.solved.length && `решено: ${last.solved.join(", ")}`,
      last.objective && `цель: ${last.objective}`,
    ].filter(Boolean).join(" · ");
    parts.push(chapter(dayLabel(last.start), ...(rows.length ? [list] : []), element("p", summary || "Были только повторения.")));
  } else {
    parts.push(chapter("Начало", element("p", "Записей пока нет. Первая сессия станет точкой отсчёта.")));
  }
  if (brief.notes.length) {
    const [domain, title, excerpt] = brief.notes[0];
    const note = element("blockquote", excerpt);
    note.append(element("cite", `${title} · ${domain}`));
    parts.push(chapter("Заметки и находки", note));
  }
  if (last?.facts.length) {
    parts.push(chapter("Из твоих карточек", ...last.facts.slice(0, 1).map(([prompt, explain]) => {
      const fact = element("div");
      fact.className = "fact";
      fact.append(element("strong", prompt), element("p", explain));
      return fact;
    })));
  }
  const plan = element("ul");
  plan.className = "plan";
  plan.replaceChildren(...priorityList());
  parts.push(chapter("Приоритеты на сегодня", plan));
  parts.forEach((part, index) => part.style.setProperty("--d", String(index)));
  document.querySelector(".awake-actions").style.setProperty("--d", String(parts.length));
  $("awake-body").replaceChildren(...parts);

  const awake = $("awake");
  awake.classList.remove("leaving");
  awake.hidden = false;
  awake.scrollTop = 0;
  document.querySelector("main").inert = true;
  body.classList.add("awake");
  syncScene();
  setTimeout(() => field.activate("complete"), 700);
  // Без preventScroll фокус на кнопке внизу прокручивает брифинг и прячет заголовок.
  $("awake-go").focus({ preventScroll: true });
}

function closeAwake() {
  const awake = $("awake");
  if (awake.hidden || awake.classList.contains("leaving")) return;
  try { localStorage.setItem("wr-awake", new Date().toDateString()); } catch {}
  awake.classList.add("leaving");
  document.querySelector("main").inert = false;
  body.classList.remove("awake");
  syncScene();
  setTimeout(() => { awake.hidden = true; }, reducedMotion() ? 0 : 700);
  ($("study").hidden ? document.querySelector('nav [aria-pressed="true"]') : $("open-setup")).focus();
}

$("awake-go").addEventListener("click", closeAwake);
$("awake-skip").addEventListener("click", closeAwake);
$("replay-awake").addEventListener("click", showAwake);

function modeTitle(id) {
  return modes.find(mode => mode.id === id)?.title ?? id;
}

function selectMode(mode) {
  focusForm.elements.direction.replaceChildren(...mode.options.map(text => new Option(text, text)));
  field.setMode(modes.indexOf(mode));
}

function renderModes(catalog) {
  modes = catalog;
  $("focus-modes").replaceChildren(...modes.map((mode, index) => {
    const label = element("label");
    const input = element("input");
    input.type = "radio";
    input.name = "mode";
    input.value = mode.id;
    input.checked = index === 0;
    input.addEventListener("change", () => selectMode(mode));
    label.addEventListener("pointerenter", () => field.hover(index));
    label.addEventListener("pointerleave", () => field.hover(-1));
    label.append(input, element("span", mode.title));
    return label;
  }));
  selectMode(modes[0]);
}

async function loadHistory() {
  const rows = await read("focus_history");
  historyRows = rows;
  drawAtlas();
  $("focus-history").replaceChildren(...rows.map(([start, end, topic, mode, direction]) => {
    const row = element("li");
    const date = new Date(start * 1000).toLocaleDateString("ru-RU", { day: "numeric", month: "short" });
    const minutes = Math.floor(Math.max(0, end - start) / 60);
    row.append(element("strong", topic), element("span", [modeTitle(mode), direction === topic ? "" : direction,
      date, minutes ? minutes + " мин" : "< 1 мин"].filter(Boolean).join(" · ")));
    return row;
  }));
  $("recent-focus").hidden = !rows.length;
}

function updateTimer() {
  if (started === null) return;
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - started);
  $("timer").textContent = clock(seconds);
  const goal = focusGoal * 60;
  // Без цели кольцо отмеряет текущую минуту.
  $("dial-progress").style.strokeDashoffset = String(1 - (goal ? Math.min(1, seconds / goal) : seconds % 60 / 60));
  $("active-study").classList.toggle("is-free", !goal);
  $("active-study").classList.toggle("is-done", Boolean(goal) && seconds >= goal);
  $("dial-goal").textContent = !goal ? "без лимита" : (seconds >= goal ? "цель достигнута · " : "цель · ") + clock(goal);
}

function clock(seconds) {
  return [
    Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60,
  ].map((part) => String(part).padStart(2, "0")).join(":");
}

function showStudy(active) {
  started = active?.[0] ?? null;
  const previousStudy = activeStudyId;
  activeStudyId = active?.[3]?.[0] ?? null;
  // Конец или смена сессии закрывает тьютора, отменяет предпросмотр и сбрасывает черновик.
  if (previousStudy !== activeStudyId) {
    closeTutorDialog(false);
    if (pageTutor && !$("tutor").hidden) loadTutorPage();
  }
  $("start-study").hidden = Boolean(active);
  $("active-study").hidden = !active;
  body.classList.toggle("in-room", Boolean(active));
  if (active) body.classList.add("setup");
  syncScene();
  if (active) {
    $("study-topic").textContent = active[1];
    const selection = active[2];
    $("active-focus").textContent = selection ? [modeTitle(selection[0]), selection[1]].join(" · ") : "Учебная сессия";
    $("study-summary").hidden = true;
    focusGoal = storedGoal(activeStudyId);
    const [id, windows, browser] = active[3] ?? [0, false, false];
    const status = $("capture-status");
    status.replaceChildren();
    for (const [source, label, enabled] of [["windows", "Окна", windows], ["browser", "Вкладки", browser]]) {
      const row = element("p", label + (enabled ? " · запись разрешена " : " · запись выключена"));
      if (enabled) row.append(button("Отключить " + label.toLowerCase(), async () => {
        const next = await run(studyControls, studyMessage, async () => {
          await invoke("revoke_capture", { id, source });
          const active = await read("study_active");
          showStudy(active);
          return active ? status.querySelector("button") ?? $("stop-study") : focusForm.elements.mode[0];
        });
        if (!$("study").hidden) next?.focus();
      }));
      status.append(row);
    }
    updateTimer();
  }
}

async function loadToday() {
  try {
    $("today").textContent = await invoke("status");
  } catch (error) {
    $("today").textContent = String(error);
  }
}

function loadStudy() {
  return run(studyControls, studyMessage, async () => {
    await Promise.all([
      (async () => {
        $("reload-study").hidden = false;
        if (!modes.length) renderModes(await read("focus_catalog"));
        showStudy(await read("study_active"));
        await Promise.all([loadHistory(), loadAtlas(), loadBriefing()]);
        $("reload-study").hidden = true;
        if (!awakeChecked) {
          awakeChecked = true;
          if (!seenToday()) showAwake();
        }
      })(),
      loadToday(),
    ]);
  });
}

$("reload-study").addEventListener("click", loadStudy);
$("start-study").addEventListener("submit", (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  const topic = form.elements.topic.value;
  run(studyControls, studyMessage, async () => {
    const mode = form.elements.mode.value;
    const direction = form.elements.direction.value;
    const windows = $("capture-window").checked;
    const browser = $("capture-browser").checked;
    body.classList.add("entering");
    field.setScene("enter");
    try {
      await pause(850);
      const [started, name, tracking, capture, timerError] = await read("study_start", { topic, mode, direction, windows, browser, estimate: Number($("focus-estimate").value) || 0, mute: $("mute-notifications").checked });
      storeGoal(capture[0], Number($("focus-goal").value));
      showStudy([started, name, [mode, direction], capture]);
      $("capture-window").checked = false;
      $("capture-browser").checked = false;
      $("focus-estimate").value = "";
      $("commit").checked = false;
      form.elements.topic.value = "";
      studyMessage.textContent = [!tracking && "Сессия началась, но трекер не запустился.", timerError]
        .filter(Boolean).join(" ");
      // Сетап раскладывает окна в фоне: таймер уже идёт, ошибки конфига показываем в статусе.
      if ($("apply-setup").checked) {
        invoke("setup_apply", { mode, direction })
          .then(note => { if (note) studyMessage.textContent = note; })
          .catch(error => { studyMessage.textContent = String(error); });
      }
    } finally {
      body.classList.remove("entering");
      syncScene();
    }
  });
});

// Разбор сессии: Claude составляет вопросы по журналу, они попадают в «Повторения».
// Сначала показываем точный запрос — в нём заголовки окон и выдержки страниц.
let summarySession = null;

function reviewReset(message = "") {
  $("review-preview").replaceChildren();
  $("review-status").textContent = message;
  $("review-session").hidden = false;
}

$("review-session").addEventListener("click", async () => {
  const start = $("review-session");
  start.disabled = true;
  $("review-status").textContent = "Готовлю запрос…";
  try {
    const files = await Promise.all([...$("review-files").files].map(async (file) => [file.name, await file.text()]));
    const preview = await read("review_prepare", { sessionId: summarySession, text: $("review-text").value, files });
    const json = element("pre", preview.requestJson);
    json.className = "tutor-json";
    json.tabIndex = 0;
    json.setAttribute("aria-label", "Точный запрос к Claude");
    const send = button("Отправить в Claude", async () => {
      send.disabled = cancel.disabled = true;
      $("review-status").textContent = "Claude составляет вопросы…";
      try {
        const result = await read("review_send", { previewId: preview.previewId });
        reviewReset(result.count
          ? `${result.summary} Вопросов на просмотр: ${result.count}. Проверь ключи и добавь нужные в повторения.`
          : `${result.summary} Проверяемых вопросов не получилось.`);
        start.hidden = result.count > 0;
        await renderDrafts($("review-drafts"), result.sessionId);
        $("review-drafts").querySelector("button")?.focus();
      } catch (error) {
        reviewReset(error instanceof Error ? error.message : String(error));
      }
    });
    send.className = "primary";
    const cancel = button("Отмена", () => {
      invoke("review_cancel").catch(() => {});
      reviewReset("Запрос отменён, ничего не отправлено.");
    });
    const actions = element("div");
    actions.className = "tutor-actions";
    actions.append(send, cancel);
    const omitted = preview.omitted ? ` · пропущено фрагментов: ${preview.omitted}` : "";
    $("review-preview").replaceChildren(element("p", `Запрос к Claude · около ${preview.estimatedTokens} токенов${omitted}`), json, actions);
    $("review-status").textContent = "";
    start.hidden = true;
    json.focus();
  } catch (error) {
    $("review-status").textContent = error instanceof Error ? error.message : String(error);
  } finally {
    start.disabled = selfTest;
  }
});

// Вопросы Claude сначала на просмотре: ключ, объяснение и цитата видны до добавления в повторения.
const DRAFT_STATUS = { pending: "ждёт решения", accepted: "в повторениях", rejected: "отклонён", disputed: "исключён из оценки" };

async function renderDrafts(container, sessionId) {
  const drafts = await read("review_drafts", { sessionId });
  const refresh = async () => {
    await renderDrafts(container, sessionId);
    (container.querySelector("button") ?? container.querySelector("h3"))?.focus();
  };
  const heading = element("h3", "Вопросы Claude на просмотре");
  heading.tabIndex = -1;
  container.replaceChildren(...(drafts.length ? [heading] : []), ...drafts.map(d => draftCard(d, refresh)));
  return drafts;
}

function draftCard(d, refresh) {
  const card = element("article");
  card.className = "draft";
  const key = element("p", "Ключ: " + d.answers.join(" | "));
  const source = d.source ? element("blockquote", d.source) : element("p", "Источник: " + d.sourceLabel);
  card.append(element("strong", d.prompt), key, element("p", d.explain), source, element("small", DRAFT_STATUS[d.status] ?? d.status));
  if (d.status !== "pending") return card;
  const message = element("p");
  message.setAttribute("role", "status");
  const act = (command) => async () => {
    try {
      await invoke(command, { id: d.id });
      await refresh();
    } catch (error) {
      message.textContent = errorText(error);
    }
  };
  const actions = element("div");
  actions.className = "tutor-actions";
  actions.append(button("Добавить в повторения", act("draft_accept")),
    button("Исправить", () => card.replaceWith(draftEditor(d, refresh))),
    button("Отклонить", act("draft_reject")));
  card.append(actions, message);
  return card;
}

function draftEditor(d, refresh) {
  const form = element("form");
  form.className = "draft";
  const field = (label, node, value) => {
    node.value = value;
    const wrap = element("label", label + " ");
    wrap.append(node);
    return wrap;
  };
  const prompt = element("input"), kind = element("select"), answers = element("input"), explain = element("textarea");
  for (const [value, title] of [["text", "текст"], ["number", "число"]]) {
    const option = element("option", title);
    option.value = value;
    kind.append(option);
  }
  const message = element("p");
  message.setAttribute("role", "status");
  const actions = element("div");
  actions.className = "tutor-actions";
  actions.append(element("button", "Сохранить"), button("Отмена", refresh));
  form.append(field("Вопрос", prompt, d.prompt), field("Тип ответа", kind, d.kind),
    field("Ключ (варианты через |)", answers, d.answers.join(" | ")), field("Объяснение", explain, d.explain), actions, message);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    try {
      await invoke("draft_edit", { id: d.id, edit: { prompt: prompt.value, kind: kind.value, answers: answers.value.split("|"), hint: d.hint, explain: explain.value } });
      await refresh();
    } catch (error) {
      message.textContent = errorText(error);
    }
  });
  setTimeout(() => answers.focus());
  return form;
}

// Редактор сетапа: ряды «рабочий стол → приложение» вместо ручной правки TOML.
const SETUP_APPS = [
  ["terminal", "Терминал"],
  ["tmux", "Терминал с tmux", "Имя сессии tmux"],
  ["browser", "Браузер — пустое окно"],
  ["tabs", "Браузер со вкладками", "Адреса через пробел"],
  ["notion", "Notion"],
  ["webapp", "Веб-приложение", "Адрес https://…"],
  ["command", "Своя команда", "Команда, например obsidian"],
];

function setupRow(win = { workspace: $("setup-rows").children.length + 1, app: "browser" }) {
  const row = element("li");
  const workspace = element("select");
  for (let n = 1; n <= 10; n++) workspace.append(new Option(`Стол ${n}`, n));
  workspace.value = String(win.workspace);
  workspace.setAttribute("aria-label", "Рабочий стол");
  const app = element("select");
  for (const [id, title] of SETUP_APPS) app.append(new Option(title, id));
  app.value = win.app === "browser" && win.urls?.length ? "tabs" : win.app;
  app.setAttribute("aria-label", "Приложение");
  const extra = element("input");
  extra.value = win.session || win.command || (win.urls || []).join(" ");
  const remove = element("button", "Убрать");
  remove.type = "button";
  remove.addEventListener("click", () => {
    (row.nextElementSibling || row.previousElementSibling)?.querySelector("select")?.focus();
    row.remove();
    if (!$("setup-rows").children.length) $("setup-add").focus();
  });
  const sync = () => {
    const hint = SETUP_APPS.find(([id]) => id === app.value)[2];
    extra.classList.toggle("off", !hint);
    extra.disabled = !hint;
    extra.placeholder = hint || "";
    extra.setAttribute("aria-label", hint || "");
  };
  app.addEventListener("change", sync);
  sync();
  row.append(workspace, app, extra, remove);
  $("setup-rows").append(row);
  return row;
}

function setupValue() {
  const window = [...$("setup-rows").children].map(row => {
    const [workspace, app, extra] = row.querySelectorAll("select, input");
    const text = extra.value.trim();
    const win = { workspace: Number(workspace.value), app: app.value === "tabs" ? "browser" : app.value };
    if (app.value === "tmux" && text) win.session = text;
    if (app.value === "command") win.command = text;
    if (app.value === "tabs" || app.value === "webapp") win.urls = text.split(/\s+/).filter(Boolean);
    return win;
  });
  return { close_others: $("setup-close-others").checked, window };
}

function setupTarget() {
  const form = $("start-study");
  return { mode: form.elements.mode.value, direction: form.elements.direction.value };
}

$("edit-setup").addEventListener("click", () => {
  const editor = $("setup-editor");
  if (!editor.hidden) {
    editor.hidden = true;
    $("edit-setup").setAttribute("aria-expanded", "false");
    return;
  }
  run(studyControls, studyMessage, async () => {
    const setup = await read("setup_get", setupTarget());
    $("setup-rows").replaceChildren();
    setup.window.forEach(setupRow);
    $("setup-close-others").checked = setup.close_others;
    editor.hidden = false;
    $("edit-setup").setAttribute("aria-expanded", "true");
    ($("setup-rows").querySelector("select") || $("setup-add")).focus();
  });
});

// Другое направление — другой файл: старые ряды не должны сохраниться в него.
$("start-study").addEventListener("change", event => {
  if (event.target.name !== "mode" && event.target.name !== "direction") return;
  $("setup-editor").hidden = true;
  $("edit-setup").setAttribute("aria-expanded", "false");
});

$("setup-add").addEventListener("click", () => setupRow().querySelectorAll("select")[1].focus());

$("setup-save").addEventListener("click", () => {
  const { direction } = setupTarget();
  run(studyControls, studyMessage, async () => {
    await invoke("setup_save", { ...setupTarget(), setup: setupValue() });
    studyMessage.textContent = `Сетап «${direction}» сохранён.`;
  });
});

$("setup-file").addEventListener("click", () => {
  run(studyControls, studyMessage, async () => {
    const path = await invoke("setup_open", setupTarget());
    studyMessage.textContent = `Сетап открыт в редакторе: ${path}`;
  });
});

$("stop-study").addEventListener("click", () => {
  run(studyControls, studyMessage, async () => {
    const stopped = activeStudyId;
    const log = await invoke("study_stop", { id: activeStudyId });
    summarySession = stopped;
    $("review-drafts").replaceChildren();
    body.classList.remove("setup");
    showStudy(null);
    field.pulse();
    $("focus-log").textContent = log;
    reviewReset();
    $("study-summary").hidden = false;
    await Promise.all([loadToday(), loadHistory(), loadBriefing()]);
  });
});
setInterval(updateTimer, 1000);

let reviewLimit = 0;
$("self-check").addEventListener("click", () => {
  reviewLimit = 5;
  act(["", "reviews"]);
});

const reviewContent = $("review-content");
const reviewMessage = $("review-message");
let reviewing = false;

let weeklyRequested = false;
let weekly = null;
let weeklyLive = false;

async function loadReviews() {
  if (weekly) {
    weekly.focus();
    return;
  }
  if (reviewing) return;
  const input = await run(reviewContent, reviewMessage, async () => {
    selfTest = await read("self_test_active");
    syncHelp();
    if (selfTest || weeklyRequested) {
      weeklyRequested = false;
      return showWeekly();
    }
    const pending = element("div");
    pending.className = "drafts";
    const drafts = await renderDrafts(pending, null);
    const queue = (await read("review_queue")).slice(0, reviewLimit || undefined);
    reviewLimit = 0;
    const total = queue.length;
    if (!total) {
      reviewContent.replaceChildren(element("p", drafts.length
        ? "Повторений нет. Проверь вопросы Claude: принятые появятся здесь." : "Сегодня нечего повторять"), pending);
      return;
    }
    const prediction = element("input");
    prediction.type = "number";
    prediction.min = 0;
    prediction.max = total;
    prediction.required = true;
    reviewContent.replaceChildren(answerForm(
      `Сколько из ${total} ответишь?`, "Начать", async (value) => {
        const input = await run(reviewContent, reviewMessage, async () => {
          const predicted = Number(value);
          const sessionId = await invoke("session_begin", { planned: total, predicted });
          reviewing = true;
          return showQuestions(queue, sessionId, predicted);
        });
        if (!$("reviews").hidden) input?.focus();
      }, prediction,
    ), pending);
    return prediction;
  });
  if (!$("reviews").hidden) input?.focus();
}

// Недельный блок: по одному вопросу без ключей; каждый вызов test_current — новый показ,
// поэтому вопрос запрашивается один раз и остаётся на экране при переключении вкладок.
async function showWeekly() {
  const q = await read("test_current");
  if (!q) {
    weekly = null;
    selfTest = false;
    syncHelp();
    reviewContent.replaceChildren(element("p", "Недельных проверок сейчас нет."));
    return null;
  }
  selfTest = true;
  syncHelp();
  const opened = performance.now();
  const input = element("input");
  input.required = true;
  input.autocomplete = "off";
  const form = answerForm("Ответ", "Ответить", async (answer) => {
    const next = await run(reviewContent, reviewMessage, async () => {
      const result = await read("test_submit", { block: q.block, itemId: q.item.id, answer, ms: Math.round(performance.now() - opened) });
      weekly = null;
      return result.closed ? showWeeklyReview(result) : showWeekly();
    });
    if (!$("reviews").hidden) next?.focus();
  }, input);
  const resumed = q.resumed && !weeklyLive ? "Продолжаем начатый блок · " : "";
  weeklyLive = true;
  const lock = element("p", "Отвечай сам: тьютор, подсказки и ключи закрыты. Разбор — после последнего ответа блока.");
  lock.className = "notice";
  const question = element("h2", q.item.prompt);
  question.className = "prose";
  reviewContent.replaceChildren(element("p", `${resumed}Недельная проверка · вопрос ${q.total - q.left + 1}/${q.total}`), lock, question);
  if (q.item.code) reviewContent.append(element("pre", q.item.code));
  reviewContent.append(form);
  weekly = input;
  return input;
}

function showWeeklyReview(result) {
  weeklyLive = false;
  selfTest = false;
  syncHelp();
  const list = element("ol");
  for (const row of result.review) {
    const entry = element("li");
    entry.append(element("strong", row.prompt),
      element("p", `${row.correct ? "✓ Верно" : "✗ Неверно"} · твой ответ: ${row.answer || "—"} · ключ: ${row.key}`),
      element("p", row.explain));
    list.append(entry);
  }
  const done = button("Готово", () => {
    reviewContent.replaceChildren();
    loadReviews();
  });
  reviewContent.replaceChildren(element("h2", `Блок закрыт: верно ${result.correct} из ${result.total}`), element("p", "Разбор"), list, done);
  loadBriefing();
  return done;
}

function showQuestions(queue, sessionId, predicted) {
  const total = queue.length;
  let index = 0;
  let correct = 0;
  let helped = 0; // верные с подсказкой: в факт для прогноза не входят, как в ядре

  function showQuestion() {
    const [itemId, role, prompt, code, hint, claude] = queue[index];
    const opened = performance.now();
    let usedHint = false;
    const hintText = element("p");
    hintText.className = "prose";
    hintText.setAttribute("role", "status");
    const input = element("input");
    const form = answerForm("Ответ", "Ответить", async (answer) => {
      const ms = Math.round(performance.now() - opened);
      const next = await run(reviewContent, reviewMessage, async () => {
        const result = await read("answer", { sessionId, itemId, role, answer, hint: usedHint, ms });
        if (result.ok) field.activate("recall");
        if (role !== "retry") {
          if (result.ok && usedHint) helped += 1;
          else if (result.ok) correct += 1;
          else queue.push([itemId, "retry", prompt, code, null, claude]);
        }
        const verdict = result.ok ? "✓ Верно" : `✗ Неверно. Ответ: ${result.answer}`;
        const feedback = element("div");
        feedback.setAttribute("role", "status");
        feedback.append(element("p", verdict), element("p", result.explain));
        feedback.lastElementChild.className = "prose";
        const next = button("Дальше", () => {
          if (index + 1 < queue.length) {
            index += 1;
            showQuestion().focus();
            return;
          }
          run(reviewContent, reviewMessage, async () => {
            await invoke("session_end", { id: sessionId, correct });
            reviewing = false;
            reviewContent.replaceChildren(element("p", `Сам: ${correct}/${total}, прогноз был ${predicted}${helped ? `; с подсказкой ещё ${helped}` : ""}`));
          });
        });
        if (claude) feedback.append(button("Сообщить об ошибке", (event) => {
          const note = element("input");
          note.placeholder = "Что не так с вопросом или ключом";
          const report = answerForm("Ошибка", "Отправить", async (text) => {
            try {
              await invoke("item_dispute", { itemId, note: text });
              for (let i = queue.length - 1; i > index; i -= 1) if (queue[i][0] === itemId) queue.splice(i, 1);
              report.replaceWith(element("p", "Вопрос убран из повторений и оценки, попытки сохранены."));
              next.focus();
            } catch (error) {
              reviewMessage.textContent = errorText(error);
            }
          }, note);
          event.currentTarget.replaceWith(report);
          note.focus();
        }));
        form.replaceWith(feedback, next);
        hintButton?.remove();
        return next;
      });
      if (!$("reviews").hidden) next?.focus();
    }, input);
    const hintButton = role !== "retry" && hint ? button("Подсказка", () => {
      usedHint = true;
      hintText.textContent = hint;
      hintButton.hidden = true;
      input.focus();
    }) : null;
    const progress = role === "retry"
      ? `Работа над ошибками · ${index - total + 1}/${queue.length - total}`
      : `Вопрос ${index + 1}/${total}`;
    const question = element("h2", prompt);
    question.className = "prose";
    reviewContent.replaceChildren(element("p", progress), question);
    if (code) reviewContent.append(element("pre", code));
    if (hintButton) reviewContent.append(hintButton, hintText);
    reviewContent.append(form);
    return input;
  }

  return showQuestion();
}

const practiceControls = $("practice-controls");
const practiceMessage = $("practice-message");
const challengeList = $("challenge-list");
const detail = $("challenge-detail");

function loadPractice() {
  run(practiceControls, practiceMessage, async () => {
    const rows = await read("challenges");
    challengeList.replaceChildren();
    detail.replaceChildren();
    if (!rows.length) {
      practiceMessage.textContent = "Задач пока нет.";
      return;
    }
    for (const row of rows) {
      const entry = element("li");
      const [, title, kind, topic, difficulty, status] = row;
      const meta = (state) => `${kind} · ${topic} · сложность ${difficulty}/5 · ${state}`;
      const info = element("small", meta(status));
      const select = button(title, () => {
        challengeList.querySelector('[aria-expanded="true"]')?.setAttribute("aria-expanded", "false");
        select.setAttribute("aria-expanded", "true");
        showChallenge(row, (state) => {
          row[5] = state;
          info.textContent = meta(state);
        });
      });
      select.className = "challenge";
      select.setAttribute("aria-expanded", "false");
      select.setAttribute("aria-controls", "challenge-detail");
      select.append(info);
      entry.append(select);
      challengeList.append(entry);
    }
  });
}

function showChallenge([id, title, kind, , , status, statement], setStatus) {
  const pwn = kind === "pwn";
  practiceMessage.textContent = "";
  const condition = element("p", statement);
  condition.className = "prose";
  const hint = element("p");
  hint.className = "prose";
  hint.setAttribute("role", "status");
  const result = element("p");
  result.setAttribute("role", "status");
  const input = element("input");
  input.required = true;
  const form = answerForm(pwn ? "Флаг" : "Ответ", "Сдать", (answer) => {
    run(practiceControls, result, async () => {
      if (!answer.trim()) throw new Error("Введи ответ.");
      result.textContent = "Проверка…";
      const ok = await read("solve", { id, answer });
      if (ok) {
        status = "решён";
        field.activate("complete");
      }
      else if (status === "новый") status = "в работе";
      setStatus(status);
      result.textContent = ok ? "✓ Решено" : "✗ Неверно";
    });
  }, input);
  const hintButton = button("Подсказка (решение будет отмечено как с помощью)", () => {
    run(practiceControls, hint, async () => {
      hint.textContent = await invoke("next_hint", { id });
    });
  });
  hintButton.disabled = selfTest;
  if (selfTest) hint.textContent = "Подсказки закрыты до конца недельной проверки.";
  detail.replaceChildren(element("h2", title), condition, hintButton, hint, form, result);

  if (!pwn) return;
  const files = element("div");
  detail.append(element("p", "Бинарь написан ИИ — запускай только через песочницу"),
    button("Файлы для анализа", () => {
      run(practiceControls, practiceMessage, async () => {
        const path = await invoke("export_files", { id });
        const commands = [
          `whiteroom pwn run ${id}`,
          `whiteroom pwn gdb ${id}`,
          `process(['whiteroom', 'pwn', 'run', '${id}'])`,
        ];
        files.replaceChildren(element("p", path));
        for (const command of commands) {
          const line = element("div");
          const code = element("code", command);
          const copy = button("Копировать", async () => {
            try {
              await navigator.clipboard.writeText(command);
              practiceMessage.textContent = "Скопировано.";
            } catch {
              const range = document.createRange();
              range.selectNodeContents(code);
              const selection = window.getSelection();
              selection.removeAllRanges();
              selection.addRange(range);
              practiceMessage.textContent = "Команда выделена для копирования.";
            }
          });
          line.className = "command";
          line.append(code, copy);
          files.append(line);
        }
      });
    }), files);
}

const siteControls = $("site-controls");
const sitesMessage = $("sites-message");
async function renderSites() {
  const domains = await read("blocked_sites");
  $("site-list").replaceChildren(...domains.map((domain) => {
    const row = element("li");
    row.append(element("span", domain));
    const remove = button("Убрать", () => {
      run(siteControls, sitesMessage, async () => {
        await invoke("site_unblock", { domain });
        await renderSites();
      });
    });
    remove.setAttribute("aria-label", "Убрать " + domain);
    row.append(remove);
    return row;
  }));
}

$("edit-topic-sites").addEventListener("click", () => {
  document.querySelector('[aria-controls="sites"]').click();
});

$("add-site").addEventListener("submit", (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  run(siteControls, sitesMessage, async () => {
    await invoke("site_block", { domain: form.elements.domain.value });
    form.reset();
    await renderSites();
  });
});

$("pilot-show").addEventListener("click", async () => {
  const target = Math.round((brief?.retention_target ?? 0.9) * 100);
  $("pilot-legend").textContent = `Минуты на календаре — активность, а не учебный результат. Учебные результаты — самостоятельная отложенная проверка и новые сопоставимые задачи. «Оценка» сессии — прогноз времени. Цель расписания повторений — вспомнить ${target}%. Ответы с подсказкой в зачёт не входят.`;
  try {
    $("pilot-text").textContent = await invoke("pilot_text");
  } catch (error) {
    $("pilot-text").textContent = errorText(error);
  }
  $("pilot-text").hidden = false;
});

const tabs = document.querySelectorAll("nav button");
let pageTutor = null;
let pageTutorRevision = 0;
let dialogTutor = null;
let dialogLoading = false;

function makeTutor(ctx, compact, reload) {
  const active = ctx.sessionId !== null;
  const t = { ctx, reload, preview: null, busy: false, closed: false };
  const title = (id) => ctx.concepts.find(concept => concept.id === id)?.title ?? "";

  t.box = element("div");
  t.box.className = compact ? "tutor tutor-compact" : "tutor chat-panel";
  if (!compact) {
    const head = element("header");
    head.className = "chat-heading";
    head.append(element("h2", "Тьютор"), element("span", active ? "Теория в сессии" : "История за сегодня"));
    t.box.append(head);
  }

  t.log = element("div");
  t.log.className = "chat-log";
  t.log.setAttribute("role", "log");
  t.log.setAttribute("aria-label", "Переписка за сегодня");
  t.log.tabIndex = 0;
  t.messages = element("ol");
  t.messages.className = "chat-messages";
  t.log.append(t.messages);
  t.empty = element("li", ctx.suggestedConceptId
    ? "Можешь начать с понятия «" + title(ctx.suggestedConceptId) + "»."
    : "Напиши вопрос или выбери понятие из каталога.");
  t.empty.className = "chat-hint";
  renderTutorHistory(t, ctx.messages ?? []);

  t.out = element("div");
  t.out.className = "tutor-preview";
  t.status = element("p");
  t.status.className = "tutor-status notice";
  t.status.setAttribute("role", "status");

  const form = element("form");
  form.className = "chat-form";
  t.controls = element("fieldset");
  t.controls.className = "chat-controls";
  const actions = element("div");
  actions.className = "tutor-actions";
  t.concept = element("select");
  t.concept.setAttribute("aria-label", "Понятие из каталога");
  t.concept.append(new Option("Понятие из каталога", ""), ...ctx.concepts.map(item => new Option(item.title, item.id)));
  t.concept.value = ctx.suggestedConceptId ?? "";
  // Каталог отвечает локально; Claude — свободный ответ через API после подтверждения запроса.
  t.engine = "catalog";
  try { if (localStorage.getItem("tutor-engine") === "claude") t.engine = "claude"; } catch {}
  const engines = element("div");
  engines.className = "tutor-engine";
  engines.setAttribute("role", "group");
  engines.setAttribute("aria-label", "Кто отвечает");
  t.engines = [["catalog", "Каталог"], ["claude", "Claude"]].map(([id, label]) => {
    const choice = button(label, () => {
      t.engine = id;
      try { localStorage.setItem("tutor-engine", id); } catch {}
      edit();
    });
    choice.dataset.engine = id;
    return choice;
  });
  engines.append(...t.engines);
  actions.append(engines, t.concept);

  t.input = element("textarea");
  t.input.rows = 2;
  t.input.setAttribute("aria-label", "Сообщение тьютору");
  t.input.placeholder = "Например, что такое хеш?";
  t.submit = element("button", "Спросить");
  t.submit.className = "primary";
  t.submit.type = "submit";
  const composer = element("div");
  composer.className = "chat-compose";
  composer.append(t.input, t.submit);

  if (active) {
    const definition = button("Определение", () => tutorPrepare(t, null, t.concept.value));
    const example = button("Пример", () => {
      tutorPrepare(t, "Пример: " + title(t.concept.value), t.concept.value);
    });
    t.quick = [definition, example];
    actions.append(...t.quick);
  }

  function edit() {
    if (t.preview) tutorCancel(t);
    t.status.replaceChildren();
    updateTutorComposer(t);
  }
  t.concept.addEventListener("change", edit);
  t.input.addEventListener("input", edit);
  t.input.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      form.requestSubmit();
    }
  });
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (t.preview) return;
    const question = t.input.value.trim() || null;
    if (t.engine === "claude") tutorPrepare(t, question, null, true);
    else tutorPrepare(t, question, question ? null : t.concept.value);
  });

  t.controls.append(actions, composer);
  form.append(t.controls);
  const footer = element("div");
  footer.className = "chat-footer";
  footer.append(t.out, t.status, form);
  if (!active) {
    const info = element("p", "Каталог отвечает без сети. Claude отвечает свободно: перед отправкой покажу точный запрос с сегодняшней перепиской.");
    info.className = "chat-info";
    footer.append(info);
  }
  t.box.append(t.log, footer);
  updateTutorComposer(t);
  return t;
}

function updateTutorComposer(t) {
  const hasQuestion = Boolean(t.input.value.trim());
  const claude = t.engine === "claude";
  for (const choice of t.engines) choice.setAttribute("aria-pressed", String(choice.dataset.engine === t.engine));
  t.concept.hidden = claude;
  for (const action of t.quick ?? []) action.hidden = claude;
  t.input.placeholder = claude ? "Спроси Claude о чём угодно по теории" : "Например, что такое хеш?";
  t.submit.disabled = t.busy || Boolean(t.preview) || (!hasQuestion && (claude || !t.concept.value));
  t.submit.textContent = claude ? "Спросить Claude" : hasQuestion || !t.concept.value ? "Спросить" : "Объяснить";
  for (const action of t.quick ?? []) action.disabled = t.busy || !t.concept.value;
}

function tutorSay(t, role, text) {
  t.empty.remove();
  const user = role === "user";
  const item = element("li");
  item.className = user ? "msg msg-user" : role === "claude" ? "msg msg-tutor msg-claude" : "msg msg-tutor";
  const author = element("span", user ? "Ты" : role === "claude" ? "Claude" : "Тьютор");
  author.className = "msg-author";
  item.append(author, element("p", text));
  t.messages.append(item);
  tutorScroll(t);
  return item;
}

function tutorScroll(t) {
  t.log.scrollTop = t.log.scrollHeight;
}

function renderTutorHistory(t, messages) {
  t.messages.replaceChildren(t.empty);
  for (const message of messages) tutorSay(t, message.role, message.text);
}

function tutorCancel(t) {
  const draft = t.preview;
  if (!draft) return;
  t.preview = null;
  draft.entry.remove();
  if (!t.messages.childElementCount) t.messages.append(t.empty);
  t.out.replaceChildren();
  invoke("tutor_cancel", { previewId: draft.previewId }).catch(() => {});
  updateTutorComposer(t);
}

function tutorBusy(t, busy) {
  t.busy = busy;
  t.controls.disabled = busy;
  updateTutorComposer(t);
}

function tutorError(t, error, question, conceptId, claude) {
  const message = error instanceof Error ? error.message : String(error);
  t.status.replaceChildren(message);
  if (message.includes("Открой тьютора заново")) {
    t.status.append(" ", button("Обновить", t.reload));
  } else if (message.includes("заново")) {
    t.status.append(" ", button("Подготовить заново", () => tutorPrepare(t, question, conceptId, claude)));
  }
}

async function tutorPrepare(t, question, conceptId, claude = false) {
  if (t.busy || t.closed || (!question && !conceptId)) return;
  tutorCancel(t);
  tutorBusy(t, true);
  t.status.textContent = "Готовлю ответ…";
  let nextFocus = t.input;
  try {
    const preview = await read("tutor_prepare", { sessionId: t.ctx.sessionId, question, conceptId: conceptId || null, claude });
    if (t.closed || preview.sessionId !== t.ctx.sessionId) {
      invoke("tutor_cancel", { previewId: preview.previewId }).catch(() => {});
      return;
    }
    const concept = t.ctx.concepts.find(item => item.id === conceptId);
    const entry = tutorSay(t, "user", question || concept?.title || "Определение");
    t.preview = { ...preview, question, conceptId, claude, entry };
    if (preview.local) {
      await tutorSend(t);
      return;
    }
    t.status.replaceChildren();
    const json = element("pre", preview.requestJson);
    json.className = "tutor-json";
    json.tabIndex = 0;
    json.setAttribute("aria-label", "Точный запрос к Claude");
    const send = button("Отправить в Claude", async () => {
      tutorBusy(t, true);
      try {
        await tutorSend(t);
      } finally {
        tutorBusy(t, false);
        if (!t.closed) t.input.focus({ preventScroll: true });
      }
    });
    send.className = "primary";
    const cancel = button("Отмена", () => {
      tutorCancel(t);
      t.status.textContent = "Запрос отменён.";
      t.input.focus({ preventScroll: true });
    });
    cancel.className = "js-cancel";
    const actions = element("div");
    actions.className = "tutor-actions";
    actions.append(send, cancel);
    const label = element("p", "Запрос к Claude · около " + preview.estimatedTokens + " токенов");
    t.out.replaceChildren(label, json, actions);
    actions.scrollIntoView({ block: "nearest" });
    nextFocus = json;
  } catch (error) {
    if (!t.closed) tutorError(t, error, question, conceptId, claude);
  } finally {
    tutorBusy(t, false);
    if (!t.closed) nextFocus.focus({ preventScroll: true });
  }
}

async function tutorSend(t) {
  const draft = t.preview;
  if (t.closed || !draft) return;
  t.preview = null;
  t.out.replaceChildren();
  t.status.textContent = "Жду ответ…";
  try {
    const result = await read("tutor_send", { previewId: draft.previewId });
    if (t.closed) return;
    if (result.sessionId !== t.ctx.sessionId) {
      throw new Error("Состояние сессии изменилось. Открой тьютора заново.");
    }
    tutorSay(t, draft.claude ? "claude" : "tutor", result.answer);
    t.status.replaceChildren();
    t.input.value = "";
  } catch (error) {
    if (!t.closed) {
      draft.entry.remove();
      if (!t.messages.childElementCount) t.messages.append(t.empty);
      tutorError(t, error, draft.question, draft.conceptId, draft.claude);
    }
  }
}

function dropTutor(t) {
  if (!t) return;
  t.closed = true;
  tutorCancel(t);
}

async function loadTutorPage() {
  dropTutor(pageTutor);
  pageTutor = null;
  const revision = ++pageTutorRevision;
  const page = $("tutor-page");
  const loading = element("p", "Загружаю переписку…");
  loading.setAttribute("role", "status");
  page.replaceChildren(loading);
  try {
    const ctx = await read("tutor_context");
    if (revision !== pageTutorRevision || $("tutor").hidden) return;
    pageTutor = makeTutor(ctx, false, loadTutorPage);
    page.replaceChildren(pageTutor.box);
    tutorScroll(pageTutor);
  } catch (error) {
    if (revision !== pageTutorRevision || $("tutor").hidden) return;
    loading.textContent = error instanceof Error ? error.message : String(error);
    loading.after(button("Повторить", loadTutorPage));
  }
}

async function openTutorDialog() {
  if (dialogTutor || dialogLoading) return;
  dialogLoading = true;
  try {
    const ctx = await read("tutor_context");
    if (ctx.sessionId !== activeStudyId || $("study").hidden || $("active-study").hidden) return;
    dialogTutor = makeTutor(ctx, true, () => {
      closeTutorDialog(false);
      openTutorDialog();
    });
    $("tutor-dialog-body").replaceChildren(dialogTutor.box);
    $("tutor-dialog").hidden = false;
    document.querySelector("main").inert = true;
    tutorScroll(dialogTutor);
    dialogTutor.input.focus();
  } catch (error) {
    studyMessage.textContent = error instanceof Error ? error.message : String(error);
  } finally {
    dialogLoading = false;
  }
}

function closeTutorDialog(returnFocus = true) {
  if (!dialogTutor) return;
  dropTutor(dialogTutor);
  dialogTutor = null;
  $("tutor-dialog").hidden = true;
  $("tutor-dialog-body").replaceChildren();
  document.querySelector("main").inert = false;
  if (returnFocus && !$("active-study").hidden) $("open-tutor").focus();
}

$("open-tutor").addEventListener("click", openTutorDialog);
$("close-tutor").addEventListener("click", () => closeTutorDialog());

window.addEventListener("focus", async () => {
  const t = dialogTutor ?? pageTutor;
  if (!t || t.closed || t.busy || t.preview) return;
  const last = t.messages.lastElementChild;
  try {
    const ctx = await read("tutor_context");
    if (t.closed || t.busy || t.preview || t.messages.lastElementChild !== last) return;
    if (ctx.sessionId !== t.ctx.sessionId) {
      t.reload();
      return;
    }
    const visible = [...t.messages.querySelectorAll(".msg > p")];
    const messages = ctx.messages ?? [];
    if (messages.length === visible.length && messages.every((m, i) => m.text === visible[i].textContent)) return;
    renderTutorHistory(t, messages);
  } catch {
    // Переписка остаётся доступной до следующей синхронизации.
  }
});

// ───── Дашборд: сетка дней текущего месяца, яркость клетки — минуты фокуса. ─────
const dashMonths = ["Январь", "Февраль", "Март", "Апрель", "Май", "Июнь", "Июль", "Август", "Сентябрь", "Октябрь", "Ноябрь", "Декабрь"];
const dashLevel = (minutes, answers) => minutes >= 90 ? 4 : minutes >= 45 ? 3 : minutes >= 15 ? 2 : minutes || answers ? 1 : 0;
const dashHours = (minutes) => minutes < 60 ? `${minutes} мин`
  : minutes % 60 ? `${Math.floor(minutes / 60)} ч ${minutes % 60} мин` : `${minutes / 60} ч`;
const dashOf = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];

function dashCount(node, value, format) {
  if (reducedMotion() || motion === "off") {
    node.textContent = format(value);
    return;
  }
  const started = performance.now();
  const step = (time) => {
    const t = Math.min(1, (time - started) / 900);
    node.textContent = format(Math.round(value * (1 - (1 - t) ** 3)));
    if (t < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

async function loadDashboard() {
  await run($("dashboard"), $("dash-message"), async () => {
    const days = await read("month_activity");
    const date = (text) => {
      const [y, m, d] = text.split("-").map(Number);
      return new Date(y, m - 1, d);
    };
    const first = date(days[0][0]);
    const now = new Date();
    const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
    $("dash-month").textContent = `${dashMonths[first.getMonth()]} ${first.getFullYear()}`;

    const grid = $("dash-grid");
    const detail = $("dash-detail");
    const describe = ([day, minutes, sessions, answers]) => {
      const d = date(day);
      const label = `${d.getDate()} ${dashOf[d.getMonth()]}`;
      if (day > today) return `${label}: впереди`;
      if (!minutes && !sessions && !answers) return `${label}: без активности`;
      return `${label}: ${dashHours(minutes)} · ${sessions} ${plural(sessions, "сессия", "сессии", "сессий")} · ${answers} ${plural(answers, "ответ", "ответа", "ответов")}`;
    };
    const cells = [];
    for (let i = 0; i < (first.getDay() + 6) % 7; i++) {
      const gap = element("li");
      gap.className = "dash-gap";
      cells.push(gap);
    }
    days.forEach((entry, index) => {
      const [day, minutes, , answers] = entry;
      const cell = element("li");
      cell.className = "dash-cell";
      cell.dataset.level = dashLevel(minutes, answers);
      cell.style.setProperty("--i", index);
      cell.tabIndex = 0;
      cell.setAttribute("aria-label", describe(entry));
      cell.append(element("span", String(date(day).getDate())));
      if (day === today) cell.classList.add("today");
      if (day > today) cell.classList.add("future");
      const show = () => { detail.textContent = describe(entry); };
      cell.addEventListener("mouseenter", show);
      cell.addEventListener("focus", show);
      cells.push(cell);
    });
    grid.replaceChildren(...cells);
    const current = days.find(entry => entry[0] === today);
    detail.textContent = current ? describe(current) : "";

    const past = days.filter(entry => entry[0] <= today);
    const active = past.filter(([, minutes, , answers]) => minutes || answers);
    let streak = 0;
    for (let i = past.length - 1; i >= 0; i--) {
      const [day, minutes, , answers] = past[i];
      if (minutes || answers) streak++;
      else if (day !== today) break;
    }
    const total = past.reduce((sum, entry) => sum + entry[1], 0);
    const sessions = past.reduce((sum, entry) => sum + entry[2], 0);
    const stats = [
      ["Фокус", total, (n) => n < 60 ? `${n} мин` : `${(n / 60).toFixed(1).replace(".", ",")} ч`],
      ["Сессии", sessions, String],
      ["Активных дней", active.length, (n) => `${n} из ${past.length}`],
      ["Серия", streak, (n) => `${n} ${plural(n, "день", "дня", "дней")}`],
    ];
    $("dash-stats").replaceChildren(...stats.map(([name, value, format], index) => {
      const box = element("div");
      box.style.setProperty("--i", index);
      const number = element("dd");
      box.append(element("dt", name), number);
      dashCount(number, value, format);
      return box;
    }));
  });
}

const load = {
  dashboard: loadDashboard,
  study: loadStudy,
  reviews: loadReviews,
  practice: loadPractice,
  sites: () => run(siteControls, sitesMessage, renderSites),
  tutor: loadTutorPage,
};

for (const tab of tabs) {
  tab.addEventListener("click", () => {
    if (tab.getAttribute("aria-pressed") === "true") return;
    for (const other of tabs) {
      const selected = tab === other;
      other.setAttribute("aria-pressed", String(selected));
      $(other.getAttribute("aria-controls")).hidden = !selected;
    }
    syncScene();
    if (tab.getAttribute("aria-controls") !== "tutor") {
      pageTutorRevision += 1;
      dropTutor(pageTutor);
      pageTutor = null;
    }
    load[tab.getAttribute("aria-controls")]();
  });
}
loadStudy();
