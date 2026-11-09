//! What the sky plugin registers for the editor (the `editor` feature): its
//! level editor entries.

use sky_editor::level_editor;
use ui_level_editor::extend::{CreateEntry, LevelComponentSection};

// ── Level editor ────────────────────────────────────────────────────────────

// The Create menu's sky (refused when the level has one).
inventory::submit! {
    CreateEntry {
        origin: pulsar_plugins::origin!(),
        id: "sky",
        menu_label: Some(level_editor::menu_label),
        environment_label: None,
        quick_add: None,
        offered: None,
        order: 300,
        create: level_editor::create_sky,
    }
}

// The Create menu's "Sky Panoramas": adds the CC0 Deep-Space panoramas to
// the project's assets (no level object).
inventory::submit! {
    CreateEntry {
        origin: pulsar_plugins::origin!(),
        id: "sky_panoramas",
        menu_label: Some(level_editor::panoramas_menu_label),
        environment_label: None,
        quick_add: None,
        offered: None,
        order: 301,
        create: level_editor::add_sky_panoramas,
    }
}

// World Settings' sky section: the level's sky, or a button that creates it
// with a sun.
inventory::submit! {
    LevelComponentSection {
        origin: pulsar_plugins::origin!(),
        title: "Sky",
        sidebar_title: "Sky and Atmosphere",
        sidebar_missing: "Missing · creates a sky",
        order: 0,
        class_name: sky_component::ATMOSPHERE_CLASS,
        find: sky_component::level_atmosphere,
        missing_note: "This level has no sky, so it renders black.",
        create_label: "Create Sky",
        create: level_editor::create_sky_with_sun,
        describe: level_editor::describe,
    }
}
