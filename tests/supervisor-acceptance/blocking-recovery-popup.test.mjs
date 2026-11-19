import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const html = readFileSync(new URL('../../extension/popup.html', import.meta.url), 'utf8')
  .replace(/<!--[\s\S]*?-->/g, '');
const tags = [...html.matchAll(/<[a-z][\w-]*\b[^>]*>/gi)];

function attribute(tag, name) {
  const match = tag.match(new RegExp('\\s' + name + '(?=\\s|=|>)(?:\\s*=\\s*(?:"([^"]*)"|\\x27([^\\x27]*)\\x27|([^\\s>]+)))?', 'i'));
  return match ? match[1] ?? match[2] ?? match[3] ?? '' : undefined;
}

function element(id) {
  const matches = tags.filter(([tag]) => attribute(tag, 'id') === id);
  assert.equal(matches.length, 1, "#" + id + ' must occur once');
  return matches[0];
}

function content(tag) {
  const name = tag[0].match(/^<([\w-]+)/)[1];
  const rest = html.slice(tag.index + tag[0].length);
  const end = rest.search(new RegExp('</' + name + '\\s*>', 'i'));
  assert.notEqual(end, -1, 'Expected a closing tag for ' + name);
  return rest.slice(0, end).trim();
}

test('the popup has an empty status region and a hidden native recovery button', () => {
  const status = element('blocking-status');
  assert.equal(attribute(status[0], 'role'), 'status');
  assert.equal(content(status), '');

  const button = element('release-blocking');
  assert.match(button[0], /^<button\b/i);
  assert.equal(attribute(button[0], 'type'), 'button');
  assert.notEqual(attribute(button[0], 'hidden'), undefined);
  assert.match(content(button), /[А-Яа-яЁё]/u);
});

test('the existing popup elements and script remain available', () => {
  for (const id of ['domain', 'toggle', 'note', 'host']) element(id);
  assert.ok(tags.some(([tag]) => /^<script\b/i.test(tag) && attribute(tag, 'src') === 'popup.js'));
});
