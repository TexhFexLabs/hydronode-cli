# hydronode-cli — notes for coding agents

The HydroNode CLI. The command is `hydronode`; `hn` is the same program under a short name (installed by Homebrew,
the scripts and winget; on Snap via `snap alias`). Help and hints show the name the program was started with
(`cli::as_bin`). In docs and examples write `hydronode`.

## Rules

- Rust workspace: `crates/hydronode-api` (client, login, credential store), `crates/hydronode-cli` (commands, output).
- Every request sends `X-HydroNode-Capabilities: data1` (otherwise the API hides virtual, MQTT and push sensors).
- Tokens only in the OS keychain (file fallback 0600), never in argv, logs or `config.toml`. Redirects are never followed.
- Server text is printed through `output::clean` (no terminal control codes).
- `--json` prints the API answer unchanged. Exit codes 0 ok, 1 error, 2 usage, 3 auth, 4 not found.
- Tests first: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo deny check`.
  Help snapshots: `INSTA_UPDATE=always cargo test --test c3_help`.
- Commits: short, GPG-signed. Tags signed (`git tag -s`).

## Release

Step by step in `docs/RELEASING.md`. In short: version in `Cargo.toml` + `CHANGELOG.md`, CI green, signed tag
`vX.Y.Z` → `release.yml` (dist 0.34.0: archives for macOS/Linux/Windows, checksums, build attestations, shell and
PowerShell installers, Homebrew formula in `TexhFexLabs/homebrew-tap`) → `packages.yml` (Snap Store, winget).
`release.yml` is hand-hardened after `dist generate` (actions pinned by SHA, installer checked by SHA-256,
write rights only for `host`/`announce`, `CODESIGN_*` only for the Apple builds); keep that when regenerating.
macOS binaries are signed with the Developer ID (`macos-sign`, secrets `CODESIGN_*`, renew before 2027-12-18). Snap packaging `packaging/snap/`, winget
manifests `packaging/winget/`.
