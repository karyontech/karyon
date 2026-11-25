# karyon eventemitter macro

Procedural macro crate of [karyon_eventemitter](https://crates.io/crates/karyon_eventemitter). It
provides the `#[derive(EventValue)]` macro, which implements `AsEventValue` with the type name as
the event id, or the name given in `#[event_id("name")]`.

Do not depend on this crate directly. Use karyon_eventemitter, which re-exports the derive under its
default `derive` feature.
