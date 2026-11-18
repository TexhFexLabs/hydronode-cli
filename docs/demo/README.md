# Demo recordings

The README GIFs are recorded with [vhs](https://github.com/charmbracelet/vhs) with a demo account.
They are not rendered in the repository yet.

```sh
brew install vhs
hn profile add demo --server https://hydronode.tech
hn -p demo login
export HN_DEMO_SENSOR=<a sensor id of the demo account>
vhs docs/demo/quickstart.tape
```

Each tape writes its GIF next to itself. Look at every frame before committing: no tokens, no
secrets, no personal data.
