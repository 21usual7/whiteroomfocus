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

function modeTitle(id) {
  return modes.find(mode => mode.id === id)?.title ?? id;
}

function selectMode(mode) {
  focusForm.elements.direction.replaceChildren(...mode.options.map(text => new Option(text, text)));
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
    label.append(input, element("span", mode.title));
    return label;
  }));
  selectMode(modes[0]);
}

async function loadHistory() {
  const rows = await read("focus_history");
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
  $("timer").textContent = [
    Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60,
  ].map((part) => String(part).padStart(2, "0")).join(":");
}

function showStudy(active) {
  started = active?.[0] ?? null;
  activeStudyId = active?.[3]?.[0] ?? null;
  $("start-study").hidden = Boolean(active);
  $("active-study").hidden = !active;
  if (active) {
    $("study-topic").textContent = active[1];
    const selection = active[2];
    $("active-focus").textContent = selection ? [modeTitle(selection[0]), selection[1]].join(" · ") : "Учебная сессия";
    $("study-summary").hidden = true;
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
        await loadHistory();
        $("reload-study").hidden = true;
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
    const [started, name, tracking, capture] = await read("study_start", { topic, mode, direction, windows, browser });
    showStudy([started, name, [mode, direction], capture]);
    $("capture-window").checked = false;
    $("capture-browser").checked = false;
    form.elements.topic.value = "";
    if (!tracking) studyMessage.textContent = "Сессия началась, но трекер не запустился.";
  });
});

$("stop-study").addEventListener("click", () => {
  run(studyControls, studyMessage, async () => {
    const log = await invoke("study_stop", { id: activeStudyId });
    showStudy(null);
    $("focus-log").textContent = log;
    $("study-summary").hidden = false;
    await Promise.all([loadToday(), loadHistory()]);
  });
});
setInterval(updateTimer, 1000);

const reviewContent = $("review-content");
const reviewMessage = $("review-message");
let reviewing = false;

function loadReviews() {
  if (reviewing) return;
  run(reviewContent, reviewMessage, async () => {
    const queue = await read("review_queue");
    const total = queue.length;
    if (!total) {
      reviewContent.replaceChildren(element("p", "Сегодня нечего повторять"));
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
    ));
  });
}

function showQuestions(queue, sessionId, predicted) {
  const total = queue.length;
  let index = 0;
  let correct = 0;

  function showQuestion() {
    const [itemId, role, prompt, code, hint] = queue[index];
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
        if (role !== "retry") {
          if (result.ok) correct += 1;
          else queue.push([itemId, "retry", prompt, code, null]);
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
            reviewContent.replaceChildren(element("p", `Итог: ${correct}/${total}, прогноз был ${predicted}`));
          });
        });
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
      if (ok) status = "решён";
      else if (status === "новый") status = "в работе";
      setStatus(status);
      result.textContent = ok ? "✓ Решено" : "✗ Неверно";
    });
  }, input);
  detail.replaceChildren(element("h2", title), condition, button("Подсказка", () => {
    run(practiceControls, hint, async () => {
      hint.textContent = await invoke("next_hint", { id });
    });
  }), hint, form, result);

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
  const domains = await read("blocked");
  $("site-list").replaceChildren(...domains.map((domain) => {
    const row = element("li", domain);
    const remove = button("Убрать", () => {
      run(siteControls, sitesMessage, async () => {
        await invoke("block_rm", { domain });
        await renderSites();
      });
    });
    remove.setAttribute("aria-label", `Убрать ${domain}`);
    row.append(remove);
    return row;
  }));
  if (!domains.length) sitesMessage.textContent = "Список пуст.";
}

$("add-site").addEventListener("submit", (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  run(siteControls, sitesMessage, async () => {
    await invoke("block_add", { domain: form.elements.domain.value });
    form.reset();
    await renderSites();
  });
});

const tabs = document.querySelectorAll("nav button");
const load = {
  study: loadStudy,
  reviews: loadReviews,
  practice: loadPractice,
  sites: () => run(siteControls, sitesMessage, renderSites),
};

for (const tab of tabs) {
  tab.addEventListener("click", () => {
    if (tab.getAttribute("aria-pressed") === "true") return;
    for (const other of tabs) {
      const selected = tab === other;
      other.setAttribute("aria-pressed", String(selected));
      $(other.getAttribute("aria-controls")).hidden = !selected;
    }
    load[tab.getAttribute("aria-controls")]();
  });
}
loadStudy();
