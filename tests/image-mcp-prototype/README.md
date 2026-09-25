# Isolated Image MCP Prototype

This is an acceptance prototype, not a production feature of the configuration
application. It does not install a server into the real user profile.

## Scope

- STDIO MCP tools: `generate_images`, `get_image_job`, `cancel_image_job`.
- The current isolated OceanWay provider supplies the base URL and credentials.
  Credentials are not tool arguments, results, or generated routing instructions.
- Default model: `gpt-image-2`. Explicit models are preserved.
- Each requested output has its own request, with at most two in flight per job.
  There is no product limit of one image or another small fixed count.
- Ordered original reference files go to `/images/edits` as multipart `image[]`.
  Text-only requests go to `/images/generations`.
- Results retain original image files, hashes, dimensions, slot status, and a
  manifest. URL downloads use a separate unauthenticated HTTPS client.
- Requests are not automatically retried. Cancellation stops queued submissions;
  requests already sent may complete and may be billed.

## Local Tests

Run inside this directory:

```powershell
npm ci
npm test
```

The unit tests use temporary homes, fake credentials, and mock requests.
They do not consume image API quota.

`probe.mjs` verifies the official SDK handshake and tool discovery. It requires
the explicit isolation root. It does not generate images.

`prepare.mjs` exports helpers for the manually invoked acceptance harness:

- `prepareIsolatedMcp`: register the server and remove only known old managed
  direct-image rules, with backups.
- `addIsolatedRouting`: add three short routing rules only inside that same
  isolated home. Existing user instructions are retained.

The test fixture must contain `.oceanway-isolation.json` and a dedicated
`codex-home`. The server refuses a normal user home, active profiles it cannot
resolve, non-OceanWay providers, and linked configuration files.

Real desktop acceptance is intentionally manual and separately authorized.
Do not run paid generation during startup, capability discovery, or unit tests.

## Not Yet Production Ready

- Node and native `sharp` are local test dependencies, not a distribution design.
  The final installer must bundle its runtime or a native server.
- Registration, transactional rollback, upgrades, and uninstall/restore are not
  integrated with the one-click configuration UI.
- Jobs are in memory. Original images and manifests persist, but there is no
  server-restart recovery or explicit failed-slot retry tool.
- The explicit cancel tool works; cancellation of a desktop turn is not yet
  wired to cancellation of all queued image requests.
- Real provider failures remain possible. A successful tool handshake or model
  listing must never be presented as a successful image-generation test.
- Natural-language routing was tested in one desktop/model environment. Three
  short instructions improve routing but are not a universal model guarantee.

See `docs/IMAGE_MCP_FEASIBILITY_20260925.md` at the repository root for evidence.
