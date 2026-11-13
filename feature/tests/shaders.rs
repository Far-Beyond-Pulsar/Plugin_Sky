//! Every sky shader, with the snippets its passes build it with, parses and
//! validates (naga), so a WGSL mistake fails here rather than on a device.
use helio_frame_api::lighting::sun_transmittance::SNIPPET as SUN_TRANSMITTANCE_SNIPPET;
use helio_pass_sky::atmosphere::{SKY_LAYERS_SNIPPET, SKY_SNIPPET, SKY_TABLE_SNIPPET};
use helio_pass_sky::clouds::CLOUDS_SNIPPET;

fn validate(label: &str, source: &str) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{label}: {}", e.emit_to_string(source)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{label}: {}", e.emit_to_string(source)));
}

#[test]
fn sky_shaders_validate() {
    let snippets = [
        SKY_SNIPPET,
        SKY_LAYERS_SNIPPET,
        CLOUDS_SNIPPET,
        SKY_TABLE_SNIPPET,
        SUN_TRANSMITTANCE_SNIPPET,
        helio_light_api::LIGHT_LIST_SNIPPET,
    ];
    for (label, source) in [
        (
            "atmosphere_luts",
            include_str!("../src/shaders/atmosphere_luts.wgsl"),
        ),
        (
            "atmosphere_composite",
            include_str!("../src/shaders/atmosphere_composite.wgsl"),
        ),
        (
            "cloud_noise",
            include_str!("../src/shaders/cloud_noise.wgsl"),
        ),
        (
            "cloud_planet_weather",
            include_str!("../src/shaders/cloud_planet_weather.wgsl"),
        ),
        (
            "cloud_shadow_map",
            include_str!("../src/shaders/cloud_shadow_map.wgsl"),
        ),
        (
            "cloud_trace",
            include_str!("../src/shaders/cloud_trace.wgsl"),
        ),
    ] {
        validate(label, &helio_core::shader::resolve_with(source, &snippets));
    }
}

/// Byte size and member offsets of struct `name` in `source` (naga's WGSL
/// layout).
fn layout(source: &str, name: &str) -> (u64, Vec<(String, u32)>) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    let mut layouter = naga::proc::Layouter::default();
    layouter
        .update(module.to_ctx())
        .expect("the module lays out");
    let (handle, ty) = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no struct `{name}`"));
    let members = match &ty.inner {
        naga::TypeInner::Struct { members, .. } => members
            .iter()
            .map(|m| (m.name.clone().unwrap_or_default(), m.offset))
            .collect(),
        _ => Vec::new(),
    };
    (u64::from(layouter[handle].size), members)
}

/// The sky's row is the frame's medium then the sky's own layers, byte for
/// byte as the component stores it; its resolved layers have the size the
/// passes bind.
#[test]
fn the_atmosphere_row_is_the_medium_then_the_layers() {
    use helio_pass_sky::atmosphere::{
        AtmosphereRow, SKY_LAYERS_BYTES, SKY_LAYERS_PARAMS_BYTES, SKY_MEDIUM_BYTES,
    };
    let source = helio_core::shader::resolve_with(
        "//!use frame_sky\n//!use sky_layers\n\
         @group(0) @binding(0) var<storage, read> rows: array<AtmosphereRow>;\n\
         @group(0) @binding(1) var<storage, read> layers: SkyLayersFrame;\n",
        &[SKY_SNIPPET, SKY_LAYERS_SNIPPET],
    );
    let (row, row_members) = layout(&source, "AtmosphereRow");
    assert_eq!(row, std::mem::size_of::<AtmosphereRow>() as u64);
    assert_eq!(
        row_members,
        vec![
            ("medium".to_string(), 0),
            ("layers".to_string(), SKY_MEDIUM_BYTES as u32),
        ]
    );
    assert_eq!(layout(&source, "SkyMedium").0, SKY_MEDIUM_BYTES);
    assert_eq!(layout(&source, "SkyLayers").0, SKY_LAYERS_PARAMS_BYTES);
    assert_eq!(layout(&source, "SkyLayersFrame").0, SKY_LAYERS_BYTES);
    // The Rust row's fields where the WGSL puts them.
    let medium = layout(&source, "SkyMedium").1;
    let layers = layout(&source, "SkyLayers").1;
    let at = |members: &[(String, u32)], base: usize, name: &str| {
        base + members.iter().find(|(n, _)| n == name).expect(name).1 as usize
    };
    assert_eq!(
        std::mem::offset_of!(AtmosphereRow, enabled),
        at(&medium, 0, "enabled")
    );
    assert_eq!(
        std::mem::offset_of!(AtmosphereRow, cloud_bottom),
        at(&layers, SKY_MEDIUM_BYTES as usize, "cloud_bottom")
    );
    assert_eq!(
        std::mem::offset_of!(AtmosphereRow, star_seed),
        at(&layers, SKY_MEDIUM_BYTES as usize, "star_seed")
    );
    assert_eq!(
        std::mem::offset_of!(AtmosphereRow, sky_texture_roll),
        at(&layers, SKY_MEDIUM_BYTES as usize, "sky_texture_roll")
    );
}
