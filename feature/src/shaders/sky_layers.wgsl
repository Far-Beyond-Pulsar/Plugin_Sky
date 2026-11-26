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
    // The night sky's light on the clouds (linear radiance of the airglow
    // and starlight fill) and the stops brighter a celestial body lights them
    // in the dark than its authored light does (`night.rs`).
    night_cloud_light: f32,
    cloud_moon_boost: f32,
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
    // x: how dark the lights leave the sky, 0 in daylight to 1 in the dark
    // (`resolve` folds every directional light into it, once a frame): the
    // stars, the panorama and the clouds' night light fade by it. yzw unused.
    night: vec4<f32>,
}

// How much the scene's directional lights light the day: each light's
// luminance, scaled by how far above the horizon it stands (the weight rises
// from SKY_HORIZON_FADE.x to .y of its elevation's sine), summed over all of
// them. The sun (~2) is daylight; a dim moon light (0.025) is not, whether or
// not it is up. Light below the horizon contributes nothing. `night.rs` is its
// CPU twin.
const SKY_LUMINANCE = vec3<f32>(0.2126, 0.7152, 0.0722);
const SKY_HORIZON_FADE = vec2<f32>(-0.05, 0.15);
// 1 when `daylight` leaves the sky dark (below SKY_DARK_RANGE.x), 0 in full
// daylight (above .y), a smooth twilight between.
const SKY_DARK_RANGE = vec2<f32>(0.05, 0.6);

fn sky_darkness_of(daylight: f32) -> f32 {
    return 1.0 - smoothstep(SKY_DARK_RANGE.x, SKY_DARK_RANGE.y, daylight);
}

// The tint of the night sky's light on the clouds: airglow and starlight, cool.
const CLOUD_NIGHT_TINT = vec3<f32>(0.55, 0.72, 1.0);

// A directional light's emitter words (`DirectionalLight::disk`: angular
// radius, brightness, kind, enabled): the kinds of disk a light draws in the
// sky as a body of its own (the moon), as the lights' own flare code writes them.
const SOLID_EMITTER_DISK: u32 = 253u;
const TEXTURED_EMITTER_DISK: u32 = 254u;

// Whether a light is a celestial body: it draws its own disk in the sky, so
// the sun's disk is not drawn for it, and the clouds light it by the night's
// eye (`cloud_moon_boost`).
fn sky_is_celestial(emitter: vec4<f32>) -> bool {
    let kind = u32(emitter.z);
    return emitter.w > 0.5 && (kind == SOLID_EMITTER_DISK || kind == TEXTURED_EMITTER_DISK);
}

// What `resolve` decided for the frame, on the GPU, where the row is read: the
// kernels that only need to run when the authored atmosphere changed are
// dispatched indirectly with arguments of zero groups otherwise, and the
// planet weather's bake reads `rebuilt` to take its whole map or a slice.
// Nothing reads it back on the CPU.
struct SkyPlan {
    // Hash of the atmosphere row `resolve` last found (never 0; 0: none yet,
    // so the next row found counts as changed).
    row_hash: u32,
    // 1 on the frame the row's hash differs from `row_hash`'s.
    rebuilt: u32,
    _pad: vec2<u32>,
    // Indirect dispatch arguments (x, y, z, unused), by `SKY_PLAN_*` slot
    // (atmosphere.rs): the tables built from the row's parameters alone and
    // the planet weather's bake.
    args: array<vec4<u32>, 5>,
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
