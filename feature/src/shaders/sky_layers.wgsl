// The sky's own layers beside the frame's sky (`frame_sky.wgsl` in
// `helio-frame-api`: the medium, the sky table, the suns): its clouds, the
// planet's weather, its night sky and its panorama, and where the wind has
// carried the clouds. The sky's resolve kernel writes them with the frame's
// medium from the same atmosphere row (`AtmosphereRow`: the medium, then
// these); only the sky's passes read them (`helio_pass_sky::SKY_LAYERS`,
// `//!use sky_layers`).

struct SkyLayers {
    // The cloud layer (`CloudLayer`): band above the surface (km), mean
    // coverage and form, extinction (per km), cumulus group / billow /
    // wisp sizes (km), wind (world m/s), albedo, erosion; then the planet's
    // weather (`PlanetWeather`): the pole, the circulation's strength and
    // cells, the synoptic systems' size (km) and storminess, the
    // mesoscale's size (km), the cirrus' amount and height (km), the
    // weather's evolution (time lapse) and seed; the representation.
    cloud_bottom: f32,
    cloud_top: f32,
    cloud_coverage: f32,
    cloud_type: f32,
    cloud_extinction: f32,
    cloud_weather_scale: f32,
    cloud_shape_scale: f32,
    cloud_detail_scale: f32,
    cloud_wind: vec3<f32>,
    clouds_enabled: u32,
    cloud_albedo: vec3<f32>,
    cloud_erosion: f32,
    cloud_pole: vec3<f32>,
    cloud_circulation: f32,
    cloud_circulation_cells: f32,
    cloud_synoptic_scale: f32,
    cloud_storminess: f32,
    cloud_mesoscale_scale: f32,
    cloud_cirrus: f32,
    cloud_cirrus_height: f32,
    cloud_evolution: f32,
    cloud_seed: u32,
    // How the clouds are drawn: 0 smooth, otherwise voxels of this side
    // (km), filled from density `cloud_voxel_fill`.
    cloud_voxel_size: f32,
    cloud_voxel_fill: f32,
    cloud_reserved: vec2<f32>,
    night_sky_enabled: u32,
    star_count: f32,
    star_brightness: f32,
    star_twinkle_fraction: f32,
    star_twinkle_speed: f32,
    star_seed: u32,
    shooting_stars_enabled: u32,
    shooting_star_rate: f32,
    shooting_star_speed: f32,
    shooting_star_length: f32,
    sky_texture_source: u32,
    sky_texture_intensity: f32,
    sky_texture_opacity: f32,
    sky_texture_yaw: f32,
    sky_texture_pitch: f32,
    sky_texture_roll: f32,
}

// An atmosphere row as the sky's component stores it: the frame's medium,
// then the sky's own parameters.
struct AtmosphereRow {
    medium: SkyMedium,
    layers: SkyLayers,
}

struct SkyLayersFrame {
    params: SkyLayers,
    // Where the wind has carried the clouds: offsets of the cumulus groups,
    // the mesoscale, billow and wisp noise in their own texture coordinates
    // (`resolve` integrates the wind each frame, wrapped to the noise's
    // period; the mesoscale's two lookups' in their own turned
    // coordinates). `cloud_groups_offset.w`: the planet's weather's phase in
    // its period (0..1, WEATHER_PERIOD_S); other w unused.
    cloud_groups_offset: vec4<f32>,
    cloud_meso_offset: vec4<f32>,
    cloud_meso_fine_offset: vec4<f32>,
    cloud_shape_offset: vec4<f32>,
    cloud_detail_offset: vec4<f32>,
}

// Whether the frame draws clouds.
fn sky_has_clouds(frame: SkyFrame, layers: SkyLayersFrame) -> bool {
    return frame.planet.w > 0.5 && layers.params.clouds_enabled != 0u
        && (layers.params.cloud_coverage > 0.0 || layers.params.cloud_cirrus > 0.0)
        && layers.params.cloud_extinction > 0.0 && layers.params.cloud_top > layers.params.cloud_bottom;
}

// The period the planet's weather evolves over (s): its systems' lives and
// drift repeat after it, so its phase wraps without a seam.
const WEATHER_PERIOD_S: f32 = 2592000.0;
// The mesoscale's two lookups (`clouds_common.wgsl`): turned apart (and
// from the groups' axes), the second CLOUD_MESO_RATIO times finer, so the
// noise never repeats in their sum.
const CLOUD_MESO_TURN_A: mat3x3<f32> = mat3x3<f32>(0.0, 0.8, 0.6, -0.8, 0.36, -0.48, -0.6, -0.48, 0.64);
const CLOUD_MESO_TURN_B: mat3x3<f32> = mat3x3<f32>(0.64, -0.6, 0.48, 0.48, 0.8, 0.36, -0.6, 0.0, 0.8);
const CLOUD_MESO_RATIO: f32 = 1.618034;
