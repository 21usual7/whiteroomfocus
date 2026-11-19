const $ = id => document.getElementById(id);

(async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const { hostOk, hostError, session, capture } = await chrome.storage.session.get(['hostOk', 'hostError', 'session', 'capture']);
  $('host').textContent = hostOk
    ? `Связь с whiteroom: есть.${session ? ' Идёт сессия — отвлекающие сайты заблокированы.' : ''}`
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
