# truce-extensions

Plugin formats for [truce](https://github.com/truce-audio/truce) that
live outside the main repo.

Right now that means LV2. Everything here depends on truce; nothing in
truce depends on anything here. That direction is the whole point of
the split — truce ships a format-neutral metadata contract and an
external-helper protocol, and a format can be built, versioned and
released against them without being in the framework's tree.

## Crates

| Crate | What it is |
|-------|------------|
| `truce-lv2` | The LV2 runtime wrapper. Links into your plugin binary and exports the LV2 C ABI. |
| `truce-lv2-meta` | Renders `manifest.ttl` / `plugin.ttl` / preset TTLs from truce's metadata tree. Never linked into a plugin. |
| `truce-format-lv2` | The `cargo truce` helper binary. Wraps `truce-lv2-meta` in the JSON protocol the CLI speaks. |

## Using LV2 in a plugin

Two independent pieces, because they run at different times.

**At compile time**, your plugin links the runtime wrapper. Add it as
an optional dependency behind an `lv2` feature, the same shape the
in-tree formats use:

```toml
[features]
lv2 = ["dep:truce-lv2"]

[dependencies]
truce-lv2 = { version = "6.3", optional = true }
```

`truce::plugin!` already emits the `::truce_lv2::export_lv2!` arm under
`#[cfg(feature = "lv2")]` — truce needs no dependency on this repo to do
that, because the arm is just a token path resolved in your crate.

**At package time**, `cargo truce` needs the helper on your `PATH` to
render the TTL and lay out the bundle:

```sh
cargo install truce-format-lv2
cargo truce doctor          # confirms the helper was found
cargo truce install --lv2
```

`cargo truce doctor` reports which formats it can resolve and which
`truce.toml` keys each helper reads, so a missing helper is a named
diagnostic rather than a silent skip.

## Compatibility

The helper protocol and the metadata tree are versioned (`schema = 1`),
and the runtime wrapper links against truce's own crates, so the
practical rule is to match `major.minor`:

| truce | truce-extensions |
|-------|------------------|
| 6.3.x | 6.3.x            |

## Versioning

The workspace dependencies on truce are path dependencies on a sibling
`../truce` checkout. That keeps a local pair building against each other
without a push in between, which is the only way the seam actually gets
exercised during development. A release replaces them with a pinned git
rev or a registry version — a published crate cannot carry a path
outside its own package.

## Development

```sh
git clone https://github.com/truce-audio/truce.git
git clone https://github.com/truce-audio/truce-extensions.git
cd truce-extensions && cargo test --workspace
```

The two checkouts have to be siblings for the path dependencies to
resolve.

## Docs

Format reference and gotchas live with the rest of the truce
documentation at [truce.audio/docs](https://truce.audio/docs/).

## License

Same as truce: the Truce License 1.0 (MIT or Apache-2.0 with the
Framework Rider). See [LICENSE](LICENSE).
