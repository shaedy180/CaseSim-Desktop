/* Only this bundled document/window receives the narrow desktop capability. */
const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
const byId = id => document.getElementById(id);
let pending = false;
async function refresh() {
  const s = await invoke('desktop_status');
  byId('version').textContent = `Desktop ${s.version} · Web version follows the online game`;
  const labels = { checking: 'Checking the signed update channel…', available: `Desktop ${s.availableVersion} is available.`, downloading: `Downloading: ${(s.downloaded / 1048576).toFixed(1)} MB`, ready: `Desktop ${s.availableVersion} is verified and ready.`, current: 'Your desktop is up to date.', installing: 'Installing the verified update…', error: s.error };
  byId('status').textContent = s.configured ? (labels[s.phase] || 'Check for a new desktop version when you are ready.') : 'Updates are not configured in this preview.';
  const busy = pending || ['checking', 'downloading', 'installing'].includes(s.phase);
  byId('check').disabled = busy || !s.configured;
  byId('download').hidden = s.phase !== 'available';
  byId('download').disabled = busy;
  byId('install').hidden = s.phase !== 'ready';
  byId('install').disabled = busy || s.gameRunning;
  byId('safety').textContent = s.gameRunning ? 'The game is still open. Finish all openings and battles and stop Auto Open before leaving. Installation stays locked.' : 'The game is paused in the local desktop screen. Installation requires another confirmation.';
  const progress = byId('progress');
  progress.hidden = s.phase !== 'downloading';
  if (s.total) { progress.max = s.total; progress.value = s.downloaded; } else progress.removeAttribute('value');
  document.querySelectorAll('[data-action]').forEach(button => { button.disabled = busy; });
}
async function run(command, args) {
  if (pending) return;
  pending = true; byId('error').textContent = '';
  try { await refresh(); await invoke(command, args); }
  catch (error) { byId('error').textContent = String(error); }
  finally { pending = false; await refresh().catch(() => {}); }
}
byId('check').addEventListener('click', () => void run('check_update'));
byId('download').addEventListener('click', () => void run('download_update'));
byId('install').addEventListener('click', () => void run('install_update'));
document.querySelectorAll('[data-action]').forEach(button => button.addEventListener('click', () => void run('desktop_action', { action: button.dataset.action })));
void refresh().catch(() => { byId('error').textContent = 'Native desktop controls are unavailable.'; });
setInterval(() => { if (!document.hidden) void refresh().catch(() => {}); }, 750);
