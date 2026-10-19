import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { validateChannel, validateManifest, releaseBase, buildMetadata } from './desktop-release.mjs';
const fixture = JSON.parse(readFileSync('src-tauri/tests/fixtures/update-signature.json', 'utf8'));
const channel = { endpoint: 'https://github.com/example/desktop/releases/latest/download/latest.json', publicKey: fixture.publicKey, downloadOrigins: ['https://github.com'] };
const manifest = () => ({ version: '22.7.50', pub_date: '2026-10-19T00:00:00Z', platforms: { 'windows-x86_64': { url: 'https://github.com/example/desktop/releases/download/desktop-v22.7.50/CaseSim.exe', signature: fixture.signature } } });

test('official metadata binds separate immutable Desktop, approved CaseSim and preparation revisions', () => {
  const inputs = { sha: 'a'.repeat(40), casesimSha: 'b'.repeat(40), preparationSha: 'c'.repeat(40), version: '22.7.52', installerHash: 'd'.repeat(64), authenticode: false };
  const metadata = buildMetadata(inputs);
  assert.equal(metadata.source, 'https://github.com/shaedy180/CaseSim-Desktop');
  assert.equal(metadata.commit, inputs.sha);
  assert.equal(metadata.casesim_commit, inputs.casesimSha);
  assert.equal(metadata.preparation_commit, inputs.preparationSha);
  for (const key of ['sha', 'casesimSha', 'preparationSha']) {
    for (const value of [undefined, 'main', 'a'.repeat(7), 'A'.repeat(40)]) assert.throws(() => buildMetadata({ ...inputs, [key]: value }));
  }
});
test('unconfigured preview is valid, publication fails closed', () => {
  const disabled = { endpoint: null, publicKey: null, downloadOrigins: [] };
  assert.equal(validateChannel(disabled), false);
  assert.throws(() => validateChannel(disabled, true));
});

test('production channel pins the dedicated host and manifest', () => {
  const production = JSON.parse(readFileSync('src-tauri/update-channel.json', 'utf8'));
  assert.equal(production.endpoint, `${releaseBase}/latest.json`);
  assert.deepEqual(production.downloadOrigins, ['https://releases.shaedy.de']);
  assert.equal(validateChannel(production, true), true);
});
test('public channel requires TLS, no credentials and a real public key', () => {
  assert.equal(validateChannel(channel, true), true);
  for (const endpoint of ['http://github.com/update', 'https://user:pass@github.com/update', 'https://127.0.0.1/update', 'https://tauri.localhost/update']) assert.throws(() => validateChannel({ ...channel, endpoint }));
  assert.throws(() => validateChannel({ ...channel, publicKey: 'placeholder' }));
});
test('release manifest pins the platform, versioned HTTPS artifact and signed version', () => {
  validateManifest(manifest(), channel, '22.7.50');
  for (const url of ['http://github.com/example/desktop-v22.7.50/a.exe', 'https://evil.test/desktop-v22.7.50/a.exe', 'https://github.com/latest/a.exe']) {
    const m = manifest(); m.platforms['windows-x86_64'].url = url; assert.throws(() => validateManifest(m, channel, m.version));
  }
  const forged = manifest(); forged.version = '22.7.51'; forged.platforms['windows-x86_64'].url = forged.platforms['windows-x86_64'].url.replace('22.7.50', '22.7.51');
  assert.throws(() => validateManifest(forged, channel, '22.7.51'));
});
