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
      applyBlock(r.block || []);
      if (capture !== null && r.request === pendingState) reportActive(connection, capture, generation);
    });
    connection.onDisconnect.addListener(() => {
      if (connection !== port) return;
      generation++;
      capture = null;
      last = "";
      port = null;
      chrome.storage.session.set({ hostOk: false, hostError: chrome.runtime.lastError?.message || "отключено", capture: null });
    });
  }
  port.postMessage({ type: "state", request: ++pendingState });
}

// Во время сессии whiteroom присылает список доменов; блокирует сам браузер (declarativeNetRequest).
let blocked = '';
async function applyBlock(domains) {
  const key = domains.join(',');
  if (key === blocked) return;
  blocked = key;
  const old = await chrome.declarativeNetRequest.getDynamicRules();
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: old.map(r => r.id),
    addRules: domains.map((d, i) => ({
      id: i + 1,
      action: { type: 'block' },
      condition: { urlFilter: `||${d}^`, resourceTypes: ['main_frame', 'sub_frame'] },
    })),
  });
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
chrome.runtime.onMessage.addListener(m => { if (m.type === "refresh") { last = ""; refresh(); } });
