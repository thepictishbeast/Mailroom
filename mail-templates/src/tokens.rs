//! Inline-style design tokens for the email chrome.
//!
//! Email clients strip `<style>` blocks (Gmail), inline only `<head>`
//! styles (Outlook on Windows), or override colors in dark mode
//! unpredictably (iOS Mail). The defensive answer is to inline every
//! style as an attribute on the element that needs it. These tokens
//! are exposed via a [`Theme`] struct so different tenants
//! (PlausiDen, SacredVote, future) can render the same AST with
//! their own chrome.

use serde::{Deserialize, Serialize};

/// Brand-aligned design tokens for one tenant. Drop into
/// [`crate::EmailDocument::render_html_with_theme`] to render the
/// AST with the tenant's palette + footer copy. The default
/// [`Theme::plausiden`] keeps the in-house chrome intact for
/// callers who pass nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    /// Hero gradient deep tone (top-left).
    pub brand_navy: String,
    /// Hero gradient mid tone + body link color.
    pub brand_primary: String,
    /// Hero gradient bright tone + accent stripes.
    pub brand_accent: String,

    /// Heading text color (`#0f172a` in PlausiDen).
    pub text_heading: String,
    /// Body prose color (`#334155`).
    pub text_body: String,
    /// Muted secondary text (`#64748b`).
    pub text_muted: String,

    /// Page surface (background of the email viewport).
    pub surface: String,
    /// Raised card surface (white in PlausiDen).
    pub surface_raised: String,
    /// Border / hairline color.
    pub hairline: String,

    /// Display name in the gradient hero (e.g., `"PlausiDen"`).
    pub brand_name: String,
    /// Suffix after brand name (e.g., `"LLC"`).
    pub brand_suffix: String,
    /// Tagline under the brand (e.g., `"Plausible. Defensible."`).
    pub tagline: String,
    /// Single character rendered in the logo tile.
    pub logo_letter: String,

    /// Footer organization name.
    pub footer_org: String,
    /// Footer location/address line (e.g., `"Massachusetts, USA"`).
    pub footer_address: String,
    /// Footer website URL (without scheme — `plausiden.com`).
    pub footer_website: String,
    /// Footer contact email.
    pub footer_email: String,
}

impl Theme {
    /// PlausiDen LLC default theme — Massachusetts navy + accent
    /// blue gradient, "Plausible. Defensible." tagline.
    #[must_use]
    pub fn plausiden() -> Self {
        Self {
            brand_navy: "#0a2c52".into(),
            brand_primary: "#0d488a".into(),
            brand_accent: "#3b82f6".into(),
            text_heading: "#0f172a".into(),
            text_body: "#334155".into(),
            text_muted: "#64748b".into(),
            surface: "#f8fafc".into(),
            surface_raised: "#ffffff".into(),
            hairline: "#e2e8f0".into(),
            brand_name: "PlausiDen".into(),
            brand_suffix: "LLC".into(),
            tagline: "Plausible. Defensible.".into(),
            logo_letter: "P".into(),
            footer_org: "PlausiDen LLC".into(),
            footer_address: "Massachusetts, USA".into(),
            footer_website: "plausiden.com".into(),
            footer_email: "team@plausiden.com".into(),
        }
    }

    /// Stub theme for SacredVote infrastructure — neutral surface +
    /// civic-red accent. Tweak when the SacredVote tenant ships its
    /// own brand palette; until then this gives the operator a way
    /// to test multi-tenant rendering without falling back to the
    /// PlausiDen chrome.
    #[must_use]
    pub fn sacredvote() -> Self {
        Self {
            brand_navy: "#1f2937".into(),     // slate-800
            brand_primary: "#b91c1c".into(),  // red-700
            brand_accent: "#ef4444".into(),   // red-500
            text_heading: "#111827".into(),
            text_body: "#374151".into(),
            text_muted: "#6b7280".into(),
            surface: "#fafaf9".into(),
            surface_raised: "#ffffff".into(),
            hairline: "#e5e7eb".into(),
            brand_name: "SacredVote".into(),
            brand_suffix: "".into(),
            tagline: "Defending the ballot.".into(),
            logo_letter: "S".into(),
            footer_org: "SacredVote".into(),
            footer_address: "USA".into(),
            footer_website: "sacred.vote".into(),
            footer_email: "support@sacred.vote".into(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::plausiden()
    }
}

/// System-font stack used everywhere. No remote @font-face; Outlook
/// falls back to Calibri, iOS Mail to SF, Android Mail to Roboto.
pub(crate) const FONT_SANS: &str =
    "-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif";

/// Monospace stack used for code / DNS values.
pub(crate) const FONT_MONO: &str = "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plausiden_default_matches_legacy_tokens() {
        let t = Theme::plausiden();
        // Match the literal hex strings the renderer used pre-Theme;
        // any change here is a visual regression to the chrome.
        assert_eq!(t.brand_navy, "#0a2c52");
        assert_eq!(t.brand_primary, "#0d488a");
        assert_eq!(t.brand_accent, "#3b82f6");
        assert_eq!(t.tagline, "Plausible. Defensible.");
        assert_eq!(t.logo_letter, "P");
    }

    #[test]
    fn sacredvote_has_distinct_palette() {
        let p = Theme::plausiden();
        let s = Theme::sacredvote();
        assert_ne!(p.brand_primary, s.brand_primary);
        assert_ne!(p.brand_name, s.brand_name);
        assert_ne!(p.tagline, s.tagline);
    }

    #[test]
    fn theme_round_trips_serde() {
        let t = Theme::plausiden();
        let json = serde_json::to_string(&t).unwrap();
        let back: Theme = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}
