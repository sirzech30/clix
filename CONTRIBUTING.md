# Contributing to Clix

Thank you for your interest in contributing to Clix!

## Prerequisites

Install Rust and the GTK4 / libadwaita development headers for your distribution (see `README.md`).

## Development Workflow

1. Fork and create a feature branch from `main`.
2. Make focused, well-tested changes.
3. Run all quality checks before opening a pull request:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```
4. Open a pull request with a clear description of what changed and why.
