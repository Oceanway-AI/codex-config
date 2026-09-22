# Direct Image API Beta Acceptance

Version: 1.4.0-beta.3. Branch: `codex/direct-image-api`.

This branch is for local user testing. It must not be merged or published as a production update before desktop and service acceptance.

## Automated Checks

- `npm test`: frontend orchestration, state transitions, simulation boundaries.
- With `npm run test:ui` serving the loopback fixture, run `node tests/simple-ui-smoke.mjs` and `node tests/image-ui-smoke.mjs`. Set `PLAYWRIGHT_MODULE` to a local Playwright `index.mjs` when needed. These tests use fake data only. They cover compact home/advanced navigation, layout from 320 to 1120 pixels, locking relocated actions, restart rejection/retry, migration confirmation, restore/readback failures and the existing image workflow.
- `cargo test`: temporary-home configuration, preservation, rollback, image mock server, queue/partial failure/cancellation.
- GitHub Actions: Windows and both macOS architectures; no local Rust/MSVC required by end users.
- Download artifacts only from the run whose head SHA matches the test branch commit.
- Optional `node tests/probe-codex-config.mjs <absolute-codex-executable> <auth-mode>` runs against a loopback fake Responses server with an isolated temporary `CODEX_HOME`. Modes are `apiKey` (default), `providerToken`, and `missingAuth` (negative control). It asserts the actual Bearer header, user-level rules in a developer message, and a CLI reference image with both input bytes and the original local path. This is not desktop upload/paste/drop or natural-language-routing acceptance.

## Isolated Live Verification

Explicitly authorized tests on September 22, 2026 used the installed beta with a separate `CODEX_HOME`, workspace, process and WebView2 profile. The normal Codex configuration/authentication were fingerprinted without logging their contents; no desktop restart was invoked.

- `gpt-image-2`: native generation 1/1, single-reference edit 1/1, and two-reference edits 2/2 returned real PNG files with request IDs and no response warnings.
- A fresh API-key setup exposed a missing Authorization header on `/responses`. Beta 2 sets `requires_openai_auth = true` for the `auth.json` strategy. Loopback runtime probes reproduce the old failure and verify both supported authentication strategies.
- At test time, `gpt-5.4` returned an upstream 502 even on a minimal authenticated request; `gpt-5.6-sol` answered successfully. Model visibility alone does not establish service health.
- `tests/isolated-live-session.mjs` is an opt-in harness for the actual installed application's native commands. Credentials enter via its private stdin channel, not arguments or source files. It deliberately calls configuration commands without the UI's restart action.
- `tests/isolated-agent-session.mjs` starts a separate Codex app-server, not the currently running desktop. Its evidence must not be presented as desktop upload/paste/drop acceptance.
- When the launch environment explicitly sets `CODEX_HOME`, automatic desktop restart is refused. `HOME`/`USERPROFILE` can also be overridden and do not prove the running desktop's home. Configuration remains saved; open the intended host manually. Normal launches without `CODEX_HOME` retain automatic restart.
- Initial natural-language testing attempted an imagegen skill read and guessed the normal configuration path. The approval gate cancelled that request before execution. The revised managed rules require runtime-home discovery before any configuration read and explicitly prohibit raw configuration/credential dumps.
- Reports, test credentials, session logs and output images belong under ignored `dist/isolated-tests/`, never in Git. Stop private helper processes after testing.

## Explicit User Checks

Configuration alone must not POST to image endpoints. The user supplies and saves their own Base URL and Key in the real desktop application.

1. Confirm existing developer instructions remain intact; configure twice and confirm one managed rules block.
2. Change Key, save again, verify no stale credential is used. Restore and verify the initial backup.
3. Inspect model visibility, then explicitly test generation. Test count greater than one and single/multiple reference files. Confirm files open, result count and request IDs.
4. Save active Codex tasks before restarting. Create a new Codex task and request "生成六张不同风格的海报".
5. Upload a reference in Codex: "参考这张图做三个版本".
6. Upload two references: "结合两张参考图生成四张".
7. Request "把第二张改成横版", verify the second actual output is the reference and originals survive.
8. Accept one representative desktop attachment-to-edit workflow, confirming the real original image is passed to the HTTP endpoint. Codex's existing upload, drag/drop and paste controls do not each need a separate regression test for this release.
9. "分析图片" and "只写提示词" must not submit image-generation requests.
10. Cancel during a queue, preserve successes, then explicitly retry missing slots. A timeout or cancellation may still incur provider charges.

## Honest Boundaries

- Instructions guide the model, not a deterministic desktop routing hook. Higher-priority host instructions, project/profile overrides, skills and runtime attachment access may affect behavior.
- No provider batching maximum has been established by this beta. The built-in test runner uses `n=1` requests and two workers, never a one-image total limit.
- Model listing proves visibility only, not generation/edit entitlement.
- The configuration app's native reference picker tests the HTTP interface; it does not prove Codex conversation attachments are accessible.
- Paid generation, edits, multi-reference behavior and natural-language acceptance are user-triggered tests, not automatic installer actions.
- Keep the configuration application open while a test is running. Results and a credential-free progress manifest are local files; this beta does not automatically resume paid jobs after restarting the application.
- No mask editor or separate task-management service is included.

## Beta 3 Interface

- The default window is 640 x 620 logical pixels. Home contains Base URL, Key, one-click configuration, history migration, restore and the advanced entry.
- Progress appears only during configuration or when intervention is required. Diagnostics and logs do not open automatically.
- Image rules are still installed by the same one-click workflow; no model or image setup step was added.
- Advanced contains local status, connection tests, diagnostics, image tests, logs, repair, restart, directories and updates. Paid tests still require explicit confirmation.
- Restart failure is labelled "saved, pending restart", not a failed save. Isolated-home restart protection remains unchanged.
- Restore readback failure is not displayed as verified success. The existing restore snapshot and migration algorithms are unchanged.
- No image rules, image transport, credential strategy or generation backend changed in this interface revision.
