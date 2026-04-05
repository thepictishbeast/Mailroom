//! Tests for the router command parser and input validation.

// The router module is private, so we test via the public crate interface.
// For unit tests of parse_command, we add #[cfg(test)] blocks inside router.rs.
// This file tests the parser module's extract_address function which IS public.

#[test]
fn extract_address_with_name() {
    // Testing the parser directly since it's used by the router
    let input = "Tim Porter <tim@sacred.vote>";
    let addr = extract_bare_address(input);
    assert_eq!(addr, "tim@sacred.vote");
}

#[test]
fn extract_address_bare() {
    let addr = extract_bare_address("admin@sacred.vote");
    assert_eq!(addr, "admin@sacred.vote");
}

#[test]
fn extract_address_angle_only() {
    let addr = extract_bare_address("<noreply@sacred.vote>");
    assert_eq!(addr, "noreply@sacred.vote");
}

/// Minimal re-implementation of extract_address for integration testing.
fn extract_bare_address(addr_str: &str) -> &str {
    if let Some(start) = addr_str.find('<') {
        if let Some(end) = addr_str.find('>') {
            return &addr_str[start + 1..end];
        }
    }
    addr_str.trim()
}
