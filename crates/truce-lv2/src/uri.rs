//! The three things the runtime wrapper and the TTL renderer must
//! agree on exactly: the plugin URI, the UI URI, and a category's
//! declared audio port counts.
//!
//! They live in `truce-lv2` - the crate linked into every shipped
//! plugin binary - rather than in the render half, because the runtime
//! is the side that cannot afford to be wrong. A host persists the
//! plugin URI into saved sessions and looks the plugin up by it on
//! reload; a port count that disagrees with the TTL lets the host
//! connect audio buffers where the plugin expects atom sequences.
//! `truce-lv2-meta` depends on this module for both, so there is one
//! definition and the render side is the one that follows.

/// The plugin's LV2 URI: the identity LV2 hosts persist in saved
/// sessions. Both the manifest writer and the runtime descriptor
/// lookup call this so a host can rediscover the plugin under the same
/// URI it persisted.
///
/// Format: `<vendor_url>/lv2/<bundle_id>`, or `urn:truce:<bundle_id>`
/// when `vendor_url` is empty (lilv's reference loader prefers an
/// `http://` URI but accepts the URN fallback for projects that
/// haven't picked a public URL yet).
#[must_use]
pub fn plugin_uri(vendor_url: &str, bundle_id: &str) -> String {
    if vendor_url.is_empty() {
        return format!("urn:truce:{bundle_id}");
    }
    format!("{}/lv2/{}", vendor_url.trim_end_matches('/'), bundle_id)
}

/// Convention: `<plugin_uri>#ui`. Same single-source-of-truth posture
/// as [`plugin_uri`].
#[must_use]
pub fn ui_uri(vendor_url: &str, bundle_id: &str) -> String {
    format!("{}#ui", plugin_uri(vendor_url, bundle_id))
}

/// A plugin category as LV2 names it. Distinct from
/// `truce_core::info::PluginCategory` because the mapping is not
/// one-to-one in general and because the TTL renderer has no business
/// depending on runtime types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lv2Category {
    Instrument,
    Effect,
    NoteEffect,
    Analyzer,
    Tool,
}

impl Lv2Category {
    /// The `(audio_in, audio_out)` port counts the TTL declares for
    /// this category.
    ///
    /// The TTL is generated without the plugin's runtime
    /// `bus_layouts()`, so these fixed counts are the contract the host
    /// binds against; the runtime port layout must match them exactly
    /// or a port-index shift lets the host connect audio buffers where
    /// the plugin expects atom sequences (heap corruption). Both the
    /// renderer and the runtime's `derive_port_layout` read this one
    /// source.
    #[must_use]
    pub fn audio_io(self) -> (u32, u32) {
        match self {
            Self::Instrument => (0, 2),
            Self::Effect | Self::Analyzer | Self::Tool => (2, 2),
            Self::NoteEffect => (0, 0),
        }
    }
}
