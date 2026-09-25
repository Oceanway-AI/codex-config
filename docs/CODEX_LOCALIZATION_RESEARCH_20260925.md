# Codex Localization Integration Research

Date: 2026-09-25.
Status: source review and one mocked reproduction only. No localization feature
was installed, no production code was changed, and no live Codex profile or
application resource was modified.

## Source Pins

Public upstream files were downloaded without executing the upstream installer.
Local inspection copies are in `dist/research/codexpp-localization-693c848`.

| Source | Observed revision |
| --- | --- |
| `BigPizzaV3/CodexPlusPlus`, main | `693c8486bafb0f98e2539c455335d3d2fd42ce00` |
| `BigPizzaV3/CodexPlusPlusScriptMarket`, main | `380b17261e609fb62616aeb5e07dfaf281df81b0` |
| `hL091015/CodexPlusPlusScriptMarket`, main | `482076e76af9c78f18e3998bd99a96dc6033eb5d` |

Primary locations:

```text
https://github.com/BigPizzaV3/CodexPlusPlus/blob/693c8486bafb0f98e2539c455335d3d2fd42ce00/assets/inject/renderer-inject.js
https://github.com/BigPizzaV3/CodexPlusPlus/blob/693c8486bafb0f98e2539c455335d3d2fd42ce00/crates/codex-plus-core/src/launcher.rs
https://github.com/BigPizzaV3/CodexPlusPlus/blob/693c8486bafb0f98e2539c455335d3d2fd42ce00/crates/codex-plus-core/src/user_scripts.rs
https://github.com/BigPizzaV3/CodexPlusPlus/blob/693c8486bafb0f98e2539c455335d3d2fd42ce00/crates/codex-plus-core/src/script_market.rs
https://github.com/BigPizzaV3/CodexPlusPlusScriptMarket/blob/380b17261e609fb62616aeb5e07dfaf281df81b0/index.json
https://github.com/hL091015/CodexPlusPlusScriptMarket/blob/482076e76af9c78f18e3998bd99a96dc6033eb5d/scripts/zh_CN%E6%B1%89%E5%8C%96.user.js
https://github.com/BigPizzaV3/CodexPlusPlus/pull/2292
https://github.com/BigPizzaV3/CodexPlusPlus/pull/2274
https://github.com/BigPizzaV3/CodexPlusPlus/issues/1397
```

## Two Distinct Implementations

### Built-In Chinese Locale Feature

`assets/inject/renderer-inject.js`, lines 99-376:

- Reads `localeOverride` through Electron's `get-setting` bridge and attempts to
  set it to `zh-CN` with `set-setting`.
- Records the previous value under an owned local-storage marker.
- On disabling the feature, restores the previous value only if the current
  value still matches the value it applied.
- Overrides navigator language and patches an internal Statsig i18n
  configuration (`72216192`, `enable_i18n`, `locale_source`).
- Reloads the page after a setting change, with a session-storage guard.

This uses the client's own localization facilities, plus internal runtime
patches. It is not merely a dictionary file or a documented stable external
localization API. The setting bridge and feature-flag behavior need version
compatibility tests.

`launcher.rs`, lines 2588-2594 and 2898-2922, starts or connects to the app through
CDP and installs the renderer bridge. `user_scripts.rs` separately bundles and
evaluates user scripts. Copying a JavaScript file into `CODEX_HOME` alone does
not reproduce that loading lifecycle.

### Script-Market Entry

The market entry `codex-zhcn-translate`, version 1.0, points outside the main
repository to `hL091015/CodexPlusPlusScriptMarket`.

The retrieved script is 861 bytes. It observes DOM mutations and replaces ten
English labels on broad selectors including `span` and `div[role]`.

The script tests `document.createObserver` before constructing a
`MutationObserver`, then calls `.observe()` unconditionally. The inspected
upstream bootstrap/runtime files do not define that nonstandard property.
A Node VM test using a minimal document without that property reproduced:

```text
TypeError: Cannot read properties of null (reading 'observe')
```

This was a mocked test, not execution in the user's application. It is enough
to reject blind bundling of this exact fetched version, not to claim that every
historical script version or every user's installation fails.

The broad `innerText` writes also require review for nested UI destruction,
accidental edits to displayed conversation text, and repeated mutation cycles.
Those are source-derived risks, not live desktop failures observed here.

Integrity discrepancy:

```text
Market SHA-256:
72214D31D425D1CE936B457AA43FCC40DF55F4DE3B9B140F9510C7F392CDC845

Downloaded script SHA-256:
BE19A7930116DFE8FA1C68571D6A3BB3130714F77C7E32A6C1DA543A182270F5
```

The hashes differ. The inspected `install_market_script` downloads and writes
the script without validating the manifest SHA-256. OceanWay should use pinned,
reviewed assets and verified hashes, not execute mutable remote script URLs.

## Recent Compatibility Evidence

- PR #2274 was merged on 2026-09-22. It addresses reload loops, including the
  language path when a session-storage guard cannot be persisted.
- PR #2292 targets Codex 26.917 and reports that the setting bridge now expects
  direct parameters instead of the older `{ params: ... }` envelope.
- As checked on 2026-09-25, #2292 was closed on 2026-09-24 with `merged=false`.
  A closed pull request must not be reported as a merged/released fix.
- The inspected main-branch source still uses the older envelope at line 181.
  The PR's compatibility claim was not independently tested on a live desktop.
- Issue #1397 about localization failing after application updates remained
  open. Issue state is supporting risk context, not proof that all versions fail.

## Licensing Boundary

The inspected main Codex++ repository declares AGPL-3.0; OceanWay Config has an
Apache-2.0 LICENSE. No explicit license was found in the small linked
translation script or the inspected script-market trees.

Do not copy and redistribute those sources as though they were already covered
by OceanWay's existing license. Resolve applicable permissions and notices
before reuse, or implement the required behavior independently without copying
upstream code. No upstream code has been incorporated into production here.

## Proposed OceanWay Scope

The feature is technically plausible, but has not passed OceanWay desktop
acceptance. Keep it separate from model prompting and the image MCP.

1. Add one visible, reversible Chinese-interface option to one-click
   configuration. A default-on policy should apply only to validated builds;
   unsupported versions must not receive speculative patches.
2. Prefer the app's own language setting and resources. If supplemental
   translations are required, restrict them to reviewed application chrome;
   never rewrite conversation text, code, prompts, model names, or files.
3. Do not write localization instructions to `AGENTS.md`, install the full
   Codex++ suite, alter provider/history behavior, or change security approvals.
4. Implement a small independent localization adapter. Do not bundle the
   500 KB combined renderer script with its unrelated enhancements.
5. Record the targeted app/profile, app version, adapter version, prior setting,
   and owned modifications. Undo only owned changes, without overwriting a
   later user language choice. Expose language restoration explicitly.
6. Keep locale status independent from provider/MCP status. A language failure
   must not erase saved credentials, block ordinary Codex use, or appear as a
   fully successful localization.
7. Use OceanWay's existing confirmed-host restart protections. Do not use
   upstream process-killing scripts. Any required debug endpoint must be
   loopback-only, scoped to the confirmed instance, and have a defined lifetime.
8. Do not patch application archives or security feature flags merely to make
   an unsupported client appear compatible. Any broader adaptation requires
   separate review and explicit compatibility evidence.

## Existing Integration Points

- `src/index.html:76`: primary configuration form.
- `src/auto-configure.js:8`: configure, readback, restart orchestration.
- `src/main.js:339`: stage reporting and retry state.
- `src-tauri/src/main.rs:2936`: command registration for an independent module.
- `src-tauri/src/main.rs:714` and `:994`: existing restore and first-backup
  boundaries. These do not already back up application UI resources.
- `src-tauri/src/windows_host.rs:146` and `restart_windows.ps1:96`: confirmed
  process targeting and restart. Resource changes, if ever authorized, would
  need an explicit stopped-host transaction; they are not part of this proposal.

## Required Acceptance Before Shipping

- Supported Windows Store and standalone installations; macOS separately.
- Existing account login and API-provider setup.
- First configuration, repeat configuration, normal restart, and app update.
- Original language restoration and preservation of later manual user choices.
- No refresh loop or interruption of an active task.
- Conversation content and image-MCP behavior unchanged.
- Failures shown as failures; unsupported versions skipped with an explanation.
- No additional user-installed runtime or manually run script.

No production readiness or full-interface translation guarantee is made by
this research report.
