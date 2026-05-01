//! Inline-style design tokens for the email chrome.
//!
//! Email clients strip `<style>` blocks (Gmail), inline only `<head>`
//! styles (Outlook on Windows), or override colors in dark mode
//! unpredictably (iOS Mail). The defensive answer is to inline every
//! style as an attribute on the element that needs it. These
//! constants keep the tokens grep-visible and synced with
//! PlausiDen-Loom's color palette.

pub(crate) const BRAND_NAVY: &str = "#0a2c52";
pub(crate) const BRAND_PRIMARY: &str = "#0d488a";
pub(crate) const BRAND_ACCENT: &str = "#3b82f6";
pub(crate) const TEXT_HEADING: &str = "#0f172a";
pub(crate) const TEXT_BODY: &str = "#334155";
pub(crate) const TEXT_MUTED: &str = "#64748b";
pub(crate) const SURFACE: &str = "#f8fafc";
pub(crate) const SURFACE_RAISED: &str = "#ffffff";
pub(crate) const HAIRLINE: &str = "#e2e8f0";

/// System-font stack used everywhere. No remote @font-face; Outlook
/// falls back to Calibri, iOS Mail to SF, Android Mail to Roboto.
pub(crate) const FONT_SANS: &str =
    "-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif";

/// Monospace stack used for code / DNS values.
pub(crate) const FONT_MONO: &str = "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace";
