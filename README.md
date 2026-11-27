# truce-extensions

Extensions for [truce](https://github.com/truce-audio/truce): crates that
build on truce's public API and release on their own schedule. Plugin
formats, editor backends, DSP and GUI components, tooling - anything that
depends on truce, with nothing in truce depending on it.

## What's here

| Extension | Crates |
|-----------|--------|
| LV2 plugin format | `truce-lv2`, `truce-lv2-meta`, `truce-format-lv2` |

## Kinds of extension

Each kind meets truce at a different seam.

**Plugin formats** have two halves, because they run at different times.
A runtime wrapper links into the plugin and exports the format's C ABI;
`truce::plugin!` emits an `export_<format>!` arm for it under
`#[cfg(feature = "<format>")]`, resolved in the plugin's own crate. A
`truce-format-<format>` helper binary on `PATH` renders the bundle at
package time: `cargo truce` discovers it, and it reads the format-neutral
metadata tree truce's derives write, through a versioned JSON protocol.
A format with a new feature name also needs its arm added to
`truce::plugin!` - the one change on truce's side.

**Editor backends** implement `truce_core::editor::Editor`, the trait a
plugin returns from `PluginLogic::editor`, as `truce-egui`, `truce-iced`
and `truce-slint` do. Nothing in truce has to change.

**Components and tooling** - DSP building blocks, widgets, preset
utilities - are ordinary crates on truce's public API.

## Using LV2 in a plugin

**At compile time**, the plugin links the runtime wrapper, as an optional
dependency behind an `lv2` feature:

```toml
[features]
lv2 = ["dep:truce-lv2"]

[dependencies]
truce-lv2 = { version = "7.0", optional = true }
```

`truce::plugin!` already emits `::truce_lv2::export_lv2!` under
`#[cfg(feature = "lv2")]`.

**At package time**, `cargo truce` needs the helper on `PATH` to render
the TTL and lay out the bundle:

```sh
cargo install truce-format-lv2
cargo truce doctor          # confirms the helper was found
cargo truce install --lv2
```

`cargo truce doctor` lists the formats it can resolve and the
`truce.toml` keys each helper reads, so a missing helper is a named
diagnostic rather than a silent skip.

## Adding an extension

- Its crates go under `crates/`, taking truce through the workspace's
  `[workspace.dependencies]` (`~X.Y` requirements on `../truce` paths; see
  *Versioning*).
- It gets an example under `examples/`: a workspace member with the
  extension enabled by default and `publish = false`. Every
  `cargo build` / `clippy` / `test` of the workspace then compiles a real
  plugin through the extension, so CI fails the moment it stops working
  against truce. The repo-root `truce.toml` carries the examples' plugin
  identities, which `truce::plugin!` reads at compile time.
- Its crates release with the rest of the workspace (see *Releasing*).

## Compatibility

Extensions link against truce's own crates, and the format helper
protocol and metadata tree are versioned (`schema = 1`), so the rule is to
match `major.minor`:

| truce | truce-extensions |
|-------|------------------|
| 7.0.x | 7.0.x            |

## Versioning

The workspace's truce dependencies are path dependencies on a sibling
`../truce` checkout, each with a `~X.Y` requirement for this workspace's
`major.minor`. The path lets a local pair build against each other
without a push in between. At publish, cargo strips the paths and keeps
the requirements, so a published crate resolves truce from crates.io, and
`~X.Y` makes cargo enforce the compatibility table above.

The sibling truce has to be at the same `major.minor`. truce bumps its
version when it releases, so while a truce release is in development, run
`scripts/align-truce.sh` to set your truce checkout's version to this
workspace's (undo with `git -C ../truce checkout -- Cargo.toml`).

## Releasing

truce releases first; this repo follows.

```sh
scripts/release/bump.sh <patch|minor|major|X.Y.Z>   # version + ~X.Y truce requirement, Cargo.lock, commit
scripts/release/release.sh --dry-run                # checks only: version drift, truce on crates.io, publish order
scripts/release/release.sh                          # tag, publish, push tag, GitHub Release
```

- `bump.sh` refuses a new `major.minor` the sibling truce isn't at.
- `release.sh` refuses to tag until every truce crate the workspace
  depends on has an `X.Y.*` release on crates.io, since `cargo publish`
  resolves them from there. Every step is idempotent, so re-run it after a
  partial failure.
- Every crate except the examples publishes, in dependency order
  (`scripts/release/topo.py`).

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

To install an example into a host, here the LV2 one:

```sh
cargo install --path crates/truce-format-lv2
cargo truce install --lv2 -p truce-example-gui-zoo-lv2
```

### CI

`.github/workflows/ci.yml` checks out truce as a sibling at `TRUCE_REF`
(the truce development line this repo tracks; overridable per run from
*Run workflow*), aligns its version, and runs fmt, clippy `-D warnings`,
build and test on Linux, macOS and Windows - on every push and pull
request, and nightly, so a truce change that breaks an extension shows up
within a day.

A second job, on Linux and macOS, builds LV2 bundles the way a plugin
author does and checks them the way a host sees them: the example, and an
effect and an instrument freshly scaffolded with `cargo truce new` and
given LV2 as the scaffold's comment says to (`scripts/ci/lv2-scaffold.sh`).
`scripts/ci/lv2-check.sh` runs `lv2_validate` against the LV2 spec, checks
lilv discovers the plugin, then instantiates it and runs a block. Both
scripts run locally too, with lilv installed (`brew install lilv`, or
`scripts/ci/install-linux-deps.sh --lv2`).

## Docs

Extension reference and gotchas live with the rest of the truce
documentation at [truce.audio/docs](https://truce.audio/docs/).

## License

Same as truce: the Truce License 1.0 (MIT or Apache-2.0 with the
Framework Rider). See [LICENSE](LICENSE).
