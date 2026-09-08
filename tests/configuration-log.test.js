import { test } from 'node:test';
import assert from 'node:assert/strict';
import { redactLogMessage, createConfigurationLog } from '../src/configuration-log.js';

test('redacts known keys, bearer tokens and credentials embedded in URLs', () => {
  const text = redactLogMessage('known-key sk-fake-secret Bearer hidden-token https://user:password@example.test?a=1&api_key=query-secret', ['known-key']);
  for (const secret of ['known-key', 'sk-fake-secret', 'hidden-token', 'password', 'query-secret']) assert.ok(!text.includes(secret));
});

test('log caps retained rows, uses text nodes and clears without keeping errors', () => {
  const nodes = new Map();
  const doc = { createElement: () => ({ children: [], dataset: {}, textContent: '',
    append(...items) { items.forEach(item => { item.parent = this; this.children.push(item); }); },
    remove() { if (this.parent) this.parent.children = this.parent.children.filter(x => x !== this); },
    replaceChildren() { this.children = []; }, addEventListener() {}, ownerDocument: doc,
  }) };
  for (const name of ['#configuration-log', '#clear-logs-button']) nodes.set(name, doc.createElement('div'));
  const $ = name => nodes.get(name);
  const logger = createConfigurationLog({ $, limit: 2 });
  logger.append('first'); logger.append('<img src=x>'); logger.append('unknown-private-value', 'error');
  const rows = $('#configuration-log').children;
  assert.equal(rows.length, 2);
  assert.equal(rows[0].children[1].textContent, '<img src=x>');
  assert.ok(!rows[1].children[1].textContent.includes('unknown-private-value'));
  logger.clear(); assert.equal($('#configuration-log').children.length, 1);
});
