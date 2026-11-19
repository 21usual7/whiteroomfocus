// Шлёт в whiteroom домен и заголовок активной вкладки; текст — только с сайтов, отмеченных учебными.
const HOST = 'com.whiteroom.host';
let port = null;
let last = '';

let capture = null;
let generation = 0;
let pendingState = 0;
function refresh() {
  generation++;
  if (!port) {
    const connection = chrome.runtime.connectNative(HOST);
    port = connection;
    connection.onMessage.addListener(r => {
      if (connection !== port) return;
      const next = r.ok && Number.isSafeInteger(r.capture) ? r.capture : null;
      if (next !== capture) { generation++; last = ""; }
      capture = next;
      chrome.storage.session.set({ hostOk: !!r.ok, hostError: r.error || "", session: !!r.session, capture });
      if (r.ok && r.request === pendingState && typeof r.session === 'boolean' && Array.isArray(r.block)) {
        blockingHostConfirmed = true;
        enqueueBlocking(async () => {
          if (connection === port && r.request === pendingState) {
            await applyBlock(r.session ? r.block : []);
          }
        }).catch(() => {});
      }
      if (capture !== null && r.request === pendingState) reportActive(connection, capture, generation);
    });
    connection.onDisconnect.addListener(() => {
      if (connection !== port) return;
      generation++;
      capture = null;
      last = "";
      port = null;
      blockingHostConfirmed = false;
      chrome.storage.session.set({ hostOk: false, hostError: chrome.runtime.lastError?.message || "отключено", session: false, capture: null });
    });
  }
  port.postMessage({ type: "state", request: ++pendingState });
}

const FIRST_BLOCK_RULE = 1_000_000;
const LAST_LEGACY_RULE = 30_000;
const FRAME_TYPES = ['main_frame', 'sub_frame'];
let blockingWork = Promise.resolve();
let blockingError = '';
let blockingHostConfirmed = false;

function enqueueBlocking(action) {
  const work = blockingWork.then(action);
  blockingWork = work.catch(() => {});
  return work;
}

function managedFilter(rule) {
  const { action, condition } = rule;
  const frames = condition?.resourceTypes;
  if (action?.type !== 'block' || (rule.priority ?? 1) !== 1 ||
      Object.keys(condition ?? {}).some(key => !['urlFilter', 'resourceTypes'].includes(key)) ||
      frames?.length !== FRAME_TYPES.length || !FRAME_TYPES.every(frame => frames.includes(frame)) ||
      !/^\|\|[a-z0-9.-]+\^$/.test(condition.urlFilter)) return null;
  return rule.id >= FIRST_BLOCK_RULE || (rule.id >= 1 && rule.id <= LAST_LEGACY_RULE)
    ? condition.urlFilter : null;
}

async function applyBlock(domains) {
  const filters = [...new Set(domains)].sort().map(domain => `||${domain}^`);
  try {
    const installed = await chrome.declarativeNetRequest.getDynamicRules();
    const managed = installed.filter(managedFilter);
    const activeFilters = managed.map(managedFilter).sort();
    if (activeFilters.length !== filters.length || activeFilters.some((filter, i) => filter !== filters[i])) {
      const occupied = new Set(installed.filter(rule => !managedFilter(rule)).map(rule => rule.id));
      let id = FIRST_BLOCK_RULE;
      const addRules = filters.map(urlFilter => {
        while (occupied.has(id)) id++;
        return { id: id++, action: { type: 'block' }, condition: { urlFilter, resourceTypes: FRAME_TYPES } };
      });
      await chrome.declarativeNetRequest.updateDynamicRules({
        removeRuleIds: managed.map(rule => rule.id),
        addRules,
      });
    }
    blockingError = '';
  } catch (error) {
    blockingError = 'Не удалось обновить правила блокировки.';
    throw error;
  }
}

async function blockingStatus(release) {
  if (release && !blockingHostConfirmed) {
    try {
      await applyBlock([]);
    } catch {
      // Read back actual rules so the popup can offer another attempt.
    }
  }
  const rules = await chrome.declarativeNetRequest.getDynamicRules();
  const count = rules.filter(managedFilter).length;
  return { count, canRelease: !blockingHostConfirmed && count > 0, error: blockingError };
}

// Раз в 30 с спрашиваем, не началась ли или не закончилась ли сессия.
chrome.alarms.create('state', { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(refresh);
// Service worker стартует при запуске браузера и после простоя — сразу узнаём состояние.
refresh();

// Выполняется на странице: заголовки и основной текст, не больше 4000 символов.
function excerpt(expectedOrigin) {
  if (location.origin !== expectedOrigin) return null;
  const root = document.querySelector('article, main, [role=main]') || document.body;
  const heads = [...document.querySelectorAll('h1, h2, h3')].slice(0, 30).map(h => h.innerText.trim()).filter(Boolean);
  const text = (heads.join('\n') + '\n\n' + root.innerText).replace(/\n\s*\n+/g, '\n').slice(0, 4000);
  return { origin: location.origin, text };
}

async function reportActive(connection, session, revision) {
  const valid = () => port === connection && capture === session && generation === revision;
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!valid() || !tab || !/^https?:/.test(tab.url || "")) return;
  const url = new URL(tab.url);
  const domain = url.host;
  const permission = { origins: [url.origin + "/*"] };
  const { study = [] } = await chrome.storage.local.get("study");
  let text = null;
  if (valid() && study.includes(domain) && await chrome.permissions.contains(permission) && valid()) {
    try {
      const [{ result }] = await chrome.scripting.executeScript({ target: { tabId: tab.id }, func: excerpt, args: [url.origin] });
      if (result?.origin === url.origin && await chrome.permissions.contains(permission)) text = result.text;
    } catch {
      // Chromium restricts scripting on some pages.
    }
  }
  if (!valid()) return;
  const key = JSON.stringify([domain, tab.title, Boolean(text)]);
  if (key === last) return;
  last = key;
  if (text) connection.postMessage({ type: "site_permission", capture: session, domain });
  connection.postMessage({ type: "page", capture: session, domain, title: tab.title || "", excerpt: text });
}

chrome.tabs.onActivated.addListener(refresh);
chrome.tabs.onUpdated.addListener((_id, info) => { if (info.status === "complete") refresh(); });
chrome.windows.onFocusChanged.addListener(refresh);
chrome.permissions.onRemoved.addListener(refresh);
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes.study) { last = ""; refresh(); }
});
chrome.runtime.onMessage.addListener((message, _sender, reply) => {
  if (message.type === 'refresh') {
    last = '';
    refresh();
  } else if (message.type === 'blocking-status' || message.type === 'release-blocking') {
    enqueueBlocking(() => blockingStatus(message.type === 'release-blocking')).then(
      reply, () => reply({ error: 'Не удалось прочитать правила блокировки.' }),
    );
    return true;
  }
});
