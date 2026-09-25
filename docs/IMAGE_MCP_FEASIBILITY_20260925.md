# Image MCP Feasibility: 2026-09-25

Status: feasibility prototype; not a release or completed installer upgrade.

## Environment

- Worktree: `D:\AI-Workspace\worktrees\codex-config-sync-image`
- Branch: `codex/sync-image-api-fix`
- Base commit: `1e731ad8da35a48c47deb9704f08373299a7a553`
- Test artifacts: `dist\acceptance-20260925`
- Desktop: `OpenAI.Codex_26.917.9434.0_x64__2p2nqsd0c76g0`
- Engine: `0.155.0-alpha.16.4`
- Conversation model shown by the desktop: `5.6 Sol`, high effort.
- The user selected full access in the isolated desktop. The test did not change
  permissions or disable safeguards.
- The normal user configuration, authentication, and global instruction files
  were fingerprinted before and after the test and remained unchanged.
- Normal desktop PID 3000 and its existing engine processes remained running.

## Tests Without Added Routing Instructions

1. Natural text-to-image request for a red/white futuristic city poster:
   - MCP selected automatically; generation used `gpt-image-2`.
   - Job `9245180f-aeac-4c32-81b2-5cd28d0f46fc`.
   - Endpoint: `https://ocean-way.top/v1/images/generations`.
   - One valid 1024 x 1536 PNG, 1,708,469 bytes.
   - Generation had already been submitted when the test turn was stopped.
     A follow-up recovered and displayed the existing output without resubmission.
   - The model read the built-in image skill, but did not use its generation
     route; the actual paid request came from the MCP.

2. One pasted reference, three requested color variants:
   - Job `83de7931-63ae-411c-a18e-bba5240787ce`.
   - Endpoint: `https://ocean-way.top/v1/images/edits`.
   - The original attachment bytes and SHA-256 reached the multipart request.
   - Slots 1 and 2 returned HTTP 502; slot 3 succeeded.
   - The model then attempted local recoloring to replace missing outputs.
     The test was stopped. This behavior failed acceptance.
   - No automatic paid retry was sent. Local recoloring is not counted as API
     generation or successful delivery.

## Short Routing Rules

Three rules were added only to the isolated home's `AGENTS.md`, with a backup:

1. Use the image MCP for OceanWay image generation/edits, without substituting
   imagegen, a dedicated image CLI, or local drawing/recoloring.
2. Preserve requested model, count, and original references; do not generate
   for analysis-only or prompt-only requests.
3. Keep successful files and report failures without changing model/route or
   retrying paid requests unless the user explicitly requests it.

The isolated desktop was restarted and a fresh task was used. An open file
picker initially prevented a graceful close; it was dismissed normally before
retrying. The test profile's missing Desktop folder was also created. No normal
desktop process was stopped.

## Tests With Short Routing Rules

1. Two original references, two requested landscape posters:
   - Attachments were selected through the desktop's actual file picker.
   - Natural request did not name MCP or the API.
   - Job `3c655f9f-f586-4fd4-b554-19f2a85a6cc4`.
   - Both references arrived in the original order with matching hashes:
     - `b4d768d979deefa95970ef2d7d2e574bb23a6df53b2f41bd53cf0f91bc08902b`
     - `265024784c8dbbaa89837e470a2646ca1b7981b2f80fc4ae18e02d09e3e45978`
   - Two successful `/images/edits` requests using `gpt-image-2`.
   - Both outputs were 1536 x 1024 PNGs and displayed in the conversation.
   - Slot 1: 1,636,824 bytes, approximately 51 seconds.
   - Slot 2: 1,668,406 bytes, approximately 49 seconds.
   - Original outputs are under the task workspace's
     `output\images\3c655f9f-f586-4fd4-b554-19f2a85a6cc4`.
   - Delivery screenshot: `dist\acceptance-20260925\mcp-dual-reference-delivery.png`.

2. Analysis and prompt writing only:
   - Requested a comparison of the two posters and a reusable text prompt,
     explicitly without generation.
   - The desktop returned analysis and prompt text.
   - MCP audit stayed at 30 entries, with zero additional image requests.

3. Explicit unavailable model:
   - Requested `oceanway-test-unavailable-model` without naming MCP.
   - Job `5404f083-8ab1-4d7d-b040-33f7e426ca19`.
   - The service received that exact model and returned HTTP 400.
   - Request ID: `9f489788-f46e-4714-9d78-1a3a79ca2be3`.
   - The desktop reported no output, model unavailability, and no retry.
   - Audit confirms one request only, with no model substitution or fallback.

Seven image API submissions were observed in total: four successful image
outputs, two HTTP 502 failures, and one expected unavailable-model HTTP 400.
These are request/result counts, not a claim about actual provider billing.

## Automated Verification

- Main project `npm test`: 32 passed.
- Prototype `npm test`: 11 passed.
- Mock coverage includes six distinct outputs, concurrency, exact reference
  bytes/order, explicit model preservation, partial failure, cancellation,
  count mismatch, persistence failure, provider changes, invalid image bytes,
  linked configuration files, and isolation guards.
- SDK startup/tool discovery passed without paid image requests.
- Review found a late-cancellation race that could overwrite a final manifest
  with a running snapshot. The prototype now waits for final persistence before
  answering a late cancellation; a deterministic interleaving test was added.
  This final lifecycle fix was mock-tested, not validated by another paid run.

## Release Boundaries

The MCP path has demonstrated real generation, original-reference transfer,
multiple references, and multiple returned images. It is not yet bundled with
the production one-click flow. A six-image natural-language live run, full
cancellation/restart recovery, failed-slot retry, and full installer lifecycle
are not signed off.

The successful two-image run after adding routing rules does not independently
verify partial-502 handling with those rules. The unavailable-model failure
test passed, but the original three-image run remains a failed acceptance case.

Do not publish a release based solely on this feasibility report. Do not treat
the earlier three-output partial failure as a passed test.
