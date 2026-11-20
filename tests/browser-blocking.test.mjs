import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('../extension/background.js', import.meta.url), 'utf8');
const legacy = { id: 1, action: { type: 'block' },
  condition: { urlFilter: '||study.example^', resourceTypes: ['main_frame', 'sub_frame'] } };
const other = { id: 2, action: { type: 'allow' },
  condition: { urlFilter: '||unrelated.example^', resourceTypes: ['main_frame'] } };

function worker(initial) {
  const noEvent = { addListener() {} };
  const state = { rules: [...initial], failures: 0, updates: 0, requests: [] };
  let receive, disconnect, alarm, message;
  const port = {
    postMessage: data => state.requests.push(data),
    onMessage: { addListener: callback => { receive = callback; } },
    onDisconnect: { addListener: callback => { disconnect = callback; } },
  };
  const chrome = {
    runtime: {
      connectNative: () => port,
      onMessage: { addListener: callback => { message = callback; } },
    },
    storage: { session: { set() {} }, onChanged: noEvent },
    alarms: { create() {}, onAlarm: { addListener: callback => { alarm = callback; } } },
    tabs: { onActivated: noEvent, onUpdated: noEvent },
    windows: { onFocusChanged: noEvent },
    permissions: { onRemoved: noEvent },
    declarativeNetRequest: {
      getDynamicRules: async () => state.rules,
      updateDynamicRules: async ({ removeRuleIds, addRules }) => {
        state.updates++;
        if (state.failures > 0) {
          state.failures--;
          throw new Error('synthetic update failure');
        }
        state.rules = state.rules.filter(rule => !removeRuleIds.includes(rule.id)).concat(addRules);
      },
    },
  };
  vm.runInNewContext(source, { chrome });
  return {
    state,
    request: () => alarm(),
    disconnect: () => disconnect(),
    receive: (session, blocked, request = state.requests.at(-1).request) =>
      receive({ ok: true, request, session, blocked, capture: null }),
    message: type => new Promise(resolve => {
      assert.equal(message({ type }, {}, resolve), true);
    }),
  };
}

test('restart and session end reconcile installed WHITE ROOM rules only', async () => {
  const run = worker([legacy, other]);
  run.receive(false, []);
  assert.equal((await run.message('blocking-status')).count, 0);
  assert.deepEqual(run.state.rules, [other]);
  run.request();
  run.receive(true, ['study.example', 'video.example']);
  assert.equal((await run.message('blocking-status')).count, 1);
  const { condition } = run.state.rules[1];
  assert.equal(condition.regexFilter, undefined);
  assert.deepEqual(Array.from(condition.requestDomains), ['study.example', 'video.example']);
  run.request();
  run.receive(false, []);
  assert.equal((await run.message('blocking-status')).count, 0);
  assert.deepEqual(run.state.rules, [other]);
});

test('a failed update can retry the same desired state and stale replies are ignored', async () => {
  const run = worker([other]);
  run.state.failures = 1;
  run.receive(true, ['study.example', 'video.example']);
  assert.match((await run.message('blocking-status')).error, /Не удалось обновить/);
  run.request();
  run.receive(true, ['stale.example'], 1);
  assert.equal((await run.message('blocking-status')).count, 0);
  run.receive(true, ['study.example', 'video.example']);
  assert.equal((await run.message('blocking-status')).error, '');
  const { condition } = run.state.rules[1];
  assert.equal(condition.regexFilter, undefined);
  assert.deepEqual(Array.from(condition.requestDomains), ['study.example', 'video.example']);
  assert.equal(run.state.updates, 2);
});

test('disconnect retains active rules until manual release and reconnect rechecks the session', async () => {
  const run = worker([legacy, other]);
  run.receive(true, ['study.example', 'video.example']);
  assert.equal((await run.message('release-blocking')).canRelease, false);
  assert.equal(run.state.updates, 1);
  run.disconnect();
  assert.equal((await run.message('blocking-status')).canRelease, true);
  run.state.failures = 1;
  assert.equal((await run.message('release-blocking')).canRelease, true);
  assert.equal((await run.message('release-blocking')).count, 0);
  assert.deepEqual(run.state.rules, [other]);
  run.request();
  run.receive(true, ['study.example', 'video.example']);
  assert.equal((await run.message('blocking-status')).count, 1);
});

test('a worker awaiting its first host reply can offer recovery for previously installed rules', async () => {
  const run = worker([legacy, other]);
  assert.equal((await run.message('blocking-status')).canRelease, true);
  assert.equal((await run.message('release-blocking')).count, 0);
  assert.deepEqual(run.state.rules, [other]);
});

test('a list change replaces the blocked domains without duplicating the block rule', async () => {
  const run = worker([legacy, other]);
  run.receive(true, ['first.example', 'video.example']);
  assert.equal((await run.message('blocking-status')).count, 1);
  run.request();
  run.receive(true, ['video.example', 'second.example']);
  assert.equal((await run.message('blocking-status')).count, 1);
  assert.deepEqual(Array.from(run.state.rules[1].condition.requestDomains), ['second.example', 'video.example']);
  run.request();
  run.receive(true, ['second.example', 'video.example']);
  await run.message('blocking-status');
  assert.equal(run.state.updates, 2);
});

test('an empty list installs no rule and an old topic allowlist rule is removed', async () => {
  const allowlist = { id: 1_000_000, action: { type: 'block' },
    condition: { regexFilter: '^https?://', excludedRequestDomains: ['google.com'], resourceTypes: ['main_frame', 'sub_frame'] } };
  const run = worker([allowlist, other]);
  run.receive(true, []);
  assert.equal((await run.message('blocking-status')).count, 0);
  assert.deepEqual(run.state.rules, [other]);
});

test('a host without the blocked list keeps rules and reports the outdated host', async () => {
  const run = worker([other]);
  run.receive(true, ['video.example']);
  assert.equal((await run.message('blocking-status')).count, 1);
  run.request();
  run.receive(true, undefined);
  const status = await run.message('blocking-status');
  assert.match(status.error, /Обнови локальный процесс/);
  assert.equal(status.count, 1);
});
