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
truce-lv2 = { version = "7.0", optional = true }
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
| 7.0.x | 7.0.x            |

## Versioning

The workspace dependencies on truce are path dependencies on a sibling
`../truce` checkout, each carrying a `~X.Y` requirement for this
workspace's `major.minor`. The path keeps a local pair building against
each other without a push in between, which is the only way the seam
actually gets exercised. At publish, cargo strips the paths and keeps
the requirements, so a published crate resolves truce from crates.io -
and `~X.Y` makes cargo enforce the compatibility table above rather
than just state it.

The consequence for development: the sibling truce has to be at the same
`major.minor`. truce only bumps its version when it releases, so between
a truce release branch opening and that release being cut, run
`scripts/align-truce.sh` to set your truce checkout's version to this
workspace's (undo with `git -C ../truce checkout -- Cargo.toml`).

## Releasing

truce releases first; this repo follows. The scripts mirror truce's
`scripts/release/`:

```sh
scripts/release/bump.sh <patch|minor|major|X.Y.Z>   # version + ~X.Y truce requirement, Cargo.lock, commit
scripts/release/release.sh --dry-run                # checks only: version drift, truce on crates.io, publish order
scripts/release/release.sh                          # tag, publish, push tag, GitHub Release
```

- `bump.sh` refuses a new `major.minor` the sibling truce isn't at.
- `release.sh` refuses to tag until every truce crate this workspace
  depends on has an `X.Y.*` release on crates.io, since `cargo publish`
  resolves them from there. Every step is idempotent, so re-run it after
  a partial failure.
- Publish order (`topo.py`) is `truce-lv2` → `truce-lv2-meta` →
  `truce-format-lv2`, computed from `cargo metadata` rather than listed.

There is no `upload-dist.sh` counterpart: this repo builds no plugin
artifacts. The helper ships through `cargo install truce-format-lv2`.

## Development

```sh
git clone https://github.com/truce-audio/truce.git
git clone https://github.com/truce-audio/truce-extensions.git
cd truce-extensions
scripts/align-truce.sh        # only while truce hasn't released this major.minor yet
cargo test --workspace
```

The two checkouts have to be siblings for the path dependencies to
resolve.

### Examples

`examples/` holds plugins built through this repo's formats.
`truce-example-gui-zoo-lv2` is truce's GUI zoo - every built-in widget,
every range shape, nested enums and meters - compiled as LV2, with the
`lv2` feature on by default. It is a workspace member, so any
`cargo build` / `clippy` / `test` of the workspace compiles its LV2
export and editor: if an LV2 plugin stops compiling, the build fails.
The repo-root `truce.toml` carries its plugin identity, which
`truce::plugin!` reads at compile time. To install it into a host:

```sh
cargo install --path crates/truce-format-lv2   # or: cargo install truce-format-lv2
cargo truce install --lv2 -p truce-example-gui-zoo-lv2
```

### CI

`.github/workflows/ci.yml` checks out truce as a sibling at `TRUCE_REF`
(the development line this repo tracks; overridable per run from
*Run workflow*), aligns its version, and runs fmt, clippy
`-D warnings`, build and test on Linux, macOS and Windows - on every
push and pull request, and nightly, since nothing in truce would
otherwise notice a change that breaks this repo.

## Docs

Format reference and gotchas live with the rest of the truce
documentation at [truce.audio/docs](https://truce.audio/docs/).

## License

Same as truce: the Truce License 1.0 (MIT or Apache-2.0 with the
Framework Rider). See [LICENSE](LICENSE).
