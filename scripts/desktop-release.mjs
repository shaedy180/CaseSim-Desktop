import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

export function validateChannel(channel, required = false) {
  assert.deepEqual(Object.keys(channel).sort(), ['downloadOrigins', 'endpoint', 'publicKey']);
  if (channel.endpoint === null && channel.publicKey === null && channel.downloadOrigins.length === 0) {
    assert.ok(!required, 'A public HTTPS update channel and public signing key must be configured before release');
    return false;
  }
  const secure = value => {
    const u = new URL(value);
    assert.equal(u.protocol, 'https:'); assert.equal(u.port, '');
    assert.equal(u.username, ''); assert.equal(u.password, '');
    assert.ok(u.hostname.includes('.') && !u.hostname.endsWith('.localhost') && !/^\[|^[\d.]+$/.test(u.hostname));
    return u;
  };
  secure(channel.endpoint);
  const key = Buffer.from(channel.publicKey, 'base64').toString('utf8').trim().split(/\r?\n/);
  assert.match(key[0], /^untrusted comment:/);
  assert.equal(Buffer.from(key[1], 'base64').length, 42, 'Expected a Tauri/Minisign Ed25519 public key');
  assert.ok(channel.downloadOrigins.length);
  for (const origin of channel.downloadOrigins) assert.equal(secure(origin).origin, origin);
  return true;
}

export const releaseBase = 'https://releases.shaedy.de/casesim/windows';

export function buildMetadata({ sha, casesimSha, preparationSha, version, installerHash, authenticode }) {
  for (const value of [sha, casesimSha, preparationSha]) assert.match(value ?? '', /^[a-f0-9]{40}$/, 'Exact Desktop, CaseSim and preparation revisions required');
  assert.match(version, /^\d+\.\d+\.\d+$/);
  assert.match(installerHash, /^[a-f0-9]{64}$/);
  assert.equal(typeof authenticode, 'boolean');
  return { commit: sha, desktop_version: version, installer_sha256: installerHash, authenticode,
    signature_version_bound: true, source: 'https://github.com/shaedy180/CaseSim-Desktop',
    casesim_commit: casesimSha, preparation_commit: preparationSha };
}

export function validateManifest(manifest, channel, expectedVersion) {
  validateChannel(channel, true);
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.equal(manifest.version, expectedVersion);
  assert.ok(Number.isFinite(Date.parse(manifest.pub_date)));
  assert.deepEqual(Object.keys(manifest.platforms), ['windows-x86_64']);
  const p = manifest.platforms['windows-x86_64'];
  const url = new URL(p.url);
  assert.equal(url.protocol, 'https:'); assert.equal(url.username, ''); assert.equal(url.password, '');
  assert.ok(channel.downloadOrigins.includes(url.origin));
  assert.ok(url.pathname.includes(`/desktop-v${expectedVersion}/`), 'Downloads must be versioned');
  const lines = Buffer.from(p.signature, 'base64').toString('utf8').trim().split(/\r?\n/);
  assert.equal(lines.length, 4);
  assert.match(lines[0], /^untrusted comment:/);
  assert.ok(lines[2].startsWith('trusted comment:'));
  assert.ok(lines[2].split('\t').includes(`version:${expectedVersion}`), 'Signature must bind the release version');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const channel = JSON.parse(readFileSync('src-tauri/update-channel.json', 'utf8'));
  const version = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')).version;
  const release = process.argv[2] === 'assemble';
  validateChannel(channel, release || process.argv.includes('--required'));
  if (release) {
    const sha = process.env.BUILD_SHA;
    const casesimSha = process.env.CASESIM_SHA;
    const preparationSha = process.env.PREPARATION_SHA;
    assert.match(casesimSha ?? '', /^[a-f0-9]{40}$/, 'Approved CaseSim source required');
    assert.match(preparationSha ?? '', /^[a-f0-9]{40}$/, 'Private preparation source required');
    assert.match(sha ?? '', /^[a-f0-9]{40}$/);
    assert.equal(channel.endpoint, `${releaseBase}/latest.json`);
    assert.deepEqual(channel.downloadOrigins, ['https://releases.shaedy.de']);
    const name = `CaseSim_${version}_x64-setup.exe`;
    const source = join('src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis', name);
    const directory = 'artifacts/desktop-release';
    mkdirSync(directory, { recursive: true });
    copyFileSync(source, join(directory, name));
    const signature = readFileSync(`${source}.sig`, 'utf8').trim();
    const notes = JSON.parse(readFileSync('desktop/release-notes.json', 'utf8'))[0];
    assert.equal(notes.version, version);
    const manifest = { version, notes: notes.sections.map(section => [section.title, ...section.notes].join('\n')).join('\n\n'), pub_date: new Date().toISOString(), platforms: { 'windows-x86_64': { signature, url: `${releaseBase}/desktop-v${version}/${name}` } } };
    validateManifest(manifest, channel, version);
    writeFileSync(join(directory, 'latest.json'), JSON.stringify(manifest, null, 2) + '\n');
    copyFileSync(`${source}.sig`, join(directory, `${name}.sig`));
    const hash = file => createHash('sha256').update(readFileSync(join(directory, file))).digest('hex');
    const metadata = buildMetadata({ sha, casesimSha, preparationSha, version, installerHash: hash(name), authenticode: process.env.DESKTOP_AUTHENTICODE === 'true' });
    writeFileSync(join(directory, 'build-info.json'), JSON.stringify(metadata, null, 2) + '\n');
    writeFileSync(join(directory, 'SHA256SUMS.txt'), [name, `${name}.sig`, 'latest.json', 'build-info.json'].map(file => `${hash(file)}  ${file}`).join('\n') + '\n');
    console.log(`Prepared immutable Desktop ${version} release for ${sha}`);
  }
}
