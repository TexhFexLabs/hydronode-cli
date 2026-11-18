# Changelog

All notable changes to this project are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-11-18

### Added

- Every request sends `X-HydroNode-Capabilities: data1`, so virtual, MQTT and JSON-push sensors show up.
- `sensors show` ends with a hint how to read history and set writable points.
- `export -o FILE` asks before it overwrites an existing file (`--yes` skips the question).
- README demo recordings.

### Changed

- Help, examples and hints use the name you started the program with (`hydronode` or `hn`).
- Hints only add `-p NAME` when a plain command would use another profile.
- The browser sign-in page is plainer.

### Security

- Browser login: a connection that sends nothing is dropped after 5 s, and an answer with the wrong
  `state` is ignored instead of ending the login. Only the real answer (or the timeout) ends it.
- Server answers are read up to a size limit (1 MB for login, 64 MB for the API, 1 GB for exports).
- Windows: the fallback token file is limited to your user (`icacls`) before anything is written.

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

[0.1.1]: https://github.com/TexhFexLabs/hydronode-cli/releases/tag/v0.1.1
[0.1.0]: https://github.com/TexhFexLabs/hydronode-cli/releases/tag/v0.1.0
