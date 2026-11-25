# Upstream and product changes

Oceanway Config forks [CC Switch](https://github.com/farion1231/cc-switch), MIT licensed.

- Version: `v4.0.24`
- Commit: `c0189078aacdd80fd2462f59b345f55f62b41cf2`
- Original copyright and license: [LICENSE](LICENSE)
- Product source: `src/` and `src-tauri/`; the existing CC Switch shell, components, service adapters and session manager remain the foundation.

Oceanway changes add a dedicated connection/configuration flow, a fixed Oceanway API endpoint, model selection, separate launch actions and a native image MCP service. Native storage, package identity and branding are separated from CC Switch. Product changes can be compared with the pinned upstream commit.

Brand assets are original Oceanway Design SVG/PNG exports from `Oceanway-AI/Oceanway-design` commit `35767499cec102b371329816300641619799ded1`, path `design-system/packages/tokens/src/platform-v1/logo/`. The surrounding UI retains CC Switch's layout, typography and component geometry; the small product theme overrides action colors.

The image engine is adapted from `Oceanway-AI/codex-config` commit `17107d20aca57115033c4f20889097921d62b2f0`.

Development checks and packaging are in `.github/workflows/build.yml` at repository root. Installer artifacts are development outputs; the Oceanway signed update channel will be configured for release.
