const $ = id => document.getElementById(id);

const releaseButton = $('release-blocking');
const blockingStatus = $('blocking-status');

async function refreshBlocking(release = false) {
  try {
    const result = await chrome.runtime.sendMessage({ type: release ? 'release-blocking' : 'blocking-status' });
    releaseButton.hidden = !result.canRelease;
    blockingStatus.textContent = result.error || (result.count === 0
      ? release ? 'Блокировка WHITE ROOM снята.' : 'Блокировка WHITE ROOM выключена.'
      : `Действующие правила блокировки WHITE ROOM: ${result.count}.`);
  } catch {
    blockingStatus.textContent = 'Не удалось проверить блокировку.';
  }
}

releaseButton.addEventListener('click', async () => {
  releaseButton.disabled = true;
  await refreshBlocking(true);
  releaseButton.disabled = false;
  (releaseButton.hidden ? blockingStatus : releaseButton).focus();
});
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'session' && changes.hostOk) refreshBlocking();
});
refreshBlocking();

(async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const { hostOk, hostError, capture } = await chrome.storage.session.get(['hostOk', 'hostError', 'capture']);
  $('host').textContent = hostOk
    ? 'Связь с whiteroom: есть.'
    : `Связи с whiteroom нет: ${hostError || 'запусти whiteroom browser-setup'}.`;
  if (!tab || !/^https?:/.test(tab.url || '')) {
    $('domain').textContent = 'Эта вкладка не записывается.';
    $('toggle').hidden = true;
    return;
  }
  const url = new URL(tab.url);
  const domain = url.host;
  const origin = `${url.protocol}//${domain}/*`;
  let { study = [] } = await chrome.storage.local.get('study');

  const render = () => {
    const on = study.includes(domain);
    $('domain').textContent = domain;
    $('toggle').textContent = on ? 'Убрать из учебных' : 'Учебный сайт';
    $('note').textContent = !hostOk || !capture ? 'Запись вкладок выключена.' : on
      ? 'Текст страниц сохраняется локально и уходит в разбор ИИ только после твоего подтверждения.'
      : 'Записываются только домен и заголовок. Отметь сайт учебным — ИИ будет спрашивать по тексту страниц.';
  };
  render();

  $('toggle').onclick = async () => {
    if (study.includes(domain)) {
      study = study.filter(d => d !== domain);
      await chrome.permissions.remove({ origins: [origin] });
    } else {
      if (!(await chrome.permissions.request({ origins: [origin] }))) return;
      study = [...study, domain];
    }
    await chrome.storage.local.set({ study });
    chrome.runtime.sendMessage({ type: 'refresh' });
    render();
  };
})();
