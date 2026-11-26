//! The LV2 view of `truce_build::meta`: map a format-neutral
//! [`ResolvedRoot`] plus a `truce.toml` `[[plugin]]` entry onto the
//! [`Lv2Bundle`] the TTL renderer takes.
//!
//! This is where LV2's vocabulary lives, and the only place that reads
//! a [`ResolvedParam`]'s raw `range` / `unit` / `flags` strings. Two
//! kinds of decision belong here and nowhere else:
//!
//! - *Lossy mappings.* `lv2:range` carries linear bounds and nothing
//!   else, so a skewed taper degrades to its min/max and `reversed(X)`
//!   takes X's bounds. The power-law shaping is a host-UI concern the
//!   TTL can't express; a format with a richer range model shouldn't
//!   inherit the loss.
//! - *Refusals.* A `const` bound has no build-time value, so the TTL -
//!   rendered without running the plugin - can't encode the port. LV2
//!   rejects it; every other format reads the const at runtime through
//!   `param_infos_static` and is unaffected.
//!
//! Both used to run at proc-macro expansion time, which made them
//! `compile_error!`s on a plugin that merely *enabled* the `lv2`
//! feature. They are packaging errors now, raised when the helper
//! actually renders a bundle.

use truce_build::meta::{ResolvedParam, ResolvedRoot};
use truce_build::{Category, PluginDef};
use truce_lv2::Lv2Category;

use crate::render::{
    Lv2Bundle, Lv2Flags, Lv2MidiBinding, Lv2MidiSource, Lv2Param, Lv2Range, Lv2UiType, Lv2Unit,
};

/// The `[vendor]` fields the TTL's `doap:maintainer` block needs.
/// Spelled out rather than taking a `truce.toml` [`truce_build::Config`]:
/// `cargo-truce` parses its own richer view of the file, so the two
/// callers don't share a struct.
#[derive(Debug, Clone, Copy)]
pub struct Vendor<'a> {
    pub name: &'a str,
    /// Vendor website, or `""`. Drives the URI scheme - see
    /// [`truce_lv2::uri::plugin_uri`].
    pub url: &'a str,
}

/// The LV2 spelling of a parsed `[[plugin]] category`.
#[must_use]
pub fn lv2_category(c: Category) -> Lv2Category {
    match c {
        Category::Instrument => Lv2Category::Instrument,
        Category::Effect => Lv2Category::Effect,
        Category::NoteEffect => Lv2Category::NoteEffect,
        Category::Analyzer => Lv2Category::Analyzer,
        Category::Tool => Lv2Category::Tool,
    }
}

/// Assemble the full [`Lv2Bundle`] for one `[[plugin]]` entry.
///
/// `version` is the caller's already-resolved effective version (see
/// [`truce_build::effective_plugin_version`]) so the TTL can't disagree
/// with the version the loaded plugin reports. `target_os` is the OS
/// the bundle is *for*, which on a cross-compile is not the one doing
/// the rendering; it picks the UI type (see [`Lv2UiType`]).
///
/// # Errors
///
/// A category or MIDI-wiring key `truce.toml` rejects, or a parameter
/// LV2 can't encode - see the module docs.
pub fn bundle(
    vendor: Vendor<'_>,
    plugin: &PluginDef,
    version: &str,
    target_os: &str,
    root: &ResolvedRoot,
) -> Result<Lv2Bundle, String> {
    let category = lv2_category(truce_build::parse_plugin_category(&plugin.category)?);
    // The same wiring resolution baked onto `PluginInfo`, so the TTL's
    // ports and the runtime `PortLayout` agree.
    let wiring = truce_build::midi_wiring(
        &plugin.category,
        plugin.midi_input,
        plugin.midi_output,
        plugin.midi_input_ports,
        plugin.midi_output_ports,
    )?;
    let (audio_in, audio_out) = category.audio_io();
    Ok(Lv2Bundle {
        plugin_name: display_name(plugin),
        vendor: vendor.name.to_string(),
        uri: truce_lv2::uri::plugin_uri(vendor.url, &plugin.bundle_id),
        ui_uri: truce_lv2::uri::ui_uri(vendor.url, &plugin.bundle_id),
        url: vendor.url.to_string(),
        version: version.to_string(),
        category,
        audio_in,
        audio_out,
        midi_in_ports: u32::from(wiring.input_ports),
        midi_out_ports: u32::from(wiring.output_ports),
        params: params(&root.params)?,
        meter_ids: root.meter_ids.clone(),
        // Always emit the UI block. Hosts that don't honour the UI URI
        // still load the plugin from manifest.ttl + plugin.ttl and just
        // skip the UI line.
        has_ui: true,
        ui_type: Lv2UiType::for_target_os(target_os),
    })
}

/// The name the TTL's `doap:name` carries: `[[plugin]] lv2_name` when
/// set, the plugin's display `name` otherwise.
///
/// The per-format name overrides exist because one plugin can ship
/// under different names per format - a trademark that only clears in
/// some stores, or a shorter name for a host with a narrow plugin menu.
/// `doap:name` is the only string an LV2 host displays, so this is the
/// one place `lv2_name` can take effect. An override set to `""` is
/// read as unset rather than as a nameless plugin.
fn display_name(plugin: &PluginDef) -> String {
    match plugin.lv2_name.as_deref() {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => plugin.name.clone(),
    }
}

/// Map every resolved parameter onto its control port.
///
/// # Errors
///
/// The first parameter LV2 can't encode, named in the message.
pub fn params(resolved: &[ResolvedParam]) -> Result<Vec<Lv2Param>, String> {
    resolved.iter().map(param).collect()
}

/// The `id -> lv2:symbol` table for the plugin whose metadata tree is
/// at `meta_dir`, resolved from scratch.
///
/// A preset's `lv2:port` / `pset:value` entries must name the exact
/// symbols the manifest declared, and symbol collision resolution needs
/// the whole port list - so this deliberately re-derives the full list
/// rather than reading a cached table. Empty when the tree is
/// unreadable or the plugin has a parameter LV2 can't encode; the
/// caller then falls back to a `state:state`-only preset.
#[must_use]
pub fn param_symbols(meta_dir: &std::path::Path) -> std::collections::BTreeMap<u32, String> {
    let resolved = truce_build::meta::read_root(meta_dir)
        .and_then(|r| truce_build::meta::resolve_root(meta_dir, &r.root))
        .and_then(|r| params(&r.params));
    let Ok(ports) = resolved else {
        return std::collections::BTreeMap::new();
    };
    crate::render::resolved_param_symbols(&ports)
        .into_iter()
        .collect()
}

/// Map one resolved parameter onto its control port.
fn param(p: &ResolvedParam) -> Result<Lv2Param, String> {
    if p.range_const {
        return Err(format!(
            "param `{}` uses a const in its `range`, which a TTL's `lv2:minimum` / \
             `lv2:maximum` can't express. Use literal bounds, or build this plugin \
             without the `lv2` format.",
            p.name
        ));
    }
    if p.default_const {
        return Err(format!(
            "param `{}` uses a const `default`, which a TTL's `lv2:default` can't \
             express. Use a literal default, or build this plugin without the `lv2` \
             format.",
            p.name
        ));
    }
    let range = range(&p.range, &p.kind, p.enum_count)?;
    Ok(Lv2Param {
        id: p.id,
        name: p.name.clone(),
        // Match `truce-derive::gen_param_info_literal`'s implicit
        // default (`a.default.unwrap_or(0.0)`) so the TTL agrees with
        // the `ParamInfo` VST3 / standalone read at runtime. Without
        // it, hosts read `lv2:default` from the TTL and open the plugin
        // at the range's minimum (gain at -60 dB) while every other
        // format honours 0.0.
        default_plain: p.default.unwrap_or(0.0),
        // Enum value names drive the port's `lv2:scalePoint` labels.
        // Empty for a non-enum, or an enum whose sidecar the walk
        // couldn't reach (cross-crate) - the port still renders, just
        // with bare indices.
        enum_names: if matches!(range, Lv2Range::Enum { .. }) {
            p.enum_names.clone()
        } else {
            Vec::new()
        },
        range,
        unit: unit(&p.unit),
        flags: flags(&p.flags),
        midi: midi(p),
    })
}

/// Parse a raw `#[param(range = ...)]` string into the port's
/// `lv2:range`. `enum_count` is the variant count [`truce_build::meta`]
/// resolved for a range-less `EnumParam<T>`.
fn range(s: &str, kind: &str, enum_count: Option<u32>) -> Result<Lv2Range, String> {
    let bounded = |ctor: fn(f64, f64) -> Lv2Range, prefix: &str| {
        s.strip_prefix(prefix)
            .and_then(|x| x.strip_suffix(')'))
            .map(|inner| pair_f64(inner, 2).map(|(lo, hi)| ctor(lo, hi)))
    };
    let linear = |min, max| Lv2Range::Linear { min, max };
    if let Some(r) = bounded(linear, "linear(") {
        return r;
    }
    if let Some(r) = bounded(|min, max| Lv2Range::Logarithmic { min, max }, "log(") {
        return r;
    }
    if let Some(r) = bounded(|min, max| Lv2Range::Discrete { min, max }, "discrete(") {
        return r;
    }
    // `lv2:range` carries only linear bounds - no skew. A skewed taper
    // keeps its min/max and loses the shaping; the trailing `factor`
    // (and `center`) args are ignored, hence the loose arity.
    if let Some(inner) = s.strip_prefix("skewed(").and_then(|x| x.strip_suffix(')')) {
        let (lo, hi) = pair_f64(inner, 0)?;
        return Ok(linear(lo, hi));
    }
    if let Some(inner) = s
        .strip_prefix("sym_skewed(")
        .and_then(|x| x.strip_suffix(')'))
    {
        let (lo, hi) = pair_f64(inner, 0)?;
        return Ok(linear(lo, hi));
    }
    // Reversal flips only the normalized mapping; the numeric bounds
    // are the wrapped shape's, so recurse.
    if let Some(inner) = s
        .strip_prefix("reversed(")
        .and_then(|x| x.strip_suffix(')'))
    {
        return range(inner.trim(), kind, enum_count);
    }
    if let Some(inner) = s.strip_prefix("enum(").and_then(|x| x.strip_suffix(')')) {
        let count = inner
            .trim()
            .parse()
            .map_err(|e| format!("enum count: {e}"))?;
        return Ok(Lv2Range::Enum { count });
    }
    match kind {
        "Bool" => Ok(Lv2Range::Discrete { min: 0.0, max: 1.0 }),
        // A range-less `EnumParam<T>`. Falling back to `count: 0` is
        // what produced the invalid `lv2:maximum 0` / `lv2:default 1`
        // port REAPER rejected, so only do so when the variant count is
        // genuinely unresolvable (a cross-crate enum) - the renderer
        // clamps the default into range to keep even that loadable.
        "Enum" => Ok(Lv2Range::Enum {
            count: enum_count.unwrap_or(0),
        }),
        // A range-less numeric param: the `#[param]` parser defaults the
        // runtime to `Linear{0,1}` and the sidecar carries an empty
        // range string. Mirror that rather than failing the build.
        _ if s.trim().is_empty() => Ok(linear(0.0, 1.0)),
        _ => Err(format!("unrecognised range `{s}`")),
    }
}

/// Leading `min, max` pair of a range's argument list. `exact` is the
/// argument count to require, or `0` to accept any count from two up
/// (a skew range's trailing `factor` / `center` don't reach the TTL).
fn pair_f64(s: &str, exact: usize) -> Result<(f64, f64), String> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if if exact == 0 {
        parts.len() < 2
    } else {
        parts.len() != exact
    } {
        return Err(format!("expected two args, got `{s}`"));
    }
    let lo: f64 = parts[0].parse().map_err(|e| format!("lo: {e}"))?;
    let hi: f64 = parts[1].parse().map_err(|e| format!("hi: {e}"))?;
    Ok((lo, hi))
}

fn unit(s: &str) -> Lv2Unit {
    match s {
        "dB" | "Db" | "db" => Lv2Unit::Db,
        "Hz" | "hz" => Lv2Unit::Hz,
        "ms" => Lv2Unit::Milliseconds,
        "s" => Lv2Unit::Seconds,
        "%" => Lv2Unit::Percent,
        "st" => Lv2Unit::Semitones,
        "pan" => Lv2Unit::Pan,
        _ => Lv2Unit::None,
    }
}

fn flags(s: &str) -> Lv2Flags {
    let mut out = Lv2Flags::default();
    for tok in s.split('|').map(str::trim) {
        match tok {
            "is_bypass" | "bypass" => out.is_bypass = true,
            "readonly" => out.readonly = true,
            "hidden" => out.hidden = true,
            _ => {}
        }
    }
    out
}

/// The port's `midi:binding`, from the raw binding token
/// `derive(Params)` recorded. Mirrors its spelling of `MidiBindKind`.
fn midi(p: &ResolvedParam) -> Option<Lv2MidiBinding> {
    let s = p.midi_binding.as_deref()?;
    let source = if let Some(n) = s.strip_prefix("cc(").and_then(|x| x.strip_suffix(')')) {
        Lv2MidiSource::Cc(n.parse().ok()?)
    } else {
        match s {
            "pitchbend" => Lv2MidiSource::PitchBend,
            "pressure" => Lv2MidiSource::ChannelPressure,
            "program" => Lv2MidiSource::ProgramChange,
            _ => return None,
        }
    };
    Some(Lv2MidiBinding {
        source,
        channel: p.midi_channel,
    })
}

#[cfg(test)]
mod tests {
    use super::{Lv2Range, param, params, range};
    use truce_build::meta::ResolvedParam;

    fn resolved(kind: &str, range: &str) -> ResolvedParam {
        ResolvedParam {
            name: "Level".into(),
            kind: kind.into(),
            range: range.into(),
            ..ResolvedParam::default()
        }
    }

    #[test]
    fn range_shapes_map_onto_lv2_bounds() {
        let r = |s: &str| range(s, "Float", None).unwrap();
        assert_eq!(r("linear(0, 1)"), Lv2Range::Linear { min: 0.0, max: 1.0 });
        assert_eq!(
            r("log(20, 20000)"),
            Lv2Range::Logarithmic {
                min: 20.0,
                max: 20000.0
            }
        );
        assert_eq!(
            r("discrete(-24, 24)"),
            Lv2Range::Discrete {
                min: -24.0,
                max: 24.0
            }
        );
        assert_eq!(r("enum(4)"), Lv2Range::Enum { count: 4 });
        // Skew degrades to its linear bounds, dropping the factor.
        assert_eq!(
            r("skewed(20, 20000, 0.3)"),
            Lv2Range::Linear {
                min: 20.0,
                max: 20000.0
            }
        );
        assert_eq!(
            r("sym_skewed(-1, 1, 0.5, 0)"),
            Lv2Range::Linear {
                min: -1.0,
                max: 1.0
            }
        );
        // Reversal keeps the wrapped shape's bounds.
        assert_eq!(
            r("reversed(log(20, 20000))"),
            Lv2Range::Logarithmic {
                min: 20.0,
                max: 20000.0
            }
        );
        assert!(range("wat(1, 2)", "Float", None).is_err());
    }

    #[test]
    fn range_less_params_follow_the_runtime_defaults() {
        assert_eq!(
            range("", "Float", None).unwrap(),
            Lv2Range::Linear { min: 0.0, max: 1.0 }
        );
        assert_eq!(
            range("", "Bool", None).unwrap(),
            Lv2Range::Discrete { min: 0.0, max: 1.0 }
        );
        // A range-less enum takes the variant count the metadata walk
        // resolved, and only collapses to 0 when it couldn't.
        assert_eq!(
            range("", "Enum", Some(3)).unwrap(),
            Lv2Range::Enum { count: 3 }
        );
        assert_eq!(
            range("", "Enum", None).unwrap(),
            Lv2Range::Enum { count: 0 }
        );
    }

    /// A const bound is a packaging error - LV2 can't encode it - and
    /// names the param so the author knows which one to change.
    #[test]
    fn const_bounds_are_rejected_by_name() {
        let mut p = resolved("Float", "linear(MIN, MAX)");
        p.range_const = true;
        let err = params(std::slice::from_ref(&p)).unwrap_err();
        assert!(err.contains("`Level`") && err.contains("`range`"), "{err}");

        let mut p = resolved("Float", "linear(0, 1)");
        p.default_const = true;
        let err = params(&[p]).unwrap_err();
        assert!(
            err.contains("`Level`") && err.contains("const `default`"),
            "{err}"
        );
    }

    /// An omitted `#[param(default)]` is 0.0 at runtime, so the port's
    /// `lv2:default` has to be 0.0 too - not the range's minimum.
    #[test]
    fn missing_default_is_zero_not_the_minimum() {
        let p = resolved("Float", "linear(-60, 6)");
        assert_eq!(p.default, None);
        assert_eq!(param(&p).unwrap().default_plain, 0.0);
    }

    /// Scale-point labels only reach an enum port; a numeric param that
    /// happens to carry names doesn't render them.
    #[test]
    fn scale_points_only_on_enum_ports() {
        let mut p = resolved("Enum", "");
        p.enum_count = Some(2);
        p.enum_names = vec!["Low".into(), "High".into()];
        assert_eq!(param(&p).unwrap().enum_names, ["Low", "High"]);

        let mut p = resolved("Float", "linear(0, 1)");
        p.enum_names = vec!["Low".into()];
        assert_eq!(param(&p).unwrap().enum_names, Vec::<String>::new());
    }
}
