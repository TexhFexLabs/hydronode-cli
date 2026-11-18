# Security

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private vulnerability reporting
("Security" tab → "Report a vulnerability") on this repository. We answer within a few days and
tell you when a fix is out.

Include what you did, what happened, and the CLI version (`hn --version`). Never send real tokens
or sensor secrets; a description of where they leak is enough.

## Scope

- The `hydronode` / `hn` program and the `hydronode-api` crate in this repository.
- Problems in the HydroNode server or web app: report them the same way, we forward them.

## What the CLI promises

- Login with OAuth 2.1 (PKCE S256, loopback redirect, `state` check) or the device grant.
- Tokens in the OS keychain; fallback file with mode 0600 and a warning.
- One refresh at a time per profile (file lock, re-read after the lock).
- HTTPS only (rustls); plain HTTP only for `localhost` and `127.0.0.1`.
- Tokens and secrets are never logged. Only `hn sensors credentials` and `hn tokens create` print a
  secret, because you asked for it.

## Supported versions

Only the latest release gets fixes.
