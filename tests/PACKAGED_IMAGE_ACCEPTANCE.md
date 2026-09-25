# Packaged Image MCP Acceptance

This is a bounded stdio integration test, not desktop UI or natural-language
acceptance. It executes the supplied packaged binary without rebuilding Rust.
Only `tests/` files are added by this change.

## Run

```powershell
node tests/mcp-bundle-images.mjs --executable D:/AI-Workspace/worktrees/codex-config-sync-image/dist/build-139ceb8/codex-config.exe --report tests/results/mcp-bundle-images-139ceb8.json
node --test tests/image-desktop-fixture.test.js
```

The harness imports the existing `image-desktop-fixture.mjs` with
`allowTextProxy: false` and uses only `oceanway-local-fixture`. It creates a fresh
temporary CODEX_HOME, workspace, profile and app-data directories, and passes an
allowlisted environment to subprocesses. CLI configuration is mock setup only.
It does not read normal Codex credentials or start a text client. An outbound
`fetch` tripwire guards the fixture's unused upstream branch.

The two small reference PNGs have different dimensions and pixels. The fixture
audits multipart byte lengths and SHA-256 values, including duplicate roles.
Successful images are known fixture bytes, not real generated images.

## Recorded Result

Final run: **8 passed, 0 failed**, September 25, 2026, 16:34:47-16:34:51 UTC
(September 26, 00:34:47-00:34:51 Asia/Shanghai). The separate fixture suite
passed **4 tests, 0 failed**. A preliminary run passed the same assertions but
revealed a harness report-status collision; that reporting bug was fixed before
the final run. No production blocker was observed in this scope.

Executable SHA-256:
`0f2dcbda04168c59c53bb7cd0b24237aed4bee852e0564105700f8b76d21a328`.

| Case | Result |
| --- | --- |
| CLI setup, initialize, tools/list | Four tools; zero image POSTs |
| Seven distinct prompts | Seven slots, seven POSTs, correct slot/request mapping |
| Ordered original references | Two edit POSTs; exact A/B/A bytes and hashes |
| Partial failure and explicit retry | Three successes retained; only two failed slots resent |
| Live ownership and cancellation | Observer left manifest unchanged; two admitted saved, seven queued cancelled |
| Stdin EOF drain | Two admitted saved; four queued cancelled; restart made zero POSTs |
| Forced process recovery | Three successes retained; two uncertain slots; zero automatic POSTs; explicit retry only |
| Extra provider output | Warning status, both outputs retained, completed retry rejected |

The final run made **30 loopback image POSTs**, peak concurrency **2**, and zero
upstream fetch attempts. Four subprocesses exited normally; one harness-owned
MCP child was intentionally terminated for crash recovery. Every owned process
exited, the fixture closed with zero active requests, and the temporary directory
was removed. Cleanup reported no errors.

Machine-readable evidence: `results/mcp-bundle-images-139ceb8.json`. Output paths
in that report describe already-removed temporary artifacts; their hashes,
request IDs, counts and synthetic fixture audit are retained for review.

## Limits

No production source changes, real credentials, upstream image requests, proxy
harness, full 240-second timeout test, desktop UI test, or model changes.
The harness is intentionally sequential except for the engine's own two workers.
