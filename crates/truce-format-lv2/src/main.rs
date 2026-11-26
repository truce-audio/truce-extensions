//! `truce-format-lv2` - the LV2 format helper `cargo-truce` shells out
//! to.
//!
//! `cargo-truce` finds this binary on `PATH` by name, the way cargo
//! finds `cargo-<subcommand>`, and talks to it in JSON over
//! stdin/stdout. Four verbs:
//!
//! - `describe` - declare what this format is and what it can do. Pure
//!   data: no side effects, no filesystem reads, nothing to configure.
//!   `cargo-truce` calls it once per run and caches the answer.
//! - `stage` - fill a bundle directory: the dylib under its URI-safe
//!   name, `manifest.ttl`, `plugin.ttl`, and one TTL per factory
//!   preset. Answers with the bundle-relative paths that need
//!   codesigning.
//! - `preset-encode` - one preset's `.ttl` bytes, for
//!   `cargo truce preset convert` / `pack`.
//! - `preset-decode` - the state envelope back out of a `.ttl`, for
//!   `cargo truce preset pull` / `convert`. Empty stdout means "not
//!   mine", which is routine: `pull` sweeps shared directories full of
//!   other vendors' files.
//!
//! Everything LV2-specific about *rendering* lives in
//! `truce-lv2-meta`; this crate is the process boundary and nothing
//! else. The split is what lets a consumer that wants a TTL in-process
//! depend on the library and skip the JSON.
//!
//! # Errors
//!
//! A failure is a non-zero exit with a message on stderr.
//! `cargo-truce` leaves stderr attached to its own, so a diagnostic
//! here reaches the user as it happens rather than being buffered into
//! a wrapped error.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use truce_build::PluginDef;
use truce_build::bus_table::LayoutRecord;
use truce_lv2_meta::{
    Lv2Bundle, PRESET_MANIFEST_PREFIXES, Vendor, bundle, param_symbols, plugin_uri,
    render_preset_manifest_entry, render_preset_ttl, render_ttls,
};

/// Protocol version. Must match `cargo-truce`'s own `SCHEMA`; a
/// mismatch in either direction is refused rather than guessed at.
const SCHEMA: u32 = 1;

/// Any failure, already phrased for the user.
type Error = String;

fn main() {
    let verb = std::env::args().nth(1);
    let result = match verb.as_deref() {
        Some("describe") => describe(),
        Some("stage") => stage(),
        Some("preset-encode") => preset_encode(),
        Some("preset-decode") => preset_decode(),
        Some(other) => Err(format!(
            "unknown verb `{other}`. This helper speaks `describe`, \
             `stage`, `preset-encode` and `preset-decode`."
        )),
        None => Err("expected a verb: describe | stage | preset-encode | \
                     preset-decode"
            .to_string()),
    };
    if let Err(e) = result {
        eprintln!("truce-format-lv2: {e}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// describe
// ---------------------------------------------------------------------------

/// The format declaration, verbatim.
///
/// A literal rather than a serialized struct: it is a constant, every
/// field is load-bearing for `cargo-truce`'s parse, and keeping it as
/// the exact text means what a reader sees here is what the tool gets.
///
/// Notes on the less obvious rows:
/// - `bundle_name: "slug"` - the base name is the lowercase-hyphenated
///   slug, not the plugin's file stem, because it is baked into the
///   TTL's `lv2:binary` IRI and some hosts reject a bundle whose file
///   names aren't URI-safe.
/// - Linux `install_dir` has no `system` entry: `~/.lv2` is the only
///   location, so a `--system` install there has nowhere to go and
///   `cargo-truce` says so instead of inventing a path.
/// - `preset_scan_dirs` is where *hosts* save user presets (lilv
///   writes to `~/.lv2`), which is not where we install bundles on
///   macOS - hence the two lists differing.
/// - `config_keys` lists only `lv2_name`, the one `truce.toml` key
///   that reaches the TTL (as `doap:name`). The LV2 runtime's other
///   key, `legacy_state.lv2_uris`, is baked into the plugin binary by
///   `derive(Params)` and never read at package time, so declaring it
///   would make a key load-bearing here that isn't.
const DESCRIBE: &str = r#"{
  "schema": 1,
  "format": "lv2",
  "label": "LV2",
  "cargo_feature": "lv2",
  "bundle_suffix": ".lv2",
  "bundle_name": "slug",
  "install_dir": {
    "macos": { "user": "~/Library/Audio/Plug-Ins/LV2",
               "system": "/Library/Audio/Plug-Ins/LV2" },
    "linux": { "user": "~/.lv2" },
    "windows": { "user": "%APPDATA%/LV2",
                 "system": "%COMMONPROGRAMFILES%/LV2" }
  },
  "macos_pkg": {
    "extension": ".lv2",
    "install_location": "/Library/Audio/Plug-Ins/LV2",
    "pkg_id_suffix": "lv2",
    "is_native_bundle": false,
    "choice_description": "LV2 plug-in, for Ardour, Carla, Mixbus and REAPER."
  },
  "preset_extension": "ttl",
  "preset_scan_dirs": {
    "macos": ["~/.lv2"],
    "linux": ["~/.lv2"]
  },
  "capabilities": ["stage", "presets", "preset-codec"],
  "config_keys": ["lv2_name"]
}
"#;

fn describe() -> Result<(), Error> {
    emit(DESCRIBE.as_bytes())
}

// ---------------------------------------------------------------------------
// The config view
// ---------------------------------------------------------------------------

/// `cargo-truce`'s filtered `truce.toml` view, at the path the request
/// names. The helper never sees `truce.toml` itself - only the keys it
/// declared in `config_keys`, plus the plugin identity and port shape
/// it would need regardless.
#[derive(Deserialize)]
struct ConfigView {
    schema: u32,
    /// Spelled exactly as a `truce.toml` `[[plugin]]` entry, so the
    /// shared schema parses it and this crate keeps no parallel struct.
    plugin: PluginDef,
    vendor: ViewVendor,
    #[serde(default)]
    config: ViewConfig,
}

#[derive(Deserialize)]
struct ViewVendor {
    name: String,
    /// Empty when `truce.toml` declares no vendor URL, which switches
    /// the plugin URI to its `urn:truce:` form.
    #[serde(default)]
    url: String,
}

/// The declared `config_keys`, as `cargo-truce` renders them. One
/// struct field per declared key, so the two lists can't drift without
/// a compile error here or a hard error there.
#[derive(Deserialize, Default)]
struct ViewConfig {
    #[serde(default)]
    lv2_name: Option<String>,
}

impl ConfigView {
    fn read(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading the config view {}: {e}", path.display()))?;
        let mut view: Self = toml::from_str(&text)
            .map_err(|e| format!("parsing the config view {}: {e}", path.display()))?;
        if view.schema != SCHEMA {
            return Err(format!(
                "the config view at {} is schema {}, but this helper speaks {SCHEMA}",
                path.display(),
                view.schema
            ));
        }
        // Fold the declared key back onto the `PluginDef` field it came
        // from, so the renderer reads it from one place whether it was
        // handed a config view or a parsed `truce.toml`.
        view.plugin.lv2_name = view.config.lv2_name.take();
        Ok(view)
    }

    fn vendor(&self) -> Vendor<'_> {
        Vendor {
            name: &self.vendor.name,
            url: &self.vendor.url,
        }
    }

    /// The effective version `cargo-truce` already resolved. Always
    /// written, so an absent one means the view was built by something
    /// that isn't following the protocol.
    fn version(&self) -> Result<&str, Error> {
        self.plugin
            .version
            .as_deref()
            .ok_or_else(|| "the config view carries no `[plugin] version`".to_string())
    }

    /// Assemble the renderer's input, reading the parameter list out of
    /// the metadata tree at `meta` and the audio ports out of the
    /// bus-layout table `truce::plugin!` embedded in `dylib`.
    fn bundle(&self, meta: &Path, dylib: &Path, target_os: &str) -> Result<Lv2Bundle, Error> {
        let root = truce_build::meta::read_root(meta)
            .and_then(|r| truce_build::meta::resolve_root(meta, &r.root))
            .map_err(|e| {
                format!(
                    "reading the metadata tree at {}: {e}. `derive(Params)` writes it \
                     while the plugin compiles; missing it means the plugin crate isn't \
                     listed under `[[plugin]]` in truce.toml, or wasn't built.",
                    meta.display()
                )
            })?;
        bundle(
            self.vendor(),
            &self.plugin,
            self.version()?,
            target_os,
            &root,
            self.audio_layout(dylib)?,
        )
    }

    /// The plugin's first bus layout - the one an LV2 plugin runs in,
    /// since its ports are fixed in the TTL - read out of the built
    /// binary without loading it, so a cross build renders too.
    fn audio_layout(&self, dylib: &Path) -> Result<LayoutRecord, Error> {
        truce_build::bus_table::read_from_binary(dylib, &self.plugin.crate_name)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                format!(
                    "`{}` declares no bus layouts; a plugin with no audio I/O declares \
                     `&[BusLayout::new()]`",
                    self.plugin.crate_name
                )
            })
    }

    /// The plugin's URI. `stage` reads it off the rendered
    /// [`Lv2Bundle`] instead; this is for the preset verbs, which
    /// render a single file and never build a bundle.
    fn plugin_uri(&self) -> String {
        plugin_uri(&self.vendor.url, &self.plugin.bundle_id)
    }
}

// ---------------------------------------------------------------------------
// stage
// ---------------------------------------------------------------------------

/// One factory preset, format-neutral. Parameter values are keyed by
/// truce's own FNV ids; mapping them onto `lv2:symbol`s is this
/// helper's job, which is exactly why they arrive as ids.
#[derive(Deserialize)]
struct Preset {
    uuid: String,
    name: String,
    /// Empty when the preset is uncategorized.
    #[serde(default)]
    category: String,
    /// The authored file's stem, already filename-safe. Preset files
    /// are named after it rather than after `name`, so a host that
    /// persists a preset path keeps resolving it across a rename.
    stem: String,
    /// The canonical state envelope, base64.
    blob: String,
    /// `(id, value)` in the envelope's order.
    #[serde(default)]
    params: Vec<(u32, f64)>,
}

impl Preset {
    fn blob_bytes(&self) -> Result<Vec<u8>, Error> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.blob)
            .map_err(|e| format!("preset `{}` has a blob that isn't base64: {e}", self.name))
    }

    /// The `rdfs:label` a host shows. LV2 presents one flat list, so
    /// the category is folded into the label the way subdirectories
    /// carry it in formats that have them.
    fn label(&self) -> String {
        if self.category.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.category, self.name)
        }
    }
}

#[derive(Deserialize)]
struct StageRequest {
    schema: u32,
    meta: PathBuf,
    config: PathBuf,
    dylib: PathBuf,
    /// The bundle directory to lay out. Created if absent, so `stage`
    /// stands on its own rather than depending on the caller having
    /// made it first.
    out: PathBuf,
    /// The OS this bundle is *for*, which on a cross-compile is not the
    /// one running this helper.
    target_os: String,
    #[serde(default)]
    presets: Vec<Preset>,
}

#[derive(Serialize)]
struct Staged {
    schema: u32,
    /// Bundle-relative paths needing a signature.
    signable: Vec<String>,
}

fn stage() -> Result<(), Error> {
    let request: StageRequest = read_request("stage")?;
    check_schema(request.schema)?;

    let view = ConfigView::read(&request.config)?;
    let b = view.bundle(&request.meta, &request.dylib, &request.target_os)?;

    // The binary's name has to match what the TTL's `lv2:binary` IRI
    // resolves against, and both derive from the bundle directory
    // `cargo-truce` created - so read it off that rather than
    // re-slugifying the plugin name here and hoping the two agree.
    let stem = request
        .out
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            format!(
                "the bundle directory {} has no usable name",
                request.out.display()
            )
        })?;
    let bin_name = format!("{stem}.{}", dylib_extension(&request.target_os));

    let (mut manifest, plugin_ttl) = render_ttls(&b, &bin_name);

    std::fs::create_dir_all(&request.out)
        .map_err(|e| format!("creating the bundle {}: {e}", request.out.display()))?;
    std::fs::copy(&request.dylib, request.out.join(&bin_name)).map_err(|e| {
        format!(
            "copying {} into the bundle as {bin_name}: {e}",
            request.dylib.display()
        )
    })?;
    write(&request.out.join("plugin.ttl"), plugin_ttl.as_bytes())?;

    // Presets extend the manifest, so they have to be rendered before
    // it is written - the alternative is writing it twice.
    if !request.presets.is_empty() {
        manifest.push_str(&write_presets(&request, &b.uri, &request.meta)?);
    }
    write(&request.out.join("manifest.ttl"), manifest.as_bytes())?;

    emit_json(&Staged {
        schema: SCHEMA,
        // Only on the platforms that sign anything. The inner dylib
        // rather than the bundle: an LV2 "bundle" is a plain directory
        // with no `Contents/Info.plist`, and `codesign` refuses to seal
        // a directory it doesn't recognise - but the Mach-O itself
        // signs fine, and that is the only file Apple's notary
        // inspects.
        signable: match request.target_os.as_str() {
            "macos" | "windows" => vec![bin_name],
            _ => Vec::new(),
        },
    })
}

/// Write `presets/<stem>.ttl` for each preset and return the
/// `manifest.ttl` block that points at them.
fn write_presets(request: &StageRequest, plugin_uri: &str, meta: &Path) -> Result<String, Error> {
    let presets_dir = request.out.join("presets");
    std::fs::create_dir_all(&presets_dir)
        .map_err(|e| format!("creating {}: {e}", presets_dir.display()))?;

    // Resolved once for the whole library rather than per preset:
    // symbol collision resolution needs the full port list, so this is
    // a re-derivation of the entire thing.
    let symbols = param_symbols(meta);

    let mut additions = String::from(PRESET_MANIFEST_PREFIXES);
    for preset in &request.presets {
        // An id with no symbol is a parameter LV2 couldn't encode, so
        // the port doesn't exist to apply a value to. The preset still
        // carries the full envelope, which is the path lilv hosts use.
        let ports: Vec<(String, f64)> = preset
            .params
            .iter()
            .filter_map(|(id, v)| symbols.get(id).map(|sym| (sym.clone(), *v)))
            .collect();
        let ttl = render_preset_ttl(
            plugin_uri,
            &preset.uuid,
            &preset.label(),
            &preset.blob_bytes()?,
            &ports,
        );
        let file_name = format!("{}.ttl", preset.stem);
        write(&presets_dir.join(&file_name), ttl.as_bytes())?;
        additions.push_str(&render_preset_manifest_entry(
            plugin_uri,
            &preset.uuid,
            &format!("presets/{file_name}"),
        ));
    }
    Ok(additions)
}

/// The shared-library extension of the OS a bundle is for. LV2 keeps
/// the plain platform extension (no `.vst3`-style wrapper), and the
/// unix answer is `.so` everywhere including macOS - lilv looks for it
/// by the name the TTL gives, not by a platform convention.
fn dylib_extension(target_os: &str) -> &'static str {
    if target_os == "windows" { "dll" } else { "so" }
}

// ---------------------------------------------------------------------------
// preset-encode / preset-decode
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct EncodeRequest {
    schema: u32,
    meta: PathBuf,
    config: PathBuf,
    preset: Preset,
}

/// Render one preset `.ttl` and write its bytes to stdout. The caller
/// decides where they land (a `.lv2` bundle, a zip entry), so this
/// never has to know about either.
fn preset_encode() -> Result<(), Error> {
    let request: EncodeRequest = read_request("preset-encode")?;
    check_schema(request.schema)?;

    let view = ConfigView::read(&request.config)?;
    let symbols = param_symbols(&request.meta);
    let ports: Vec<(String, f64)> = request
        .preset
        .params
        .iter()
        .filter_map(|(id, v)| symbols.get(id).map(|sym| (sym.clone(), *v)))
        .collect();

    let ttl = render_preset_ttl(
        &view.plugin_uri(),
        &request.preset.uuid,
        &request.preset.label(),
        &request.preset.blob_bytes()?,
        &ports,
    );
    emit(ttl.as_bytes())
}

#[derive(Serialize)]
struct Decoded {
    schema: u32,
    name: String,
    /// The canonical state envelope, base64.
    blob: String,
}

/// Recover the state envelope from a preset `.ttl` on stdin, or print
/// nothing when the file isn't one of ours.
///
/// Unlike the other verbs this takes raw bytes rather than a JSON
/// request: there is nothing else to say, and base64-wrapping a whole
/// preset file to keep the shape symmetric would be worse than the
/// asymmetry.
fn preset_decode() -> Result<(), Error> {
    let bytes = read_stdin()?;
    // Not an error. `cargo truce preset pull` sweeps shared preset
    // directories, so most files it offers are other plugins' - or not
    // text at all.
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(());
    };
    let Some((name, blob)) = parse_preset_ttl(&text) else {
        return Ok(());
    };
    emit_json(&Decoded {
        schema: SCHEMA,
        name,
        blob: base64::engine::general_purpose::STANDARD.encode(&blob),
    })
}

/// Pull `(rdfs:label, state envelope)` out of a preset TTL: the
/// `<urn:truce:state-blob>` property's base64 literal - the State
/// extension key the runtime stores under - plus the label when
/// present.
///
/// A tolerant line-oriented scan rather than a Turtle parser, because
/// both this helper's emitter and lilv (which writes host-saved user
/// presets) keep the property and its literal on one statement, and
/// pulling in an RDF stack to read one property would be the only
/// reason this crate needed one.
///
/// `None` for anything without that property, which covers both other
/// vendors' presets and files that aren't Turtle at all.
fn parse_preset_ttl(text: &str) -> Option<(String, Vec<u8>)> {
    let after_key = text.split("<urn:truce:state-blob>").nth(1)?;
    // lilv wraps long literals, so the base64 can arrive across
    // several lines.
    let b64: String = quoted(after_key)?.split_whitespace().collect();
    let blob = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let name = text
        .split("rdfs:label")
        .nth(1)
        .and_then(quoted)
        .unwrap_or_default()
        .to_string();
    Some((name, blob))
}

/// The contents of the first double-quoted run in `s`.
fn quoted(s: &str) -> Option<&str> {
    let start = s.find('"')? + 1;
    let end = start + s.get(start..)?.find('"')?;
    s.get(start..end)
}

// ---------------------------------------------------------------------------
// Plumbing
// ---------------------------------------------------------------------------

fn check_schema(schema: u32) -> Result<(), Error> {
    if schema == SCHEMA {
        return Ok(());
    }
    Err(format!(
        "the request is schema {schema}, but this helper speaks {SCHEMA}. \
         Install a truce-format-lv2 matching your cargo-truce."
    ))
}

fn read_stdin() -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .map_err(|e| format!("reading the request from stdin: {e}"))?;
    Ok(buf)
}

fn read_request<T: for<'de> Deserialize<'de>>(verb: &str) -> Result<T, Error> {
    let bytes = read_stdin()?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("the `{verb}` request on stdin isn't valid JSON: {e}"))
}

fn emit_json<T: Serialize>(value: &T) -> Result<(), Error> {
    let bytes =
        serde_json::to_vec(value).map_err(|e| format!("building the answer as JSON: {e}"))?;
    emit(&bytes)
}

fn emit(bytes: &[u8]) -> Result<(), Error> {
    std::io::stdout()
        .write_all(bytes)
        .map_err(|e| format!("writing the answer to stdout: {e}"))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    std::fs::write(path, bytes).map_err(|e| format!("writing {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo-truce` parses this at startup; a typo in it would make
    /// the helper look broken rather than look like a typo.
    #[test]
    fn the_describe_output_is_valid_json_declaring_this_schema() {
        let v: serde_json::Value = serde_json::from_str(DESCRIBE).expect("DESCRIBE is valid JSON");
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(v["format"], "lv2");
        // The binary suffix and the format token have to agree: the
        // token is what `--lv2` resolves to and what this binary's own
        // name ends with.
        assert_eq!(v["bundle_suffix"], ".lv2");
    }

    /// Every key `describe` declares needs a field on [`ViewConfig`],
    /// or `cargo-truce` writes a value this helper then ignores.
    #[test]
    fn every_declared_config_key_is_one_this_helper_reads() {
        let v: serde_json::Value = serde_json::from_str(DESCRIBE).expect("valid JSON");
        let declared: Vec<&str> = v["config_keys"]
            .as_array()
            .expect("config_keys is a list")
            .iter()
            .map(|k| k.as_str().expect("a key is a string"))
            .collect();
        assert_eq!(declared, ["lv2_name"]);

        let view: ViewConfig = toml::from_str(r#"lv2_name = "Custom""#).expect("parses");
        assert_eq!(view.lv2_name.as_deref(), Some("Custom"));
    }

    /// The `[plugin]` block is spelled as a `truce.toml` entry, so the
    /// shared `PluginDef` reads it - including `crate`, which is the
    /// one key whose field name differs.
    #[test]
    fn the_config_view_parses_as_a_truce_toml_plugin_entry() {
        let dir = std::env::temp_dir().join("truce-format-lv2-view-test");
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let path = dir.join("lv2.config.toml");
        std::fs::write(
            &path,
            r#"schema = 1

[plugin]
name = "My Gain"
bundle_id = "my-gain"
crate = "my_gain"
version = "1.2.3"
category = "effect"
midi_input = true

[vendor]
name = "Acme"
id = "com.acme"
url = "https://acme.test"

[config]
lv2_name = "My Gain (LV2)"
"#,
        )
        .expect("writing the view");

        let view = ConfigView::read(&path).expect("the view parses");
        assert_eq!(view.plugin.name, "My Gain");
        assert_eq!(view.plugin.crate_name, "my_gain");
        assert_eq!(view.plugin.category, "effect");
        assert_eq!(view.plugin.midi_input, Some(true));
        assert_eq!(view.version().expect("a version"), "1.2.3");
        assert_eq!(view.vendor().url, "https://acme.test");
        // The declared key is folded onto the `PluginDef` field it came
        // from, so the renderer has one place to read it.
        assert_eq!(view.plugin.lv2_name.as_deref(), Some("My Gain (LV2)"));
    }

    /// A view from a newer `cargo-truce` is refused, not read as if its
    /// fields meant what this build thinks they mean.
    #[test]
    fn a_view_from_another_schema_is_refused() {
        let dir = std::env::temp_dir().join("truce-format-lv2-schema-test");
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let path = dir.join("lv2.config.toml");
        std::fs::write(
            &path,
            r#"schema = 2

[plugin]
name = "X"
bundle_id = "x"
crate = "x"
category = "effect"

[vendor]
name = "Acme"
id = "com.acme"
"#,
        )
        .expect("writing the view");
        let Err(err) = ConfigView::read(&path) else {
            panic!("a schema mismatch must be refused, not read anyway");
        };
        assert!(err.contains("schema 2"), "{err}");
    }

    /// The round trip the `preset convert` / `pull` pair relies on.
    #[test]
    fn a_rendered_preset_decodes_back_to_its_envelope_and_label() {
        let blob = b"not really an envelope, but opaque to the codec".to_vec();
        let ttl = render_preset_ttl(
            "https://example.com/lv2/x",
            "u-1",
            "pad/Glass",
            &blob,
            &[("cutoff".to_string(), 8000.0)],
        );
        let (name, decoded) = parse_preset_ttl(&ttl).expect("our own output decodes");
        assert_eq!(name, "pad/Glass");
        // Port values present and still ignored: the envelope is the
        // canonical copy, the ports are the redundant one.
        assert_eq!(decoded, blob);
    }

    /// Someone else's preset, and a file that isn't Turtle at all, are
    /// both "not mine" rather than errors - `preset pull` sweeps
    /// directories full of them.
    #[test]
    fn a_foreign_preset_is_not_recognised() {
        assert!(parse_preset_ttl("@prefix pset: <...> .\n<x> a pset:Preset .").is_none());
        assert!(parse_preset_ttl("\x00\x01 definitely not turtle").is_none());
    }

    /// A wrapped base64 literal - lilv writes long ones across lines -
    /// still decodes.
    #[test]
    fn a_wrapped_base64_literal_decodes() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(b"0123456789abcdef");
        let (first, second) = encoded.split_at(8);
        let ttl = format!(
            "    state:state [\n        <urn:truce:state-blob> \"{first}\n{second}\"^^xsd:base64Binary\n    ] .\n"
        );
        let (_, decoded) = parse_preset_ttl(&ttl).expect("a wrapped literal decodes");
        assert_eq!(decoded, b"0123456789abcdef");
    }

    /// The bundle's binary extension follows the target OS, not the
    /// host. LV2 keeps `.so` on macOS.
    #[test]
    fn the_binary_extension_follows_the_target_os() {
        assert_eq!(dylib_extension("windows"), "dll");
        assert_eq!(dylib_extension("macos"), "so");
        assert_eq!(dylib_extension("linux"), "so");
    }
}
