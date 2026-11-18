# Demo recordings

The README GIFs are recorded with [vhs](https://github.com/charmbracelet/vhs) against a demo account
(a profile logged in to the demo account; the GIFs use its sensor "Greenhouse").

```sh
brew install vhs
cargo build --release
export HN_DEMO_PROFILE=demo                            # logged in: hydronode -p demo login
export HN_DEMO_SENSOR=<sensor id of the demo account>
export HN_DEMO_BIN="$PWD/target/release"                # record the current build
for t in quickstart values watch; do vhs docs/demo/$t.tape; done
```

Each tape writes its GIF next to itself. Look at every frame before committing: no tokens, no
secrets, no e-mail addresses, no personal data. `simulate.tape` sends values as a device, so it is
not recorded against the review account.
