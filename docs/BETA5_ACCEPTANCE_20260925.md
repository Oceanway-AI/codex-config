# OceanWay Config Image MCP Acceptance

Date: 2026-09-25. Version: `1.4.0-beta.5`.
Status: in progress; not approved for general release.

## Scope

One-click provider/auth setup, native image MCP, natural-language image
generation, ordered original references and arbitrary user-requested counts.
The primary form contains only Base URL, Key and configuration actions.
Existing history migration behavior is unchanged.

## Build Identity

- Branch: `codex/sync-image-api-fix`.
- Previous downloaded candidate:
  `081bad695c775949a1f6d07f22b2731759adf48b`.
- Previous cloud build: `36150166377`; Windows and both macOS jobs passed.
- Source has changed since that candidate. Final build and desktop checks must
  use the updated artifact, not an earlier prototype.
- No local Rust or Visual C++ toolchain was installed.
- No main merge, release tag, stable release or updater publication.

## Isolation

- API fixture: `dist/acceptance-beta5-api-20260925`.
- Official-login fixture: `dist/acceptance-beta5-20260925`.
- Separate `CODEX_HOME`, desktop profile, workspace and temporary directory.
- Before final application configuration, no provider, MCP or routing rules
  were manually installed in the fresh API fixture.
- Formal config/auth/instruction fingerprints are recorded in
  `protected-config-before.json`. Recheck them after desktop operations.
- Restart targets exact PID, creation time, executable and private profile.
- Host package: `OpenAI.Codex_26.917.9434.0_x64__2p2nqsd0c76g0`.
- Embedded application version: `26.917.71314`.
- Official-login verification requires user login inside its isolated window;
  production authentication is never copied.

## Required Gates

| Gate | Status |
| --- | --- |
| Frontend tests | 37 passed locally; rerun after further changes |
| Rust engine/config tests | Previous Windows candidate 166 passed; updated build pending |
| Packaged MCP handshake | Previous Windows/ARM64 candidate passed; updated artifact pending |
| UI-only configuration | Previous API candidate saved/read back/handshook/restarted; retest final artifact |
| Official login migration | Pending user login in isolated window |
| Reconfigure, restore, credential update, invalid files | Automated tests; final UI checks pending |
| Six distinct posters | Pending final-artifact real desktop test |
| One reference, three versions | Pending final-artifact real desktop test |
| Two references, four versions | Pending final-artifact real desktop test |
| Modify second output to landscape | Pending final-artifact real desktop test |
| Analysis-only and prompt-only negative intents | Pending desktop check |
| Unavailable model, no substitution/retry | Pending desktop check |
| Partial failure, timeout, cancellation, recovery | Mock coverage; desktop checks pending |
| Production protection | Matched baseline at latest check |

The approved real-image first-pass matrix requests 14 outputs. Request counts
are not proof of supplier billing. Paid failures are not automatically retried.
No real image request has yet been submitted in this implementation acceptance.

Local HTTP failure fixtures accept only the reserved dummy credential and
numeric loopback addresses. Real-provider HTTPS rules remain in place.
Fixture images are mock results and never count toward the real-image matrix.

## Recovery

- Restore configuration removes owned MCP registration and routing rules.
- Keep unrelated MCPs, user instructions, desktop preferences, sessions and images.
- Preserve established provider/auth snapshot and history migration semantics.
- A timeout or interrupted process never automatically resubmits image work.
- Earlier prototype evidence is not final packaged-application acceptance.

## Previous Download Evidence

- EXE: `dist/build-081bad6/codex-config.exe`, 22,272,000 bytes.
- EXE SHA-256:
  `6090C0D386105B717A8FE446C49C259CEE69795D39AAC3F28116E6BE379A7A6A`.
- Installer: `dist/build-081bad6/bundle/nsis/codex-config_1.4.0-beta.5_x64-setup.exe`,
  5,722,994 bytes, not executed.
- Installer SHA-256:
  `6832124696482B7E628B8D275F12E99B757A584D9BA2F690734AECF83D98A3A8`.
- The real application form configured the API fixture; screenshot:
  `dist/acceptance-beta5-api-20260925/native-config-success.png`.
- Desktop process/window readiness preceded full interactive startup.
  Do not treat a successful restart as successful natural-language generation.
