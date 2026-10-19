import { execFileSync } from 'node:child_process';
import { readFile, readdir, mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { createHash } from 'node:crypto';

// Derive attribution from the locked Windows dependency graph, including build
// tools conservatively. Generated notices are installer resources, not sources.
const metadata = JSON.parse(execFileSync('cargo', [
  'metadata', '--manifest-path', 'src-tauri/Cargo.toml', '--locked',
  '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc',
], { encoding: 'utf8', maxBuffer: 20 * 1024 * 1024 }));
const used = new Set(metadata.resolve.nodes.map(node => node.id));
const packages = metadata.packages.filter(pkg => pkg.source && used.has(pkg.id))
  .sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`));
const sections = ['CaseSim Desktop third-party notices',
  'Unmodified dependency source is available at each exact-version crates.io download URL below. This includes source for MPL-2.0 components. The upstream licenses and copyright notices follow.'];
for (const pkg of packages) {
  const directory = dirname(pkg.manifest_path);
  const files = (await readdir(directory, { withFileTypes: true }))
    .filter(file => file.isFile() && /^(licen[cs]e|copying|copyright|notice|unlicense)([-_.]|$)/i.test(file.name))
    .map(file => join(directory, file.name));
  if (pkg.license_file && !files.includes(resolve(directory, pkg.license_file))) {
    files.push(resolve(directory, pkg.license_file));
  }
  const texts = await Promise.all(files.map(file => readFile(file, 'utf8')));
  if (!texts.length && pkg.license === 'MPL-2.0') {
    const response = await fetch('https://www.mozilla.org/media/MPL/2.0/index.txt');
    if (!response.ok) throw new Error('MPL license download failed');
    const text = await response.text();
    if (createHash('sha256').update(text).digest('hex') !== '3f3d9e0024b1921b067d6f7f88deb4a60cbe7a78e76c64e3f1d7fc3b779b9d04') {
      throw new Error('MPL license text changed; review before distributing');
    }
    texts.push(text);
  }
  if (!texts.length) {
    // Some workspace crates omit root licenses from their published tarballs.
    // Resolve only their immutable recorded upstream Git commit, never HEAD.
    const vcs = JSON.parse(await readFile(join(directory, '.cargo_vcs_info.json'), 'utf8'));
    const repository = pkg.repository?.match(/^https:\/\/github\.com\/([^/]+\/[^/]+?)(?:\.git)?$/)?.[1];
    if (!repository || !/^[a-f0-9]{40}$/.test(vcs.git?.sha1 ?? '')) {
      throw new Error(`Cannot resolve pinned license for ${pkg.name}`);
    }
    for (const filename of ['LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE-2.0']) {
      const url = `https://raw.githubusercontent.com/${repository}/${vcs.git.sha1}/${filename}`;
      const response = await fetch(url);
      if (response.ok) texts.push(`${url}\n\n${await response.text()}`);
      else if (response.status !== 404) throw new Error(`License download failed: ${response.status}`);
    }
  }
  if (!texts.length) throw new Error(`License text missing: ${pkg.name} ${pkg.version}`);
  sections.push(`\n${'='.repeat(72)}\n${pkg.name} ${pkg.version}\nLicense: ${pkg.license ?? 'see license text'}\nSource: https://crates.io/api/v1/crates/${pkg.name}/${pkg.version}/download\nRepository: ${pkg.repository ?? ''}\n\n${texts.join('\n\n')}`);
}
sections.push(await readFile('desktop/FONT-LICENSE.txt', 'utf8'));
await mkdir('src-tauri/target/notices', { recursive: true });
await writeFile('src-tauri/target/notices/THIRD-PARTY-NOTICES.txt', sections.join('\n\n'));
console.log(`Prepared attribution for ${packages.length} locked dependencies and JetBrains Mono`);
