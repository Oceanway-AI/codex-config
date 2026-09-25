# OceanWay Config Image MCP Acceptance

Date: 2026-09-25 UTC. Version: `1.4.0-beta.5`.
Status: core image workflow passed in one isolated Windows desktop; remaining
release gates below are open. Not approved for general release.

## Scope

One-click provider/auth setup, native image MCP, natural-language image
generation, ordered original references and arbitrary user-requested counts.
The primary form contains only Base URL, Key and configuration actions.
Existing history migration behavior is unchanged.

## Build Identity

- Branch: `codex/sync-image-api-fix`.
- Tested application commit: `139ceb859f17d1ad13a65a30552cd401a81e99c0`.
- Final cloud build: `36157558449`; Windows x64, macOS ARM64 and macOS Intel
  build and automated-test jobs passed.
- Windows EXE: `dist/build-139ceb8/codex-config.exe`, 22,164,480 bytes.
  SHA-256: `0F2DCBDA04168C59C53BB7CD0B24237AED4BEE852E0564105700F8B76D21A328`.
- Windows installer:
  `dist/build-139ceb8/bundle/nsis/codex-config_1.4.0-beta.5_x64-setup.exe`,
  5,732,380 bytes.
  SHA-256: `B52C70D7DE54FDF6E537470FA9640D770464609CE3BF55FF2B8638DFF7884CAC`.
- The portable EXE is running in the isolated fixture. The installer was
  downloaded and hashed, not executed.
- Subsequent evidence/test-only commits do not change the tested executable.
- No local Rust or Visual C++ toolchain was installed.
- No main merge, release tag, stable release or updater publication.

## Isolation

- API fixture: `dist/acceptance-beta5-api-20260925`.
- Official-login fixture: `dist/acceptance-beta5-20260925`.
- Separate `CODEX_HOME`, desktop profile, workspace and temporary directory.
- The initially fresh API fixture was configured through the previous
  candidate's UI; final commit `139ceb8` then exercised the upgrade/reconfigure
  flow through its real form. Neither run manually registered MCP or added
  routing rules outside the application.
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
| Frontend tests | 38 passed locally and in all three cloud jobs |
| Rust engine/config tests | Windows 152 passed; both macOS targets 155 passed |
| Packaged MCP handshake | Final Windows and ARM64 initialize/list/EOF passed; Intel cross-build only |
| UI-only configuration | Final EXE saved/read back/handshook/restarted the isolated API fixture through the real form |
| Fresh configuration using final EXE | Final UI check was an upgrade/reconfigure; fresh UI setup belongs to the previous candidate |
| Official login migration | Pending user login in isolated window |
| Reconfigure, restore, credential update, invalid files | Automated tests; final UI checks pending |
| Six distinct posters | 6/6 real outputs saved and displayed through OceanWay Images MCP |
| One reference, three versions | 3/3 real edits saved and displayed; original file SHA-256 matched |
| Two references, four versions | 4/4 real edits saved and displayed; two dragged original attachments and their order/hashes verified |
| Modify second output to landscape | 1/1 real edit saved and displayed; actual dimensions 1536x1024 and previous second output hash verified |
| Analysis-only and prompt-only negative intents | Both returned text only; no new image job |
| Unavailable model, no substitution/retry | One specified-model request failed with HTTP 400; desktop reported failure, no replacement or retry |
| Partial failure, timeout, cancellation, recovery | Rust mock coverage plus 8/8 packaged stdio scenarios; desktop fault checks pending |
| Production protection | Matched baseline at latest check |

The approved real-image first-pass matrix requests 14 outputs. Request counts
are not proof of supplier billing. Paid failures are not automatically retried.
Real supplier calls use the user's isolated test credentials, never production
login tokens. Credentials and ownership tokens are excluded from this report.

## Real Desktop Evidence

- Host package: `OpenAI.Codex_26.917.9434.0_x64__2p2nqsd0c76g0`.
- Text model: `gpt-5.6-sol`, selected before creating the test task.
- Image model: `gpt-image-2`; each image request used `n=1`.
- Task: `01a0d952-c07b-7b21-ae6a-721e18081863`.
- Six distinct poster prompts:
  `image-54912-1790352683475816600-0`, six successful 1024x1024 outputs.
- Three variants using the first real output:
  `image-54912-1790353452047977000-22`, edit mode, three successful
  1024x1024 outputs. Reference SHA-256:
  `ed4cd5029bafcca3e35dfa3e0644eb10688b6d89bfb141f1a552640a733fdf28`.
- Two-reference request:
  `image-54912-1790354096481804300-35`. Actual file drag events put the first
  two poster files into the desktop composer; both removable attachments were
  visible before sending. MCP reference order matches those files. The second
  reference SHA-256 is
  `ca5b7a2956b880ab11439db5c6da73505016e48720961ed3beb608bc8fdcc12f`.
- Landscape edit of the previous second output:
  `image-54912-1790354540712897900-51`, one successful 1536x1024 image.
  Reference SHA-256:
  `2f2055f5b2bd4e688554f80de627512359556982dadb95588aa069ff086fdd0e`.
  Output SHA-256:
  `53888b17c9d0b61dae06661f0d9ace9045c0aeb7347b2414b8c85278dfc11b4e`.
- All 14 outputs have distinct hashes; file bytes, PNG dimensions and original
  reference hashes were independently checked after generation. Sanitized
  per-image evidence: `tests/results/desktop-images-139ceb8.json`.
- Actual image files and individual request IDs, dimensions, timings and hashes
  are preserved in each job's `manifest.json`. No fixture image is counted here.
- Evidence screenshots under `dist/acceptance-beta5-api-20260925`:
  `image-only-native-config-success.png`, `real-six-output-desktop.png`,
  `real-three-edits-and-two-attachments.png`,
  `two-original-attachments-before-send.png`,
  `real-four-multi-reference-output.png`, `real-landscape-output.png`,
  `negative-analysis-and-prompt-only.png`, `negative-unavailable-model.png`.
- The three-variant turn experienced text-service reconnects after the images
  had completed. The saved images were retained and the image job was not
  submitted again. The desktop eventually displayed all three.
- File drag and use of previous real output are exercised here. Clipboard
  paste and the native file picker are not claimed as separately accepted.
- Analysis-only and prompt-writing-only requests each produced a text answer
  without creating a new image job.
- Specifying `oceanway-image-does-not-exist-acceptance` produced one failed job:
  `image-54912-1790355218870955000-58`. The service returned HTTP 400 with request
  ID `72694f34-8336-4184-949b-19ef4360214f`. The desktop reported the error and
  neither substituted another model nor retried.
- Session tool records contain five generation calls: the four successful
  matrix jobs and the negative test. They contain zero imagegen calls and
  zero retry calls. All four successful turns displayed their saved images.
- Final formal config/auth/instruction fingerprints still match the baseline.
  Formal desktop PID 3000 and backends 58880/22784 retain their original
  process identities; only the isolated desktop was restarted.

Local HTTP failure fixtures accept only the reserved dummy credential and
numeric loopback addresses. Real-provider HTTPS rules remain in place.
Fixture images are mock results and never count toward the real-image matrix.

## Packaged Failure and Recovery Checks

`node tests/mcp-bundle-images.mjs --executable dist/build-139ceb8/codex-config.exe`
passed all eight scenarios, both independently and after integration. It uses
a fresh temporary home, a numeric-loopback service and a reserved dummy key.
It covers seven distinct prompts, ordered A/B/A original references, partial
failure and explicit retry, two-worker scheduling, cancellation, EOF draining,
cross-process ownership, interrupted-job recovery and anomalous extra outputs.
Each run made 30 mock image POSTs and zero upstream fetches. Recovery itself
sent no automatic POSTs; successful output files were retained. Owned processes
and temporary files were cleaned up.

See `tests/PACKAGED_IMAGE_ACCEPTANCE.md` and
`tests/results/mcp-bundle-images-139ceb8.json`. This does not replace real desktop
failure handling, official-account migration or a full 240-second timeout run.

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
