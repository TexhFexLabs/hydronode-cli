# Changelog

All notable changes to this project are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-18

### Added

- `hydronode` and `hn`: one program, two names.
- Login with the browser (OAuth 2.1, PKCE), a device code or an API token; profiles per server.
- Tokens in the OS keychain with a private file as fallback; refresh under a lock per profile.
- Commands: status, sensors, values (summary, raw, periods, compare), watch (full screen and plain),
  set, command, export (CSV, JSON, Parquet), devices (with checked over-the-air updates), console,
  push, simulate, anomalies, messages, stations, automations, tokens, sessions, api, config, profile,
  completion, open.
- `--json` everywhere, exit codes 0 to 4, errors with a hint.

[0.1.0]: https://github.com/TexhFexLabs/hydronode-cli/releases/tag/v0.1.0
