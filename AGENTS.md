# CaseSim Desktop agent instructions

This is the canonical native Windows shell repository. Read this file and the sole
manual [README.md](README.md) before changes. The owner explicitly authorized native
source extraction/publication on 2026-10-19. Initial empty-repository bootstrap may
create a README-only main commit once; all implementation follows the workflow below.
The hosted web/API/economy/auth application and signing/deployment secrets stay private.
Never import private application code or its Git history into this repository.

Use protected main and permanent working-branch only. Fetch first, preserve unrelated
work, continue any existing working-branch PR to main, and fast-forward from main
before coding. Commit/push only working-branch, PR only into main. Never force-push,
push implementation to main, create other branches, bypass checks/reviews, or delete
either permanent branch. Disable automatic branch deletion and dependency bots that
create additional branches. Keep checks strict and enforced for administrators.

Open a draft PR early. Wait for complete Linux and Windows validation and CI provenance
on its latest exact head. Failed tests, skipped required jobs, stale/missing receipts
block merging. Use a merge commit only after checks/reviews pass. Main must verify
the latest unexpired PR receipt, identical tested tree and both merge parents; do not
repeat the full native build after merge. Then fast-forward/push working-branch to
merged main and verify both remote branches, protection and main provenance.

Each native implementation commit increments patch exactly once in package.json,
both lockfile roots, Tauri configuration, Cargo.toml/lock and desktop/release-notes.json.
Keep newest English notes concise, user-facing, without emojis/em dashes or internal
file/CI details. Merge synchronization and test-only changes need no product release.
Run npm run desktop:check and relevant tests. Never weaken a gate to publish.

Preserve native runtime behavior, identifier/profile storage, public navigation/auth
contracts, no remote IPC, exact local URL/window authorization, strict CSP, HTTPS and
version-bound updater signature checks. Keep the verification key and update origin.
Do not copy credentials/cookies, expose native APIs remotely, alter game/economy/auth,
or add background gameplay/replay. Use solid dark surfaces, compact angular controls,
neon green actions, existing font/colors and accessible focus/reduced motion. No redesign
or unsupported performance claims during extraction. Keep installer/font/dependency
attribution. Record consumers before removals; preserve useful security regressions.

Public CI builds only this public project, without secrets or private checkouts.
Preview installers are unsigned. Official private release preparation verifies exact
Desktop main CI plus approved CaseSim compatibility identity, signs and retests the
installed package, then stages it. desktop-releases retains separate protected human
approval and promote-only capability. Never approve on the owner's behalf. Never
overwrite releases or downgrade the channel. No hosted-game deployment is authorized.

Keep all general docs in root README and instructions here. Store logs/screenshots,
generated output and audit evidence in ignored artifacts/tmp. Never commit credentials,
signing keys, certificates, profiles, private data, generated binaries or target output.
Inspect status and full staged name/status/stat/diff before each commit; stage explicit
intentional paths only, never git add . or -A. Completion requires passing checks,
merged PR, verified main provenance, synchronized two branches and accurate limitations.
