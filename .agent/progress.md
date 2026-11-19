# Session record

## 2026-11-19 — research reconciliation

Reviewed existing specifications, independent critique, handoff, implementation paths, tests and focus UI. Preserved the existing prototype. Updated the contributor guide and initialized the missing state files. M1 is proposed for approval with measurable exit criteria.

- A direct synthetic native-message frame on an empty temporary SQLite database returned success with session inactive. Database counts were zero studies and one page. This verifies a storage boundary gap independently of Chromium.
- Source inspection showed that the default context query has no session end bound and practice can append prior titles and retry details after the consent prompt.
- The standard pilot command passed 10/13. Focus duration and page counts were duplicated and the pilot assignment count differed. The test helper reuses a directory named by process ID and test. With a fresh temp root, all 13 passed. Reproduce the isolated baseline with `TMPDIR="$(mktemp -d)" mise exec -- cargo test --locked --offline -p whiteroom-core --test pilot`.
- The full platform/sandbox suite, visual checks and live provider were outside this research check. No application source was changed.
- Git reported no repository. The mounted `.git` directory is empty and read-only. The files are present in the workspace. A commit against original history requires a checkout with its writable metadata.
- Next authorized action after M1 approval is the test-fixture isolation repair. Check D3 and D4 at session start.
- Independent reviewer confirmed the collection, query and preview findings. It also identified premature feedback on interrupted day-seven blocks, missing first-presentation records and overwritten item forms. These are pilot acceptance dependencies in the architecture review and backlog.

Prepared the local supervisor launcher and ignored its runtime logs. Readiness check stopped at the existing approval gate and unavailable Git metadata. The launcher validation command runs the pilot suite in a fresh temporary directory.

## 2026-11-19 — approved M1 implementation

Owner approved D4. Window and browser consent are per-session, separately selectable at start and revocable in the desktop. Session IDs constrain tracking, native writes and default summaries. Existing rows survive migration. Unassigned historical activity remains accessible in explicit time-range queries and exports. An independent reviewer found navigation, upgrade and session-switch races. The final changes validate the document origin before reading the DOM, reject legacy SQL writes at the database boundary, read active controls in one snapshot and bind stop summaries to the stopped study.

Offline workspace build passed. The 16-test pilot suite passed twice in the same parent temp directory. The native-host test exercised the actual CLI subprocess. The recording-transport test verified the preview for initial and retry requests and zero calls on refusal. The browser-worker fixture passed permission, navigation and disconnect cases. No provider calls were made. Reproduce with `./scripts/supervise check`.

The supported desktop remains unavailable (GTK display initialization failed, local Broadway socket creation denied). Sandbox preflight returned a read-only filesystem error. Git metadata is unavailable. Claude Code's desktop assignment is in `docs/prompt-claude-m1.md`. Resume with WebKitGTK keyboard/layout verification and checkpoint in the original checkout.

The owner supplied the repository URL https://github.com/21usual7/whiteroom. A read-only Git lookup could not resolve github.com from this environment. Remote HEAD and ancestry remain unverified.

Reviewed Claude’s first M1 desktop report. Real 900 px session controls worked and the updated theme fit both offscreen widths. Fixed focus restoration on revocation and rebuilt the current UI. JS syntax and supervisor checks passed. Final real-window keyboard and layout verification and Git checkpoint are pending.

SSH lookup of git@github.com:21usual7/whiteroom.git stopped before authentication: OpenSSH rejected a system include owned by nobody in this environment. Host lookup for github.com and ssh.github.com also returned no addresses. Host-side SSH history handoff is documented in docs/prompt-claude-git.md.

Owner corrected the published repository to https://github.com/21usual7/whiteroomfocus. Local Git metadata is now readable at master, 2ef31a0. Remote synchronization was not verified.

## Supervisor orchestration

The owner requested a repository-local supervisor. The initial M1 queue preserves the existing desktop acceptance and owner checkpoint gates. Product implementation and research remain outside this change.

All 16 supervisor tests passed using mocked Codex and temporary Git repositories. The real offline bubblewrap suite passed Cargo check, Clippy, one CLI unit test, 16 pilot tests, JavaScript syntax and the browser consent fixture. Clippy reported four existing style warnings. No live Codex calls were made. The supervisor changes are available in the existing checkout for owner review; the checkout still contains pre-existing uncommitted files. M1 desktop acceptance remains pending.

## 2026-11-19 — M1 closure

The owner accepted the real-window WebKitGTK checks and local Git checkpoint `31b699c` in D6. Keyboard navigation, the focus ring after arrow selection and focus restoration after each revocation passed at 360×640 and 900×640. The current UI was unchanged from the reviewed UI. Both M1 items are in the completed ledger. The supervisor queue is empty while the next milestone awaits owner approval.

All 17 supervisor tests passed. The launcher reported M1 closed, returned code 3 for `run` and `--status`, and made zero Codex calls.

## 2026-11-19 — proposed follow-up milestones

The backlog now records ordered tasks, acceptance evidence and approval boundaries for browser blocking recovery, local data retention and deletion, the delayed assessment pilot and pilot-driven learning workflow improvements. The active supervisor queue awaits owner approval.

## 2026-11-19 — browser blocking recovery approved

The owner approved stage 1 in D7. A synthetic run of the current worker reproduced stale rules after restart, a skipped retry after update failure and retained rules after disconnect. The queue starts with the popup contract and trusted acceptance fixture. Protected rule and messaging changes and the temporary-profile Chromium check have separate owner gates.

The 17 supervisor tests and browser consent fixture passed. The popup fixture rejected the current markup and passed with the required elements in a temporary copy.

## 2026-11-19 — supervisor baseline repair

The first run under D7 stopped before Codex because a clean Git archive lacks Tauri's generated permissions and schemas. The offline validator now mounts only those output directories in temporary storage and leaves sources read-only. The isolated checks passed on a fresh Git archive with both output directories absent. The regression suite passed all 18 supervisor tests.

## 2026-11-19 — редизайн интерфейса (Claude Code)

По запросу владельца переделан вид окна (`whiteroom-app/ui/`): графитовая «лаборатория» на главном экране и белая комната во время сессии. На главном экране карта из реальных данных: шесть режимов фокуса (размер узла — минуты в последних сессиях), орбита задач практики по статусам, ядро с числом карточек к повторению. Клик по узлу выбирает режим. Вход в сессию — белая апертура от кнопки, при reduced-motion без анимации. Добавлена необязательная цель по времени: хранится только в localStorage окна по id сессии, в базу не пишется. Rust, команды Tauri и id элементов не менялись.

Проверено в headless Chromium с заглушкой `__TAURI__`: ширина 280/360/900 px без выхода за край, сценарий «выбор режима на карте → цель 25 мин → старт → комната → закончить → сводка». В реальном окне WebKitGTK и с клавиатуры редизайн не проверялся. ИИ-тьютор в окне, отдельные интерфейсы для других доменов и графики удержания не делались: для них нет данных и команд, платные вызовы требуют отдельного одобрения.

Второй проход по запросу владельца: живой фон `whiteroom-app/ui/field.js` на WebGL 1 без зависимостей (7000 частиц, шесть скоплений режимов с яркостью по минутам из истории, ядро по числу карточек к повторению). Сцены: главный экран, протокол сессии (отдельный шаг), вход (частицы выстраиваются в кольца, Esc или кнопка пропускают), белая комната (фон на 16 % яркости, 30 к/с), другие разделы (приглушено). Переключатель «Фон: живой/тихий/выкл» хранится в localStorage. Адаптация: при кадре дольше ~22 мс частиц вдвое меньше до 1500, дальше принудительно тихий режим; при скрытом окне отрисовка стоит; без WebGL или после потери контекста остаётся статичный CSS-фон. SVG-атлас удалён. Проверено в headless Chromium (SwiftShader): шейдеры компилируются, сценарий старт/стоп и Esc проходят на 280/360/1280 px без выхода за край. WebGL в окне WebKitGTK и реальная частота кадров не проверены: offscreen-окно GTK не создаёт GL-контекст.

Третий проход: фон `field.js` стал голографическим мозгом. Силуэт задан эллипсоидами (полушария с бороздой, височные доли, мозжечок, ствол), кора — 9000 точек вдоль линий извилин, 1900 нейронов, граф из ближайших соседей и изогнутых волокон (глубина → кора, мозолистое тело). Импульсы до 200 одновременно идут по рёбрам графа, ветвятся и зажигают узлы; раз в 8–14 с волна по области. Данные: яркость шести областей — минуты по режимам, частота импульсов растёт с числом карточек к повторению, верный ответ в повторениях даёт волну и усиливает пройденные связи до закрытия окна, решённая задача и конец сессии — волна по сети. Наведение курсора рядом с нейроном запускает импульс. Переход в белую комнату не менялся по просьбе владельца. В подписи указано, что модель декоративная. Проверено в headless Chromium (SwiftShader): шейдеры собираются, импульсы рисуются по рёбрам (ручной прогон шага симуляции во временной копии), сценарий старт/стоп на 280/360/1280 px. Плавность на реальном GPU и WebGL в окне WebKitGTK не проверены.

## 2026-11-19 — живой тест Claude API

С согласия владельца выполнены по одному прямому запросу `review` и `practice math` через WHITE ROOM с временной базой. Экспорт ключа проверен в интерактивном Bash. Публичные материалы внесены через native messaging. Оба запроса уложились в лимиты (4 143 и 6 238 байт JSON). Ревью дало три правильных вопроса об утилитах Linux. Корреляция событий и сохранение порядка при удалении повторов в вопросы не попали. [pwn.college](https://pwn.college/linux-luminarium/data/) · [Разбор picoCTF Log Hunt](https://github.com/Ryan-Wachira/Log-Hunt-picoCTF) · [KC7](https://kc7cyber.com/module/a-rap-beef-an-intro-to-security-investigations-187)

Лабораторная «Журнал AUTH_FAIL» (3/5) прошла песочницу. Ответ пересчитан независимо по условию. Импорт в отдельную временную базу и проверка верного и неверного ответов прошли. [Файл лабораторной](../challenges/examples/claude-auth-fail.json)

## 2026-11-19 — карта хранения, проверка дизайна и тьютор

На временной БД подтвердились пути SQLite, rollback-журнал и WAL, состав экспорта, регистрация native messaging и сохранение README задачи. В изолированном Chromium список учебных сайтов пережил перезапуск, значение из session storage очистилось. Предложение о сроках и объёме очистки ждёт решения владельца. [Карта хранения](../docs/local-data-inventory.md)

Сборка интерфейса проверена с WebKitGTK 2.54.1 на отдельном Xwayland. На 900×640 клавиатура открыла протокол, стрелка переключила режим, сессия запустилась с двумя выключенными источниками. На 360×640 клавиатура завершила ту же сессию. Кадры фона различались. Частота кадров на нативном Wayland не измерялась.

Субагент добавил Rust-ядро тьютора, четыре Tauri-команды и проверки. Активная сессия получает локальную теорию с рекомендацией по режиму и направлению. Вне сессии Claude выбирает объяснение из каталога после подтверждения JSON. Пять новых тестов проверяют контракт, защиту от подсказок, одноразовый черновик, устаревшую сессию и отсутствие транспорта в активном режиме. Живых вызовов не было. [Задание для фронтенда Claude Code](../docs/prompt-claude-tutor.md)

Четвёртый проход (Claude Code, по запросу владельца): утренний брифинг, нейронная сфера и панели.
- Core: `whiteroom-core/src/brief.rs`, `Db::briefing(now, offset)` — последний день с занятиями до сегодняшнего (сессии, ответы, решённые задачи, основная цель, пояснения из карточек этого дня), последние выдержки с учебных сайтов, план на сегодня (карточки к повторению, новые вопросы, начатые задачи, доступные недельные проверки), фокус за 7 дней, доля верных повторений за 14 дней, повторения сейчас / 24 ч / 7 дней. Тест `briefing_shows_last_active_day` в `pilot.rs` (пустая база, пропуск дней, граница дня и часовой пояс).
- Tauri: команда `briefing(offset)` добавлена в invoke handler, `build.rs` и `capabilities/main.json`.
- Окно: брифинг при первом открытии за день (localStorage `wr-awake`), главы проявляются по очереди, Esc/«Пропустить» закрывают сразу, «Утренний брифинг» под показателями открывает повторно. Приоритеты кликабельны: повторения, практика, «Продолжить тему» заполняет протокол.
- Фон: вокруг мозга сфера из 900 нейронов с дугами по поверхности и мостами к коре; импульсы ходят по общему графу. Переход в белую комнату не менялся.
- Панели вокруг сферы: цели на сегодня, заметки и находки, память и повторения; на узком экране — под сценой.
Проверено: pilot 17/17, unit-тест CLI, `node --check`, headless Chromium (сфера, брифинг, Esc, повтор, «Продолжить тему», сценарий старт/стоп на 280/360/1280 px). Не проверено: реальное окно WebKitGTK и частота кадров на GPU.

## 2026-11-19 — интерфейс ИИ-тьютора (Claude Code, docs/prompt-claude-tutor.md)

Сделано в `whiteroom-app/ui/` поверх команд Codex `tutor_context/prepare/send/cancel`: раздел «Тьютор» в навигации (понятие и/или вопрос → предпросмотр) и диалог «Спросить тьютора» в белой комнате (рекомендуемое понятие, «Определение» → `question: null`, «Пример» → `Пример: <понятие>`). Облачный запрос — точный JSON обычным текстом с прокруткой и оценкой токенов, затем «Отправить»/«Отмена»; локальный ответ сразу. Повторное нажатие блокируется, черновик отправляется один раз, изменение понятия или вопроса отменяет черновик, устаревший черновик предлагает «Подготовить заново», ответ с чужим `sessionId` не показывается. Диалог: фокус на выборе понятия, Esc и «Закрыть» возвращают фокус на кнопку вызова, остальной интерфейс `inert`, таймер идёт; конец или смена сессии закрывает диалог и отменяет черновик. Состояние только в памяти окна.

Проверено с заглушкой IPC по контракту (scratchpad): 12 сценариев раздела и 9 сценариев диалога — отмена, двойное нажатие, устаревший черновик, смена сессии между предпросмотром и подтверждением, Esc и возврат фокуса, конец сессии во время подготовки, JSON прокручивается и кнопки подтверждения на экране — на 360 и 900 px в headless Chromium и в WebKitGTK 2.54.1 (offscreen, без аппаратного ускорения). Реальные вызовы Claude не делались.

## 2026-11-19 — синхронизация прогресса в D1

Владелец подтвердил перенос учебного прогресса и журналов активности, включая заголовки окон, домены и выдержки. Создана приватная база white-room-progress. После первой отправки сверено 80 записей в D1, повторный вызов отправил ноль изменений. Включён пользовательский таймер с интервалом 15 минут. Токен хранится в зашифрованном файле credentials systemd. Прошли 20 Python-тестов, в том числе фильтрация полей, повтор после сбоя и сохранение ранее скопированных записей. [Синхронизация](../docs/progress-sync.md)

## 2026-11-19 — разрешённые сайты по темам и таймер Wayland

По D12 Chromium использует список разрешённых доменов текущей темы. Google входит в каждый список. На временном профиле Chromium 155 с настоящим native host проверены переходы к разрешённым доменам, блокировка остальных, смена темы и снятие правил после остановки. В рабочем профиле установлено распакованное расширение, зарегистрирован локальный процесс и подтверждён его запуск браузером.

Проверка приложения Tauri с временной БД подтвердила сохранение разрешённого домена из протокола сессии, старт с выключенной записью и поверхность таймера в слое Overlay Hyprland. Сессия завершилась и таймер исчез. CLI-проверка подтвердила, что таймер продолжает работать после выхода команды запуска. Изменения находятся в рабочем дереве вместе с ранее начатыми правками.

## 2026-11-19 — чат тьютора и окно Wayland

По D14 фронтенд-агент переделал страницу тьютора в чат с прокручиваемой перепиской, исправил переполнения и выравнивание на основных экранах. Rust отвечает на распознаваемые вопросы каталога локально. Для произвольных вопросов вне сессии сохранён точный предпросмотр запроса. У маршрутизатора Claude отключено рассуждение, чтобы короткий бюджет был доступен ответу. Метаданные модели получены с рабочими учётными данными, платная генерация проверялась с фиксированным ответом.

Нативный чат открывается кнопкой рядом с таймером либо командой `whiteroom tutor-overlay`. Проверка в Hyprland поверх полноэкранного приложения подтвердила ввод, локальный ответ, закрытие по Esc, снятие окна при завершении сессии и возврат фокуса. Tauri/WebKitGTK на временной БД подтвердил локальный ответ без ключа, предпросмотр и отмену Claude, а также общую переписку с нативным тьютором. Новая сборка открыта с рабочими учётными данными.

После сообщения о нераспознанном вопросе добавлены разговорные формулировки для вопросов о корреляции и короткие продолжения по последнему объяснению в текущем чате. Сборка рабочего пространства и 27 тестов Rust прошли. Обновление собрано, проверка в пользовательском сеансе отложена.
