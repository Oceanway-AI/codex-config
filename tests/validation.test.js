import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validateBaseUrl } from '../src/validation.js';
test('accepts HTTP(S) provider paths including local fixtures', () => {
 for (const url of ['https://example.invalid/v1', 'http://127.0.0.1:1234']) assert.equal(validateBaseUrl(url),url);
});
test('rejects malformed, credential-bearing and non-HTTP URLs', () => {
 for (const url of ['not a url','ftp://example.invalid','https://user:pass@example.invalid','https://example.invalid?key=fake','https://example.invalid#fake','https://example.invalid?','https://example.invalid/ has-space']) assert.throws(()=>validateBaseUrl(url));
});
