//! The two strings the runtime wrapper and the TTL renderer must agree
//! on exactly: the plugin URI and the UI URI.
//!
//! They live in `truce-lv2` - the crate linked into every shipped
//! plugin binary - rather than in the render half, because the runtime
//! is the side that cannot afford to be wrong. A host persists the
//! plugin URI into saved sessions and looks the plugin up by it on
//! reload. `truce-lv2-meta` depends on this module, so there is one
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
