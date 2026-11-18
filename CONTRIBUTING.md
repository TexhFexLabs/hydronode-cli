# Contributing

Thanks for helping. Small, focused pull requests are easiest to review.

## Setup

```sh
rustup toolchain install stable --component rustfmt clippy
cargo install cargo-deny
cargo test --workspace
```

The workspace has two crates:

| Crate | What |
|---|---|
| `crates/hydronode-api` | API client, login flows, credential store, refresh lock, device signing, time parser |
| `crates/hydronode-cli` | The `hydronode` and `hn` binaries: commands, help, output, live view |

## Rules

- Test first: write the failing test, see it fail, make it pass. Tests talk to a fake server
  (`wiremock`), never to a real HydroNode.
- Only `example.com` hosts and obviously fake tokens (`hat_fake…`) in tests and docs.
- Help text: English, short, one line per command in the overview; each command page goes purpose,
  then arguments and options, then examples.
- Output changes show up in `insta` snapshots. Review them (`cargo insta review` or
  `INSTA_UPDATE=always cargo test` and read the diff) and commit the `.snap` files.
- The device signature must match `crates/hydronode-api/tests/signature-vectors.json`, a copy of
  `hydronode-sdk/vectors/signature-vectors.json`. Change the source first, then the copy.

## Before you push

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

CI runs the same on Linux, macOS and Windows.
