# Contributing

Thank you for your interest in contributing. This project is part of the PlausiDen ecosystem — tools that restore the presumption of innocence in the digital age.

## Development Setup

1. Install [Rust](https://rustup.rs/) (stable toolchain)
2. Install [Just](https://just.systems/) command runner
3. Clone the repository and run:

```bash
just check-all
```

## Workflow

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/your-feature`
3. Make your changes following the code standards below
4. Run `just check-all` to verify formatting, linting, tests, and build
5. Commit with a clear message explaining **why** (not just what)
6. Open a pull request against `main`

## Code Standards

- **Formatting:** `cargo fmt` (enforced by CI)
- **Linting:** `cargo clippy -- -D warnings` (zero warnings policy)
- **Testing:** Every public function must have at least one test
- **Documentation:** Every public item must have a `///` doc comment
- **Error handling:** Use `thiserror` for library errors, `anyhow` for application errors. No `unwrap()` in library code.
- **Security:** Use audited cryptographic crates only. No custom cryptography. All secret material must be zeroized after use.

## Issue Labels

- `good first issue` — Suitable for new contributors
- `help wanted` — Community input welcome
- `security` — Security-related changes (see SECURITY.md)
- `bug` — Something is broken
- `enhancement` — New feature or improvement

## Code of Conduct

This project follows the [Contributor Covenant](https://www.contributor-covenant.org/version/2/1/code_of_conduct/). Be respectful, constructive, and inclusive.

## Questions?

Open an issue or reach out to the maintainers.
