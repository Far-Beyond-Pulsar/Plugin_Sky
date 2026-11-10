//! Sky (`pulsar.sky`): the plugin's root crate, the one place the plugin
//! registers everything it contributes.
//!
//! The plugin's other crates hold the code: the feature crate
//! (`helio-pass-sky`: the atmosphere's tables, the clouds and the sky
//! composite, publishing the frame's `sky`, `sun_visibility` and
//! `sun_transmittance`), `sky_component` (the atmosphere component, its row
//! and its row join) and `sky_editor` (the level editor UI). The sky has no
//! api crate: what other kinds read of it is the frame's. None of them
//! registers a contribution; this crate imports them and submits their
//! registrations, so this file and the two modules below are the whole list
//! of what the plugin brings:
//!
//! - [`runtime`]: what a game links. The component class, its scene rows,
//!   the Helio feature and the level rule (a level has one sky).
//! - `editor` (with the `editor` feature): what the editor adds. The Create
//!   menu's sky, the World Settings section and the Deep-Space panoramas as
//!   starter content.
//!
//! Every registration names this crate as its origin
//! (`pulsar_plugins::origin!()`), so the plugin gate skips all of them while
//! a project disables the plugin, and the plugin catalog lists them under
//! `pulsar.sky`. Other plugins never depend on this crate; hosts reach it
//! through the installed set or a generated game, a game without the
//! `editor` feature.

// The plugin's descriptor (`pulsar-plugin.toml`): linking this crate
// installs the plugin; a project enables it or not (`pulsar_plugins`).
pulsar_plugins::installed!("pulsar-plugin.toml");

pub use sky_component;
#[cfg(feature = "editor")]
pub use sky_editor;

pub mod runtime;

#[cfg(feature = "editor")]
pub mod editor;
