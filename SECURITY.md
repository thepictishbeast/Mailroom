# Security Policy

## Reporting Vulnerabilities

If you discover a security vulnerability in this project, please report it responsibly.

**Email:** security@sacredvote.org
**Expected response time:** 48 hours for acknowledgment, 7 days for initial assessment.

**Do NOT:**
- Open a public GitHub issue for security vulnerabilities
- Disclose the vulnerability publicly before it has been addressed
- Exploit the vulnerability beyond what is necessary to demonstrate it

## Scope

This security policy covers:
- All code in this repository
- Dependencies used by this project
- Configuration files and deployment scripts

## Supported Versions

Only the latest release on the `main` branch receives security updates.

## Security Practices

- All cryptographic operations use audited crates (ring, ed25519-dalek, chacha20poly1305, argon2)
- No custom cryptography
- All secret material is zeroized after use (zeroize crate)
- All user input is validated
- All SQL queries are parameterized
- Dependencies are audited with `cargo audit`
