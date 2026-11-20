# Claude Code — интерфейс WHITE ROOM 1.0

Часть задания [prompt-claude-full-app.md](prompt-claude-full-app.md). Ядро и команды Tauri готовы
(`whiteroom-core/src/learn.rs`, `whiteroom-app/src/main.rs`, раздел «WHITE ROOM 1.0»). Сделай окно.

## Владение файлами

Только `whiteroom-app/ui/` (index.html, app.js, style.css, theme.css; новый файл — если упрощает) и
стенд проверки в scratchpad. Rust, `build.rs`, `capabilities/` не трогать: если команды не хватает —
опиши её в отчёте. Тексты — по-русски. Отступ 2 пробела, camelCase. Существующие разделы и id не ломать.

## Команды (все `invoke`, ответы-строки — JSON, если не сказано иначе)

| Команда | Аргументы | Ответ |
| --- | --- | --- |
| `today` | `offset` (сек., `-new Date().getTimezoneOffset()*60`) | `{actions:[{kind: block\|test\|reviews\|goal, title, reason, due, block?, goal?, count?, spread?}], upcoming:[{title, at}], gap_days, gap_note, empty, goals}` |
| `goals` | — | `[{id, title, module, order, skills:[{id,title,practice,seen,diag,prepared,needs,horizon}], materials:[{label,src,scope}], deadline, block_min, left, next_skill, archived}]` |
| `goal_example` | — | id новой цели (пример «Анализ журналов», офлайн) |
| `goal_from_module` | `id` (модуль) | id цели |
| `goal_update` | `id`, `edit:{title, skills:[id…], materials, deadline (unix сек или null), block_min}` | — ; ошибка текстом |
| `goal_archive` | `id` | — |
| `prereq_set` | `skill`, `needs:[id]` | — ; цикл — ошибка |
| `plan_preview` | `goal` | `[{kind, item, min, why}]` |
| `block_start` | `goal`, `predicted` (число или null) | id блока (если блок уже открыт — его id) |
| `block_open` | — | id или null |
| `block_state` | `id` | `{block, goal, title, closed, step, steps:[{kind,item,min,why,status: answered\|skipped\|done\|current\|todo}], item:{id,kind,prompt,code,stage,source,domain,grading: auto\|self,has_hint,hinted}\|null, note, thoughts:[{ts,text}], minutes_left}` |
| `block_plan_set` | `id`, `plan:[{kind,item,min,why}]` | пройденные шаги неизменны |
| `block_hint` | `id`, `step` | текст подсказки (ответ потом не самостоятельный) |
| `block_reveal` | `id`, `step` | для `grading=self`: `{answer:[…], rubric, explain}` |
| `block_answer` | `id`, `step`, `answer`, `rating` (`again\|hard\|good` для self, иначе null), `ms` | `{ok, helped, key, explain, rubric, due, retry}`; повтор шага возвращает тот же результат |
| `block_retry` | `id`, `step`, `answer`, `ms` | bool (повтор после разбора, в зачёт не идёт) |
| `block_skip` | `id`, `step` | перерыв пройден/пропущен или задание отложено |
| `block_note`, `block_thought` | `id`, `note` / `text` | заметка «где остановился», буфер мыслей |
| `block_close` | `id`, `energy` (1–5 или null), `reflection` (или null) | итог `{answers:[{prompt,answer,ok,helped,key,explain,due}], independent_correct, helped, skipped, note, thoughts}` |
| `block_summary` | `id` | то же без закрытия |
| `spread_reviews` | `days` (2–30) | число карточек |
| `progress` | — | `{retention:[{horizon,domain,num,den,low,rule,rows}], skills:[{skill,title,domain,num,den,low,rule,rows}], forecast:{fsrs:{n,expected,actual,low,rule}, self:{n,predicted,actual,low,rule}}, queue:{now,day,week}, volume:{…}, room_minutes:{total,week,note}, low_n}` |
| `seen` | — | `{capture:{session,topic,windows,browser,revoke}, stored:{db,bytes,journal,wal,copies,rows}, integrations:{native_host,d1_sync_timer,claude_key_in_env}, notes:[…]}` |
| `backup` | — | путь копии |
| `modules`, `module_get` (`id`) | | список; модуль как объект `{id,title,domain,objective:[{id,title,needs}],item:[…]}` |
| `module_parse` | `text` (TOML) | модуль-объект, в базу не пишет |
| `module_preview` | `module` | `{added, changed:[{id,prompt,from_v,attempts}], unchanged, removed_kept_in_history, pilot_objectives}` |
| `module_save`, `module_undo` | `module` / `id` | — |
| `anki_import` | `tsv`, `id`, `title` | модуль-объект для редактора (не сохранён) |
| `anki_export` | `id` | TSV-текст |
| `versions` | — | `{app, schema, cli, match, message}` |

Поля задания: `id, v, objective, role (practice|diag|test7|test30), kind (number|text|expr|code|card|explain),
prompt, code?, answer:[…], hint?, explain, stage? (worked|faded|solo|explain|transfer), source?, rubric?,
domain? (positive|real для expr), tests? (Python для code)`.

## Экраны

1. **Сегодня** — стартовый раздел навигации. Действия из `today` по порядку, у каждого причина и срок
   (локальная дата), кнопка действия: продолжить блок, открыть проверку (существующий поток недельной
   проверки `test_current`/`test_submit`), повторения (существующий раздел), продолжить цель (→ план).
   Пустая база: «Начать с примера» (`goal_example`) и «Создать цель из модуля». `gap_note` — нейтрально,
   с кнопкой «Распределить на N дней» при `spread`. Список «Дальше» из `upcoming`.
2. **Цель** — редактирование: название, порядок навыков (кнопки вверх/вниз, с клавиатуры), предпосылки,
   материалы (название, ссылка/путь, область), необязательный срок, длительность блока. Ошибки ядра — текстом
   рядом с формой.
3. **План и блок** — `plan_preview` → правка порядка, длительности, удаление шагов и перерывов → необязательный
   прогноз «сколько отвечу верно» → `block_start`, затем `block_plan_set`, если план правили. В блоке: причина
   шага (`why`), задание (код моноширинным блоком; для `expr` — предпросмотр формулы обычным текстом с
   `^`→верхний индекс, без библиотек; для `code` — textarea), подсказка, ответ, для `self` — «Показать эталон»
   и три кнопки самооценки. После ответа — разбор и «Решить ещё раз» (`block_retry`) при ошибке. Перерыв —
   таймер и «Пропустить». Всегда доступны: «Где остановился» (заметка), «Мысль на потом», «Завершить».
   В итоге — ответы, помощь, ошибки, когда вернётся (`due`), необязательные энергия и рефлексия.
   Повторный клик не должен слать второй запрос (блокируй кнопку до ответа). После перезапуска окна
   `block_open` → «Сегодня» предлагает продолжение; возобновление восстанавливает текущий шаг.
4. **Прогресс** — удержание по сроку (7/30) и домену, успех на новых задачах по навыку, прогноз FSRS и
   самопрогноз против факта, очередь, объём данных, время в комнате с подписью «контекст». У каждой доли —
   `num/den`, отметка «мало данных», раскрытие (`<details>`) с правилом и строками.
5. **Материалы** — список модулей, редактор вопросов (prompt, answer через перевод строки, explain, hint,
   kind, role, stage, source, rubric, code, tests, domain), добавление/удаление, импорт TOML и Anki TSV
   (через `<input type=file>` → текст → `module_parse`/`anki_import`), «Проверить изменения»
   (`module_preview` — показать затронутые записи) → «Сохранить», «Отменить последнее сохранение»,
   экспорт TSV, «Создать цель из модуля».
6. **Что WHITE ROOM видит** — из `seen`: запись по источникам и как отозвать, что хранится, копии,
   интеграции, кнопка «Сделать копию» (`backup`). Плюс строка версий из `versions`, при `match=false` —
   `message` как предупреждение.

## Проверка

- `node --check` для каждого js.
- Стенд с заглушкой `window.__TAURI__` (как в прошлых заданиях — найди прежний стенд в истории
  `.agent/progress.md`, или сделай новый в scratchpad): сценарий клавиатурой — пример на пустой базе →
  правка плана → ответ → подсказка → самооценка → перерыв пропущен → заметка → «перезапуск» (перезагрузка
  страницы) → продолжение с того же шага → завершение → итог; прогресс с `low`; материалы: импорт → предпросмотр
  → сохранить → отмена. В headless Chromium на 360×640 и 900×640, без горизонтального переполнения,
  видимый фокус, reduced motion, Esc закрывает диалоги и возвращает фокус. Снимки экранов сохрани в scratchpad.
- Если доступен реальный бинарь (`./target/debug/whiteroom-app` с временной `WHITEROOM_DB`) — пройди
  сценарий и в нём. Рабочую базу владельца не трогать.

Отчёт: что сделано, пути, какие проверки прошли (с цифрами), что не проверено, каких команд не хватило.
