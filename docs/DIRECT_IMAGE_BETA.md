# Direct Image API Beta Acceptance

Version: 1.4.0-beta.1. Branch: `codex/direct-image-api`.

This branch is for local user testing. It must not be merged or published as a production update before desktop and service acceptance.

## Automated Checks

- `npm test`: frontend orchestration, state transitions, simulation boundaries.
- `cargo test`: temporary-home configuration, preservation, rollback, image mock server, queue/partial failure/cancellation.
- GitHub Actions: Windows and both macOS architectures; no local Rust/MSVC required by end users.
- Download artifacts only from the run whose head SHA matches the test branch commit.
- Optional `node tests/probe-codex-config.mjs <absolute-codex-executable>` runs against a loopback fake Responses server with an isolated temporary `CODEX_HOME`. On this workstation's `codex-cli 0.155.0-alpha.9`, it verified user-level rules in a developer message and a CLI reference image with both input bytes and a local path. This is not desktop upload/paste/drop or natural-language-routing acceptance.

## Explicit User Checks

Configuration alone must not POST to image endpoints. The user supplies and saves their own Base URL and Key in the real desktop application.

1. Confirm existing developer instructions remain intact; configure twice and confirm one managed rules block.
2. Change Key, save again, verify no stale credential is used. Restore and verify the initial backup.
3. Inspect model visibility, then explicitly test generation. Test count greater than one and single/multiple reference files. Confirm files open, result count and request IDs.
4. Save active Codex tasks before restarting. Create a new Codex task and request "生成六张不同风格的海报".
5. Upload a reference in Codex: "参考这张图做三个版本".
6. Upload two references: "结合两张参考图生成四张".
7. Request "把第二张改成横版", verify the second actual output is the reference and originals survive.
8. Repeat attachment tests using file upload, drag/drop and clipboard paste. Merely seeing an image is not proof the original bytes were available to HTTP tools.
9. "分析图片" and "只写提示词" must not submit image-generation requests.
10. Cancel during a queue, preserve successes, then explicitly retry missing slots. A timeout or cancellation may still incur provider charges.

## Honest Boundaries

- Instructions guide the model, not a deterministic desktop routing hook. Higher-priority host instructions, project/profile overrides, skills and runtime attachment access may affect behavior.
- No provider batching maximum has been established by this beta. The built-in test runner uses `n=1` requests and two workers, never a one-image total limit.
- Model listing proves visibility only, not generation/edit entitlement.
- The configuration app's native reference picker tests the HTTP interface; it does not prove Codex conversation attachments are accessible.
- Paid generation, edits, multi-reference behavior and natural-language acceptance are user-triggered tests, not automatic installer actions.
- No mask editor or separate task-management service is included.
