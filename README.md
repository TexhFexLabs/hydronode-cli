<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/logo-dark.svg">
    <img src="docs/assets/logo-light.svg" alt="HydroNode" width="96" height="96">
  </picture>
</p>

<h1 align="center">hydronode</h1>

<p align="center">
  Your HydroNode sensors, values and devices from the terminal.
</p>

<p align="center">
  <a href="https://github.com/TexhFexLabs/hydronode-cli/actions/workflows/ci.yml"><img src="https://github.com/TexhFexLabs/hydronode-cli/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/rust-1.89%2B-orange.svg" alt="Rust 1.89+">
  <img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg" alt="macOS, Linux, Windows">
</p>

`hydronode` (short: `hn`) signs in like the web app, shows what your sensors measured, watches live
values, switches outputs, sends commands, updates firmware over the air and fakes a device when you
need test data. Tables for people, `--json` for scripts.

<!-- Demo GIFs are rendered from docs/demo/*.tape with vhs (https://github.com/charmbracelet/vhs). -->
<!-- <p align="center"><img src="docs/demo/quickstart.gif" alt="hn login, hn status, hn values" width="720"></p> -->

## Install

> The first release is not out yet. The channels below are prepared and go live with it.

| Way | Command |
|---|---|
| Homebrew (macOS, Linux) | `brew install texhfexlabs/tap/hydronode` |
| Shell installer (macOS, Linux) | `curl -LsSf https://github.com/TexhFexLabs/hydronode-cli/releases/latest/download/hydronode-cli-installer.sh \| sh` |
| winget (Windows 10/11) | `winget install TexhFexLabs.HydroNode` |
| PowerShell installer (Windows) | `powershell -c "irm https://github.com/TexhFexLabs/hydronode-cli/releases/latest/download/hydronode-cli-installer.ps1 \| iex"` |
| Snap (Linux) | `sudo snap install hydronode` (the command is `hydronode`; `sudo snap alias hydronode.hn hn` adds `hn`) |
| Download | archives for every system on [GitHub Releases](https://github.com/TexhFexLabs/hydronode-cli/releases), with `.sha256` and build provenance (`gh attestation verify <file> -R TexhFexLabs/hydronode-cli`) |
| From source | `git clone https://github.com/TexhFexLabs/hydronode-cli && cargo install --path hydronode-cli/crates/hydronode-cli` |

Every way installs two names for the same program: `hydronode` (clear in scripts) and `hn` (short at
the prompt). Binaries: macOS (Apple silicon, Intel), Linux (x86_64, arm64, static musl), Windows x86_64.

## Quickstart

```console
$ hn login                      # pick browser, device code or API token
$ hn status                     # devices, anomalies, messages at a glance
$ hn sensors                    # your sensors with their last value
$ hn values <sensor-id>/temperature --from 7d
$ hn watch                      # live values, q quits
```

Over SSH or without a browser: `hn login --device` shows a code you confirm on your phone.
In CI: `echo "$HYDRONODE_TOKEN" | hn login --with-token` with an API token.

## Commands

| Group | Command | What it does |
|---|---|---|
| Start | `login`, `logout`, `whoami` | Sign in with the browser, a device code or an API token; sign out; show the account |
| | `status` | Projects, device health, open anomalies, unread messages, what needs attention |
| Data | `sensors` | List, show, create, rename, delete; `credentials` prints the device secret (owner) |
| | `values` | Min, max, average, trend and gaps in at most 48 steps; `--raw`, `--below N`, `--above N`; `values compare` (`--shift 7d`) |
| | `watch` | Full-screen live view, or one line per value with `--plain` |
| | `set`, `command` | Set a writable data point and wait for the device; queue a typed command |
| | `export` | Readings as CSV, JSON or Parquet |
| Devices | `devices` | Firmware, health, battery, signal; `update` checks, shows the changes and asks before it starts |
| | `console` | What devices sent and why a request was rejected; `-f` follows |
| | `push` | Send one round of values signed like a device |
| | `simulate` | A fake device with `--kind weather`, `greenhouse` or `random`, every `--every 30s` |
| More | `anomalies`, `messages`, `stations`, `automations` | Read and act on them |
| | `tokens`, `sessions` | API tokens; where you are signed in |
| | `api` | Any API path with your login, for everything else |
| | `config`, `profile` | Settings; servers and their logins (`-p work`) |
| | `completion`, `open` | Shell completion; open the web app |

`hn help <command>` (same as `hn <command> --help`) explains each command with examples.

## Scripts

- `--json` prints a stable JSON shape (the API answer for reads). Errors go to stderr, with `--json` as
  `{"error": {"code", "message", "hint"}}`.
- Exit codes: `0` ok, `1` error, `2` usage (bad flag, bad time, missing `--yes`), `3` not signed in or not
  allowed, `4` not found.
- Times: `24h`, `7d`, `90m`, `2026-11-01`, `2026-11-01T10:00+01:00`.
- Commands that delete or update devices ask first. Without a terminal they need `--yes`.
- `NO_COLOR=1` turns colors off; colors are only used on a terminal anyway.

```sh
hn sensors --json | jq -r '.[] | select(.monitoring.status == "OFFLINE") | .name'
hn values "$SENSOR/soil_moisture" --below 30 --from 7d --json | jq .totalMinutes
```

## Profiles

A profile is a server plus its own login. `default` points to `https://hydronode.tech`.

```console
$ hn profile add work --server https://hydronode.tech
$ hn -p work login
$ hn profile use work           # or HYDRONODE_PROFILE=work
```

Settings live in `~/.config/hydronode/config.toml` (Windows: `%APPDATA%\hydronode`), never a token.
`HYDRONODE_CONFIG_DIR` moves the folder.

## Security

- **Login like the web app.** `hn login --web` uses OAuth 2.1 authorization code with PKCE (S256) and a
  one-time redirect to `127.0.0.1` on a random port; the `state` is checked before any code is exchanged.
  `--device` uses the device authorization grant. Both end on HydroNode's consent page and give a normal
  session you can see and end under Sessions & security ("HydroNode CLI").
- **Tokens in the keychain.** macOS Keychain, Windows Credential Manager, Secret Service on Linux. Without
  one the CLI warns and keeps them in a file only you can read (`credentials.json`, mode 0600).
- **One refresh at a time.** Refreshing happens under a lock file per profile and reads the stored login
  again after taking it, so two `hn` processes never use the same refresh token twice (that would end the
  session everywhere).
- **HTTPS only**, with rustls. Plain `http://` only for `localhost` and `127.0.0.1`.
- **No secrets in output.** Tokens are never printed or logged. Exceptions you ask for explicitly:
  `hn sensors credentials` (device secret, owner only) and `hn tokens create` (shown once).
- **Read-only tokens stay read-only.** With an `hrt_` token, commands that change something stop before
  any request and say which login they need.

Report vulnerabilities as described in [SECURITY.md](SECURITY.md).

## Troubleshooting

| Problem | Fix |
|---|---|
| `error: not logged in` | `hn login`, or `hn -p <profile> login` for another profile |
| `your session has ended` | The session was signed out (web, other device). `hn login` again |
| Browser does not open | Copy the printed link, or use `hn login --device` |
| `warning: no usable system keychain` | Linux without Secret Service: tokens go to a 0600 file. Install `gnome-keyring` or `kwallet` for the keychain |
| `plain http is not allowed` | Use `https://`, or `localhost` for a local server |
| `This needs a full login` | You use a read-only token. `hn login` with the browser or a full `hat_` token |
| `hn watch` exits with code 3 | Live values need a browser or device login; API tokens cannot open the live socket |
| Strange output in pipes | Use `--json`; tables are for people |

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md). Short version: `cargo test --workspace`, `cargo clippy
--all-targets -- -D warnings`, `cargo fmt`, `cargo deny check`.

## License

MIT, see [LICENSE](LICENSE). © TexhFex Labs.
