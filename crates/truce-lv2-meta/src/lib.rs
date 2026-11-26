//! The render half of LV2 support: turn truce's format-neutral
//! metadata into the Turtle a host reads.
//!
//! Two layers, deliberately separate:
//!
//! - [`render`] takes an [`Lv2Bundle`] - plain strings, numbers, and
//!   enums - and prints TTL. It knows Turtle and nothing about truce.
//! - [`resolve`] is the only place that reads a
//!   `truce_build::meta::ResolvedParam`'s raw `range` / `unit` / `flags`
//!   strings and decides how LV2 spells them, including where LV2 is
//!   lossier than truce and where it has to refuse outright.
//!
//! The split is what lets a caller that already has its own port model
//! render a TTL without going through truce's `truce.toml` view, and it
//! keeps LV2's vocabulary out of the printer.
//!
//! This crate is the render side only. It is not linked into shipped
//! plugin binaries - [`truce_lv2`] is - and it depends on that crate
//! rather than the reverse, because the two must agree on the plugin
//! URI and the port indices (`truce_lv2::PortLayout`), and the runtime is
//! the side that cannot afford to be wrong.

pub mod render;
pub mod resolve;

pub use render::{
    Lv2Bundle, Lv2Category, Lv2Flags, Lv2MidiBinding, Lv2MidiSource, Lv2Param, Lv2Range, Lv2UiType,
    Lv2Unit, PRESET_MANIFEST_PREFIXES, lv2_version, preset_uri, render_preset_manifest_entry,
    render_preset_ttl, render_ttls, resolved_param_symbols,
};
pub use resolve::{Vendor, bundle, lv2_category, param_symbols, params};
// Re-exported from the runtime crate so a consumer that only renders
// metadata - the `truce-format-lv2` helper, say - doesn't name the
// plugin runtime directly just to spell a URI the same way.
pub use truce_lv2::uri::{plugin_uri, ui_uri};

#[cfg(test)]
mod tests {
    use super::{Vendor, bundle};
    use truce_build::PluginDef;
    use truce_build::bus_table::LayoutRecord;
    use truce_build::meta::ResolvedRoot;

    /// The cross-crate half of the URI contract: what the manifest
    /// declares has to be the string the runtime wrapper reports.
    ///
    /// A host persists the plugin URI into the saved session and looks
    /// the plugin up by it on reload, so a disagreement between these
    /// two doesn't fail loudly - it loses the user's session. Both sides
    /// read `truce_lv2::uri::plugin_uri`; this is the test that notices if
    /// one of them stops.
    #[test]
    fn manifest_uri_matches_the_runtime_uri() {
        let plugin = PluginDef {
            name: "My Gain".into(),
            bundle_id: "my-gain".into(),
            category: "effect".into(),
            ..PluginDef::default()
        };
        let vendor = Vendor {
            name: "Vendor",
            url: "https://example.com",
        };
        let b = bundle(
            vendor,
            &plugin,
            "1.2.3",
            "macos",
            &ResolvedRoot::default(),
            LayoutRecord::default(),
        )
        .expect("an effect with no parameters renders");

        assert_eq!(
            b.uri,
            truce_lv2::uri::plugin_uri("https://example.com", "my-gain")
        );
        assert_eq!(
            b.ui_uri,
            truce_lv2::uri::ui_uri("https://example.com", "my-gain")
        );
    }
}
