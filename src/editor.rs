//! What the sky plugin registers for the editor (the `editor` feature): its
//! level editor entries and its starter content (the Deep-Space panoramas).

use sky_editor::{level_editor, panoramas};
use ui_level_editor::extend::{CreateEntry, LevelComponentSection, StarterContent};

/// Everything the sky plugin registers for the editor.
pub(crate) fn build(p: &mut pulsar_plugins::PluginBuilder) {
    // ── Level editor ────────────────────────────────────────────────────────

    // The Create menu's sky (refused when the level has one).
    p.add(CreateEntry {
        origin: p.origin(),
        id: "sky",
        menu_label: Some(level_editor::menu_label),
        environment_label: None,
        quick_add: None,
        offered: None,
        order: 300,
        create: level_editor::create_sky,
    });

    // Tools > Starter Content: the CC0 Deep-Space panoramas, added to the
    // project's assets (no level object). The shell downloads the pinned
    // archive with progress and cancel and checks the SHA-256; the editor
    // crate extracts and writes.
    p.add(StarterContent {
        origin: p.origin(),
        id: "sky.deep_space_panoramas",
        title: "Sky Panoramas (CC0 Deep-Space)",
        description: "Galaxy and nebula equirect HDRs by Toolbench, for the Sky's Texture",
        license: "CC0 1.0",
        license_url: "https://creativecommons.org/publicdomain/zero/1.0/",
        archive_url: panoramas::ARCHIVE_URL,
        sha256: panoramas::ARCHIVE_SHA256,
        max_bytes: 256 << 20,
        destination: panoramas::PROJECT_DIR,
        order: 0,
        install: panoramas::install,
    });

    // World Settings' sky section: the level's sky, or a button that creates
    // it with a sun.
    p.add(LevelComponentSection {
        origin: p.origin(),
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
    });
}
