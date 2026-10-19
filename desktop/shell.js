/* Only bundled content has the allow-connect capability. No remote-page injection. */
const retry = document.getElementById('retry');
const heading = document.getElementById('heading');
const message = document.getElementById('message');
const failed = new URLSearchParams(location.search).has('failed');
const paused = new URLSearchParams(location.search).has('paused');
if (paused) {
  heading.textContent = 'Game paused for desktop maintenance';
  message.textContent = 'Return to the game when you are ready. Desktop updates require your confirmation.';
  retry.textContent = 'Return to game';
}
if (failed) {
  document.body.classList.add('failed');
  heading.textContent = 'Connection unavailable';
  message.textContent = 'CaseSim requires an internet connection. Check your connection or try again shortly if the server is unavailable.';
}
async function connect() {
  retry.disabled = true;
  document.body.classList.remove('failed');
  heading.textContent = 'Connecting to CaseSim';
  message.textContent = 'Opening your online experience. Slow connections may take a moment.';
  try {
    await window.__TAURI_INTERNALS__.invoke('connect');
  } catch {
    heading.textContent = 'Unable to connect';
    message.textContent = 'The connection could not be started. Please try again.';
    document.body.classList.add('failed');
  } finally {
    retry.disabled = false;
  }
}
retry.addEventListener('click', connect);
window.addEventListener('online', () => { if (!paused && !retry.disabled) void connect(); });
setTimeout(() => {
  if (!failed && !paused) message.textContent = 'Still connecting. You can retry without restarting CaseSim.';
}, 15000);
if (!failed && !paused) void connect();
