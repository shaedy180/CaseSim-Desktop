# CaseSim Desktop

Public source for the native Windows Tauri/WebView2 shell of the hosted
[CaseSim](https://shaedy.de/casesim/) game. The web application, API, databases and
authentication service remain private. Rules are in [AGENTS.md](AGENTS.md).

Only Node 24, pinned Tauri CLI and Playwright are needed from npm. Rust dependencies
are locked separately. This source publication does not grant a new license to
CaseSim code, name or artwork. Preserve third-party licenses; JetBrains Mono uses
the bundled OFL and the installer includes generated locked dependency notices.


The Windows 10/11 x64 shell uses Tauri 2 and Evergreen WebView2, loading the hosted
`https://shaedy.de/casesim/` game and central shaedy-auth unchanged. Only the small
startup/recovery and Desktop controls pages are bundled. Web updates never require
rebuilding the desktop app. Independent Desktop SemVer lives in
`src-tauri/tauri.conf.json`, Cargo metadata and `desktop/release-notes.json`;
web/API release metadata remains separate.

The normal Windows title bar, system menu and window controls remain. The extra
CaseSim menu strip is removed on Windows; WebView2 handles native keyboard input
without granting hosted pages any new command access:

| Shortcut | Action |
| --- | --- |
| Ctrl+Shift+D | Open local Desktop controls, version and manual update check |
| Ctrl+Shift+A | Open the real shaedy-auth account/session/password/sign-out page |
| Ctrl+Shift+H | Return to CaseSim |
| F11 | Toggle fullscreen |
| Ctrl+0 | Reset game zoom |

Desktop controls also expose these actions as buttons. Saved window position,
logical size and maximized state, automatic WebView2 DPI handling and focusing an
existing instance are retained. Fullscreen is temporary. WebView2 keeps its default
hardware acceleration; no GPU/Chromium switches, synthetic presence, game timings,
PixiJS settings or economy rules are changed. The native release profile strips
symbols and uses thin LTO; runtime/installer measurements belong in CI artifacts,
not unmeasured performance claims.

Navigation stays limited to HTTPS `shaedy.de/casesim` and its descendants, and the
explicit `auth.shaedy.de` account/login/privacy/auth routes. Query/fragment return
paths survive; unrelated HTTP(S) links use the default browser, executable schemes
and credential-bearing URLs are blocked, and trusted popups reuse the game window.
The policy does not interfere with image/API/WebSocket/SSE requests or the existing
sandboxed 3D viewer. Steam linking across the external browser profile remains
unsupported; Steam is not required for username/password login.

The ordinary Secure/HttpOnly auth cookie stays in
`%LOCALAPPDATA%/de.shaedy.casesim.desktop/WebView2`; the shell never reads or copies
credentials/cookies. Remote game and auth documents have **no native grants**.
The recovery command only retries the last trusted in-memory URL. The separate
`desktop` window loads only bundled `settings.html`; both its exact local URL and
window label are checked by every desktop command in addition to Tauri ACLs.
Neither local capability grants the updater/dialog plugin JavaScript APIs. CSP
blocks local pages from making network requests or framing hosted content.

Top-level DNS/TLS/network failures and HTTP 5xx use local Retry; auth HTTP 401/403
remain service responses. Renderer failure uses Retry rather than replaying a
page automatically. WebView2 owns GPU recovery; a browser-process crash offers a
native, explicitly confirmed restart. The application's normal API/auth/realtime
recovery remains responsible for failures inside a loaded game. Local diagnostics
contain timestamped event codes only, rotate at 256 KiB with three backups, and live
in Tauri's per-user app log directory (Windows local app data for this identifier).
No URLs, arguments, credentials or session data are logged.

#### Native updates and restart protection

The official Tauri 2 updater runs in Rust. Checks are manual, serialized with
download/installation, use HTTPS with redirects rejected, an embedded public key,
version-bound Ed25519 signatures and the default strictly-newer version comparator.
The package source must match an explicitly allowed HTTPS origin. Download progress
and clear connection/signature/install errors appear in local Desktop controls.
Downloaded bytes are verified before installation and verified again by the official
installer path. Downloaded packages exist only in memory until that path executes.

Installation is locked whenever the hosted game/auth document is open. Finish
openings/battles and stop Auto Open, then explicitly choose **Leave game for
maintenance** and confirm. This replaces the game with a local paused screen that
never reconnects automatically. Installation requires a second native confirmation;
return navigation is blocked while installation starts. Cancelling retains the
verified download. There is no remote activity bridge and no claim to inspect
server-side in-flight operations: users must finish them before confirming exit.
Window state is saved before the installer runs and controlled restart begins.

`src-tauri/update-channel.json` pins the public verification key, manifest
`https://releases.shaedy.de/casesim/windows/latest.json` and the sole download origin
`https://releases.shaedy.de`. Never embed private repository credentials.

Every release page, manifest and package requires shaedy-auth. A manual update
check opens the system browser for login and explicit download consent. The Rust
client generates a random PKCE S256 verifier/state and binds an ephemeral
127.0.0.1-only callback before opening the browser. It validates the callback's
method, exact path, Host and unique state/code, rejects oversized/invalid requests
and stops waiting after five minutes. The code is exchanged only at the fixed
HTTPS auth origin; no redirect is followed. Neither browser nor WebView2 cookies
are read, copied or attached to requests. The two profiles can sign in independently.

The narrow bearer grant lasts at most ten minutes, stays only in native memory,
and is sent in a sensitive Authorization header for both the manifest and package.
The token, verifier and code never cross IPC or enter diagnostics. No refresh token
or embedded client secret exists. The release server rechecks the issuing user's
active session on every request: signing out, password changes/resets, expiry or
revocation blocks further downloads. An already admitted stream may finish.
Expired grants require a fresh manual check/login. A verified downloaded package
can still be installed after its download grant expires. Signature verification
and explicit game-exit/install confirmations remain mandatory.

The server uses private/no-store caching. No public feed or static machine bypass
is provided. Redirects are rejected on manifest, download and token exchange so
neither bearer nor verifier can reach a different URL. Direct browser downloads
use that browser's ordinary shaedy-auth session. Until authenticated hosting and
the corresponding auth service are deployed, update checks fail closed.

The owner authorized this standalone public native repository on 2026-10-19.
Only native source, bundled local pages, icons, the licensed font and isolated test
fixtures were extracted as a fresh snapshot. Private web/API code, authentication
implementation, data, deployment configuration, Git history and secrets are excluded.
The original native snapshot is Desktop 22.7.51; this ownership/build change is 22.7.52.

Official releases are rebuilt privately by `shaedy180/CaseSim-V2` only on an explicit
release request. That orchestrator verifies an immutable 40-character Desktop main
commit against this repository's successful main CI provenance, and separately checks
the approved private CaseSim commit and images. It installs this repository's lean
locked dependencies, builds/signs NSIS and repeats the installed Windows tests before
upload-only staging. No public workflow checks out private code or receives signing,
server or private repository credentials. Public preview artifacts cannot be promoted
as official releases.

`build-info.json` identifies the Desktop `commit` and public `source`, approved
`casesim_commit`, private `preparation_commit`, version, installer SHA-256,
version-bound signature and separate Authenticode status. These commit identifiers
are public release metadata. The public `shaedy180/desktop-releases` repository
retains its separate main-only, human-review protected promotion environment. Its
root-owned host controller independently verifies the staged signature, version,
source identities, expected URLs and checksums before atomic publication. Preserve
the existing verification key, immutable versions and monotonic channel. Recover a
faulty public release with a newer signed patch; infrastructure recovery stays with
the private infrastructure owner. A merge or preview does not publish an update or
deploy the hosted game.

Optional Authenticode uses environment secrets `DESKTOP_CERTIFICATE_PFX` (base64 PFX)
and `DESKTOP_CERTIFICATE_PASSWORD`. The certificate is imported only on the protected
release runner, must have the code-signing EKU and a current private key, and signs
EXE/installer with SHA-256 and timestamping. Installer and installed EXE signatures
are checked; temporary
material/certificate is cleaned even after failure. Without a certificate, the
preview and Tauri-signed update channel still work as Windows-unsigned builds.
SmartScreen can warn; signing does not guarantee reputation and no bypass is shipped.

#### Build, installer and validation

Windows prerequisites: Node 24, the pinned Rust toolchain, MSVC x64 tools/Windows SDK.
NSIS installs per user, embeds the WebView2 bootstrapper and installs the runtime if
missing (internet required). Uninstall uses Windows Settings > Apps; it preserves
normal per-user session data and never revokes server sessions. Sign out before
sharing a PC. MSIX requires separate package identity/signing/update-channel work;
ARM64 requires native runner/hardware validation. Neither is added to this x64 release.
Icons derive from the existing CaseSim icon; the bundled font/license and locked
Rust dependency notices remain included in the installer.

```powershell
npm ci
npm run desktop:check
npm run desktop:licenses
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
npm run desktop:build
```

The PR CI workflow validates the independent release/security configuration,
formatting, Rust unit/signature tests and Clippy on Linux, then builds Windows NSIS
and runs the installed WebView2 fixture and silent install/uninstall checks. Production
DNS is blocked before fixtures attach; executable-scoped test debugging is removed
afterward and never ships. Required checks include Linux, Windows and CI provenance.
The PR tests the merge tree; main verifies the unexpired successful receipt, identical
tree and both merge parents without rebuilding. Main verification cannot skip a
failed native test. Manual CI is available for pre-PR diagnostics, not merge bypass.

Public CI builds and installs unsigned previews for isolated validation, but uploads
only build metadata, checksums and synthetic diagnostics (14-day retention). It
does not distribute installers or provide a public download bypass. Official signed
packages are available only through authenticated releases.shaedy.de. Public source
can still be built independently. Desktop 22.7.52 passed the installed controls-window
fixture in both public CI and private signed preparation; it was staged privately
and its publication was cancelled before the authenticated transport change.

Before public release, owner checks still include real
login/logout/revocation/password changes, gameplay/PixiJS/3D/realtime, multiple DPI
scales/monitors, clean-machine WebView2 bootstrap, interactive installation and an
actual signed upgrade/restart on Windows. Synthetic cookies and cryptographic
fixtures cannot certify these real-service/installer checks.
