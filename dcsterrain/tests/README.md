# Workspace integration tests

Tests that cross crate boundaries. They use synthetic theatres only: no
extracted or packed DCS data is committed, and nothing here reads a file from
a DCS install. A test that fits inside one crate lives in that crate instead.

The workspace root is a virtual manifest, which cannot own a `tests/`
directory, so this one is inert until the first test arrives. Adding that test
means adding a package stanza to `dcsterrain/Cargo.toml` alongside the
workspace stanza:

```toml
[package]
name = "dcsterrain-tests"
version.workspace = true
edition.workspace = true
publish = false
autolib = false
autobins = false
```

Without it `cargo test` silently runs nothing here.
