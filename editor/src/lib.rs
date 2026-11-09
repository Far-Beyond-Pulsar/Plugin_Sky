//! Sky (`pulsar.sky`): the editor crate -- the level editor's sky entry and
//! World Settings section ([`level_editor`]). It registers nothing itself:
//! the plugin's root crate (`sky_plugin`, with its `editor` feature)
//! submits these through the level editor's extension points. World
//! Settings and the inspector edit the component through its reflected
//! properties.

// The level editor UI's text (`locales/`).
rust_i18n::i18n!("locales", fallback = "en");

pub mod level_editor;

/// Adding the CC0 Deep-Space sky panoramas to a project.
pub mod panoramas;
