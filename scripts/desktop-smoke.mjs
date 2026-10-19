import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { mkdir, writeFile, appendFile, readFile } from 'node:fs/promises';
import { setTimeout } from 'node:timers/promises';
import { chromium } from '@playwright/test';

// This is an isolated runner test. Never reuse a person's real WebView2 profile.
if (!process.env.CI || process.platform !== 'win32') throw new Error('Windows CI runner required');
await mkdir('artifacts/desktop', { recursive: true });
let executable;
const endpoint = 'http://127.0.0.1:9222';
const results = [];
const metrics = { launches: [] };
const desktopVersion = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8')).version;
const native = mode => execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/desktop-smoke.ps1', '-Mode', mode, '-ProcessId', String(app?.pid ?? 0)], { encoding: 'utf8', windowsHide: true }).trim();
let app;
let browser;
let context;
let page;
let offline = false;
let firstNavigation;
let fixtureSessionObserved = false;
let serverFailure = false;
let accessFailure = 0;
const root = 'https://shaedy.de/casesim/';
const deepLink = `${root}inventory?desktop-fixture=1#collection`;

async function stop() {
  if (app && app.exitCode === null) {
    // Graceful WM_CLOSE gives WebView2 time to persist its profile and cookies.
    execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      `$p = Get-Process -Id ${app.pid}; $null = $p.CloseMainWindow(); $p.WaitForExit(10000)`], { windowsHide: true });
    if (app.exitCode === null) await setTimeout(1000);
    if (app.exitCode === null) app.kill();
  }
  if (browser) await browser.close().catch(() => {});
  browser = undefined;
  await setTimeout(1000);
}

async function start() {
  const started = performance.now();
  app = spawn(executable, [], {
    windowsHide: true,
    env: {
      ...process.env,
      // Block all real hosted traffic even before the test attaches. Routing
      // below supplies fixtures before DNS. Never enable debugging in installers.
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9222 --host-resolver-rules="MAP shaedy.de ~NOTFOUND, MAP auth.shaedy.de ~NOTFOUND"',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let output = '';
  app.stdout.on('data', data => { output += data; });
  app.stderr.on('data', data => { output += data; });
  app.on('error', error => { output += error.message; });
  let connectionError;
  for (let attempt = 0; attempt < 60; attempt++) {
    try {
      browser = await chromium.connectOverCDP(endpoint, { timeout: 1500 });
      break;
    } catch (error) {
      connectionError = error.message;
      if (app.exitCode !== null) throw new Error(`Release app exited: ${output}`);
      await setTimeout(500);
    }
  }
  await writeFile('artifacts/desktop/runtime.log', output);
  assert.ok(browser, `release executable exposes the test-only CDP port: ${connectionError ?? ''} ${output}`);
  context = browser.contexts()[0];
  let fixtureReady = false;
  await context.route('https://**/*', async route => {
    const request = route.request();
    const url = new URL(request.url());
    await appendFile('artifacts/desktop/runtime.log', `Fixture route: ${url.origin}${url.pathname}\n`);
    if (!['shaedy.de', 'auth.shaedy.de'].includes(url.hostname)) return route.abort();
    if (!fixtureReady || offline) return route.abort('internetdisconnected');
    if (url.hostname === 'shaedy.de') {
      firstNavigation ??= request.url();
      fixtureSessionObserved = ((await request.allHeaders()).cookie ?? '').includes('casesim_fixture_session=present');
      if (accessFailure) return route.fulfill({ status: accessFailure, contentType: 'text/html', body: '<h1>Fixture access denied</h1>' });
      if (serverFailure) return route.fulfill({ status: 503, contentType: 'text/html', body: '<h1>Fixture unavailable</h1>' });
      // CaseSim's real redirectToAuthLogin uses location.assign. Match that
      // contract instead of a synthetic HTTP redirect, whose follow-up request
      // can bypass WebView2/CDP interception and hit the DNS isolation rule.
      if (!fixtureSessionObserved) return route.fulfill({ contentType: 'text/html', body: `<script>location.assign(${JSON.stringify(`https://auth.shaedy.de/login?return=${encodeURIComponent(deepLink)}`)})</script>` });
      return route.fulfill({ contentType: 'text/html', body: `<!doctype html><title>Fixture game</title><h1>CaseSim fixture</h1><a href="/casesim/museum">Museum fixture</a><button id="popup" onclick="window.open('/casesim/quests','_blank')">Trusted popup</button><canvas id="gpu"></canvas>` });
    }
    if (url.pathname === '/auth/login') return route.fulfill({ status: 200, contentType: 'application/json', headers: { 'set-cookie': 'casesim_fixture_session=present; Domain=.shaedy.de; Path=/; Max-Age=3600; Secure; HttpOnly; SameSite=Lax' }, body: '{"ok":true}' });
    return route.fulfill({ contentType: 'text/html', body: `<!doctype html><title>Fixture auth</title><h1>Auth navigation fixture</h1><button id="fixture-login">Continue fixture session</button><script>document.getElementById('fixture-login').onclick=async()=>{await fetch('/auth/login',{method:'POST'});location.assign(new URLSearchParams(location.search).get('return'))}</script>` });
  });
  for (let attempt = 0; attempt < 30 && !context.pages().length; attempt++) await setTimeout(200);
  page = context.pages()[0];
  assert.ok(page, 'main webview exists');
  page.on('pageerror', error => { void appendFile('artifacts/desktop/runtime.log', `Page error: ${error.message}\n`); });
  page.on('requestfailed', request => {
    const url = new URL(request.url());
    void appendFile('artifacts/desktop/runtime.log', `Request failed: ${url.origin}${url.pathname}: ${request.failure()?.errorText}\n`);
  });
  // Let the real automatic startup attempt reach recovery. Forcing a second
  // page.goto here races the in-flight native connect command and its completion.
  await page.getByRole('heading', { name: 'Connection unavailable', exact: true }).waitFor();
  metrics.launches.push({ bundledRecoveryMs: Math.round(performance.now() - started), fixture: 'blocked DNS, then native navigation failure; not production game startup' });
  fixtureReady = true;
  results.push('Clean release launch reaches bundled recovery after a navigation failure');
}

try {
  metrics.installer = JSON.parse(native('install'));
  executable = metrics.installer.executable;
  results.push('NSIS silently installs the expected Desktop version; all runtime checks use the installed application');
  await start();
  await page.screenshot({ path: 'artifacts/desktop/recovery.png' });
  await page.getByRole('button', { name: 'Retry connection' }).click();
  await page.waitForURL('https://auth.shaedy.de/login?**');
  assert.equal(firstNavigation, root);
  results.push('Release executable navigates to the exact production URL and follows auth redirects');
  assert.equal(new URL(page.url()).searchParams.get('return'), deepLink);
  const denied = await page.evaluate(async () => {
    try { await window.__TAURI_INTERNALS__.invoke('connect'); return false; } catch { return true; }
  });
  assert.equal(denied, true, 'remote auth cannot invoke native recovery');
  await page.locator('#fixture-login').click();
  await page.waitForURL(deepLink);
  assert.equal(fixtureSessionObserved, true);
  results.push('Validated return path, query and fragment survive the fixture login round trip');
  const gameDenied = await page.evaluate(async () => {
    try { await window.__TAURI_INTERNALS__.invoke('connect'); return false; } catch { return true; }
  });
  assert.equal(gameDenied, true);
  const nativeDenied = await page.evaluate(async () => {
    const commands = ['desktop_status', 'check_update', 'download_update', 'install_update', 'desktop_action'];
    return Promise.all(commands.map(async command => {
      try { await window.__TAURI_INTERNALS__.invoke(command, { action: 'leave_game' }); return false; } catch { return true; }
    }));
  });
  assert.ok(nativeDenied.every(Boolean), 'hosted game cannot invoke any desktop or update command');
  results.push('Remote game and auth pages cannot invoke the local native command');
  const windowInfo = JSON.parse(native('inspect'));
  assert.equal(windowInfo.menu, false, 'extra native menu strip is removed');
  assert.equal(windowInfo.titlebar, true, 'native titlebar is preserved');
  assert.equal(windowInfo.systemMenu, true, 'normal Windows system controls remain');
  metrics.nativeWindow = windowInfo;
  await page.getByRole('heading', { name: 'CaseSim fixture', exact: true }).click();
  native('controls');
  let controls;
  for (let attempt = 0; attempt < 30; attempt++) {
    controls = browser.contexts().flatMap(candidate => candidate.pages()).find(candidate => candidate.url().endsWith('/settings.html'));
    if (controls) break;
    await setTimeout(300);
  }
  assert.ok(controls, 'hidden-menu keyboard accelerator opens native Desktop controls');
  await controls.getByRole('heading', { name: 'Desktop controls', exact: true }).waitFor();
  await controls.locator('#version').filter({ hasText: `Desktop ${desktopVersion}` }).waitFor();
  await controls.screenshot({ path: 'artifacts/desktop/desktop-controls.png' });
  await controls.getByRole('button', { name: 'Account and sign out' }).click();
  await page.waitForURL('https://auth.shaedy.de/?**');
  await controls.getByRole('button', { name: 'Return to game' }).click();
  await page.waitForURL(root);
  await controls.getByRole('button', { name: 'Reset zoom' }).click();
  await controls.getByRole('button', { name: 'Fullscreen' }).click();
  await setTimeout(500);
  await controls.getByRole('button', { name: 'Fullscreen' }).click();
  await Promise.all([controls.waitForEvent('close'), controls.getByRole('button', { name: 'Close controls' }).click()]);
  results.push('Native titlebar, hidden menu accelerators, local Desktop controls, fullscreen and zoom reset verified');
  await page.getByRole('link', { name: 'Museum fixture' }).click();
  await page.waitForURL(`${root}museum`);
  await page.locator('#popup').click();
  await page.waitForURL(`${root}quests`);
  assert.equal(context.pages().length, 1);
  results.push('Internal links and trusted popups stay in a single main window');
  const webgl = await page.evaluate(() => Boolean(document.getElementById('gpu').getContext('webgl2')));
  assert.equal(webgl, true, 'WebGL2 remains available on the runner');
  results.push(`Fixture WebGL2 context available: ${webgl} (runner graphics only)`);
  await page.screenshot({ path: 'artifacts/desktop/online-fixture.png' });
  const secondary = spawn(executable, [], { windowsHide: true, stdio: 'ignore' });
  const secondaryCode = await Promise.race([
    new Promise(resolve => { secondary.once('exit', resolve); secondary.once('error', () => resolve(-1)); }),
    setTimeout(10000, -1),
  ]);
  if (secondaryCode !== 0 && secondary.exitCode === null) secondary.kill();
  assert.equal(secondaryCode, 0, 'second launch focuses the running instance and exits');
  assert.equal(app.exitCode, null);
  assert.equal(context.pages().length, 1);
  results.push('A second executable launch keeps the existing session and a single game window');
  await stop();
  await start();
  await page.getByRole('button', { name: 'Retry connection' }).click();
  await page.getByRole('heading', { name: 'CaseSim fixture', exact: true }).waitFor();
  assert.equal(fixtureSessionObserved, true);
  results.push('Synthetic Secure HttpOnly domain cookie persists after application restart');
  for (const status of [401, 403]) {
    accessFailure = status;
    await page.goto(`${root}restricted`).catch(() => {});
    await page.getByRole('heading', { name: 'Fixture access denied', exact: true }).waitFor();
    await setTimeout(500);
    assert.equal(new URL(page.url()).hostname, 'shaedy.de');
  }
  accessFailure = 0;
  await page.goto(root);
  results.push('HTTP 401 and 403 remain auth/access responses instead of becoming offline screens');
  serverFailure = true;
  await page.reload().catch(() => {});
  await page.getByRole('heading', { name: 'Connection unavailable' }).waitFor();
  serverFailure = false;
  await page.getByRole('button', { name: 'Retry connection' }).click();
  await page.getByRole('heading', { name: 'CaseSim fixture', exact: true }).waitFor();
  results.push('HTTP 503 shows bundled recovery; Retry restores the game without restart');
  offline = true;
  await page.reload().catch(() => {});
  await page.getByRole('heading', { name: 'Connection unavailable' }).waitFor();
  offline = false;
  await page.getByRole('button', { name: 'Retry connection' }).click();
  await page.getByRole('heading', { name: 'CaseSim fixture', exact: true }).waitFor();
  results.push('Navigation network failure recovers through Retry without restart');
  await stop();
  Object.assign(metrics.installer, JSON.parse(native('uninstall')));
  results.push('NSIS silently uninstalls the tested application on the isolated runner');
  console.log(results.join('\n'));
} catch (error) {
  try { if (app?.pid) native('diagnostics'); } catch { /* Preserve the original smoke failure. */ }
  await appendFile('artifacts/desktop/runtime.log', `Visible test targets: ${JSON.stringify(browser?.contexts().flatMap(candidate => candidate.pages()).map(candidate => { const u = new URL(candidate.url()); return u.origin + u.pathname; }))}\n`);
  if (page && !page.isClosed()) {
    await page.screenshot({ path: 'artifacts/desktop/smoke-failure.png' }).catch(() => {});
  }
  throw error;
} finally {
  await writeFile('artifacts/desktop/smoke-results.json', JSON.stringify({ results }, null, 2));
  await writeFile('artifacts/desktop/runtime-metrics.json', JSON.stringify(metrics, null, 2));
  await stop();
}
