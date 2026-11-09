//! The sky texture layer on the authored component: defaults, saved-level
//! compatibility, independence from the procedural layers, the project
//! texture it names, orientation and blend.
use helio_pass_sky::AtmosphereRow;
use helio_pass_sky::sky_texture::{file_path, source};
use pulsar_reflection::TextureSrc;
use sky_component::AtmosphereComponent;

#[test]
fn defaults_and_saved_levels_keep_the_procedural_sky_and_no_texture() {
    let defaults = AtmosphereComponent::default();
    assert!(!defaults.sky_texture_enabled);
    let row = defaults.to_row();
    assert_eq!(row.sky_texture_source, source::NONE);
    assert_eq!(row.night_sky_enabled, 1);
    assert_eq!(row.star_count, AtmosphereRow::earth().star_count);
    // A level saved before the sky texture existed still loads.
    let old = serde_json::json!({ "night_sky_enabled": true, "star_count": 500.0 });
    let loaded: AtmosphereComponent = serde_json::from_value(old).unwrap();
    assert!(!loaded.sky_texture_enabled);
    assert_eq!(loaded.star_count, 500.0);
    assert!(loaded.sky_texture_file.is_empty());
}

#[test]
fn texture_and_procedural_layers_are_independent() {
    let both = AtmosphereComponent {
        sky_texture_enabled: true,
        sky_texture_file: TextureSrc::new(sky_path()),
        night_sky_enabled: true,
        shooting_stars_enabled: true,
        clouds_enabled: true,
        ..Default::default()
    };
    let row = both.to_row();
    assert_ne!(row.sky_texture_source & source::FILE, 0);
    assert_eq!(
        (
            row.night_sky_enabled,
            row.shooting_stars_enabled,
            row.clouds_enabled
        ),
        (1, 1, 1)
    );
    let no_stars = AtmosphereComponent {
        night_sky_enabled: false,
        ..both.clone()
    }
    .to_row();
    assert_eq!(no_stars.night_sky_enabled, 0);
    assert_ne!(no_stars.sky_texture_source & source::FILE, 0);
    let no_texture = AtmosphereComponent {
        sky_texture_enabled: false,
        ..both
    }
    .to_row();
    assert_eq!(
        (no_texture.night_sky_enabled, no_texture.sky_texture_source),
        (1, source::NONE)
    );
}

#[test]
fn orientation_and_blend_reach_the_row() {
    let mut sky = AtmosphereComponent {
        sky_texture_enabled: true,
        sky_texture_file: TextureSrc::new(sky_path()),
        ..Default::default()
    };
    sky.sky_texture_yaw_deg = 90.0;
    sky.sky_texture_pitch_deg = -30.0;
    sky.sky_texture_roll_deg = 15.0;
    sky.sky_texture_intensity = 3.0;
    sky.sky_texture_opacity = 2.0;
    let row = sky.to_row();
    assert!((row.sky_texture_yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    assert!((row.sky_texture_pitch + 30f32.to_radians()).abs() < 1e-6);
    assert!((row.sky_texture_roll - 15f32.to_radians()).abs() < 1e-6);
    assert_eq!(
        (row.sky_texture_intensity, row.sky_texture_opacity),
        (3.0, 1.0)
    );
}

#[test]
fn a_project_file_round_trips_and_no_file_shows_nothing() {
    let absolute = std::env::current_dir().unwrap().join("my_sky.exr");
    let sky = AtmosphereComponent {
        sky_texture_enabled: true,
        sky_texture_file: TextureSrc::new(absolute.to_string_lossy()),
        ..Default::default()
    };
    let row = sky.to_row();
    assert_ne!(row.sky_texture_source & source::FILE, 0);
    assert_eq!(file_path(row.sky_texture_source).unwrap(), absolute);
    assert_eq!(
        AtmosphereComponent::from_row(&row).sky_texture_file,
        sky.sky_texture_file
    );
    // None chosen: the procedural sky alone, even with the layer on.
    let empty = AtmosphereComponent {
        sky_texture_file: TextureSrc::default(),
        ..sky.clone()
    };
    assert_eq!(empty.to_row().sky_texture_source, source::NONE);
    // The layer off hides a chosen panorama.
    let off = AtmosphereComponent {
        sky_texture_enabled: false,
        ..sky
    };
    assert_eq!(off.to_row().sky_texture_source, source::NONE);
}

#[test]
fn an_asset_id_that_is_not_indexed_draws_the_procedural_sky() {
    // Without an open project nothing can resolve an asset id.
    let by_id = AtmosphereComponent {
        sky_texture_enabled: true,
        sky_texture_file: TextureSrc::from_ref(
            pulsar_reflection::TextureRef::parse("3f2c1a9e-7b4d-4e5a-9c1d-0a1b2c3d4e5f").unwrap(),
        ),
        ..Default::default()
    };
    assert_eq!(by_id.sky_texture_path(), None);
    assert_eq!(by_id.to_row().sky_texture_source, source::NONE);
}

#[test]
fn a_saved_path_and_a_saved_id_both_load() {
    let old = serde_json::json!({ "sky_texture_enabled": true, "sky_texture_file": "assets/sky/a.hdr" });
    let loaded: AtmosphereComponent = serde_json::from_value(old).unwrap();
    assert_eq!(loaded.sky_texture_file.legacy_path(), Some("assets/sky/a.hdr"));
    let id = "3f2c1a9e-7b4d-4e5a-9c1d-0a1b2c3d4e5f";
    let new = serde_json::json!({ "sky_texture_file": id });
    let loaded: AtmosphereComponent = serde_json::from_value(new).unwrap();
    assert_eq!(loaded.sky_texture_file.asset_ref().unwrap().as_str(), id);
}

fn sky_path() -> String {
    std::env::current_dir()
        .unwrap()
        .join("sky.hdr")
        .to_string_lossy()
        .into_owned()
}
