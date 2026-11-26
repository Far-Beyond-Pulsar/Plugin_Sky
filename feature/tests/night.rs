//! The night's light on the clouds (`night.rs`): the CPU twins' values, and
//! the WGSL constants they stand for.
use helio_pass_sky::atmosphere::AtmosphereRow;
use helio_pass_sky::night::{
    cloud_fill, darkness, daylight, moon_gain, Light, CLOUD_FILL_TINT, DARK_RANGE,
    DEFAULT_CLOUD_LIGHT, DEFAULT_MOON_BOOST, HORIZON_FADE, LUMINANCE,
};

const LAYERS: &str = include_str!("../src/shaders/sky_layers.wgsl");
const COMMON: &str = include_str!("../src/shaders/clouds_common.wgsl");
const TRACE: &str = include_str!("../src/shaders/cloud_trace.wgsl");

/// The numbers of WGSL `const NAME = vec<N><f32>(a, b, ..);`.
fn wgsl_vector(name: &str) -> Vec<f32> {
    let at = LAYERS
        .find(&format!("const {name} = "))
        .unwrap_or_else(|| panic!("const {name}"));
    let rest = &LAYERS[at..];
    let open = rest.find('(').expect("(") + 1;
    let close = rest.find(')').expect(")");
    rest[open..close]
        .split(',')
        .map(|n| n.trim().parse().expect("a float"))
        .collect()
}

const SUN: Light = Light {
    toward_y: 0.6,
    radiance: [2.0; 3],
};
const MOON: Light = Light {
    toward_y: 0.5,
    radiance: [0.0170, 0.019, 0.025],
};

#[test]
fn the_wgsl_constants_are_the_cpu_twins() {
    assert_eq!(wgsl_vector("CLOUD_NIGHT_TINT"), CLOUD_FILL_TINT);
    assert_eq!(wgsl_vector("SKY_LUMINANCE"), LUMINANCE);
    assert_eq!(wgsl_vector("SKY_HORIZON_FADE"), HORIZON_FADE);
    assert_eq!(wgsl_vector("SKY_DARK_RANGE"), DARK_RANGE);
}

#[test]
fn a_new_atmospheres_defaults_are_the_documented_ones() {
    let row = AtmosphereRow::earth();
    assert_eq!(row.night_cloud_light, DEFAULT_CLOUD_LIGHT);
    assert_eq!(row.cloud_moon_boost, DEFAULT_MOON_BOOST);
    // A zeroed row (a host that knows nothing of the night) changes nothing.
    let zero: AtmosphereRow = bytemuck::Zeroable::zeroed();
    assert_eq!(cloud_fill(&zero, 1.0), [0.0; 3]);
    assert_eq!(moon_gain(&zero, 1.0), 1.0);
}

#[test]
fn the_sun_is_day_and_the_moon_alone_is_night() {
    assert_eq!(darkness(daylight(&[SUN, MOON])), 0.0);
    // The moon alone, however high, is dark; so is the sun below the horizon.
    assert_eq!(darkness(daylight(&[MOON])), 1.0);
    let set_sun = Light {
        toward_y: -0.2,
        ..SUN
    };
    assert_eq!(darkness(daylight(&[set_sun, MOON])), 1.0);
    // Twilight is between, and monotonic as the sun rises.
    let dusk: Vec<f32> = [0.0f32, 0.02, 0.05, 0.1, 0.15]
        .iter()
        .map(|&y| darkness(daylight(&[Light { toward_y: y, ..SUN }])))
        .collect();
    assert!(dusk.windows(2).all(|w| w[0] >= w[1]), "{dusk:?}");
    assert!(dusk[0] > 0.0 && dusk[4] < 1.0);
}

#[test]
fn daylight_leaves_the_clouds_lighting_untouched() {
    let row = AtmosphereRow::earth();
    assert_eq!(cloud_fill(&row, 0.0), [0.0; 3]);
    assert_eq!(moon_gain(&row, 0.0), 1.0);
}

#[test]
fn the_night_fills_clouds_cool_and_boosts_the_moon_by_its_stops() {
    let row = AtmosphereRow::earth();
    let fill = cloud_fill(&row, 1.0);
    assert!(fill[2] > fill[1] && fill[1] > fill[0] && fill[0] > 0.0, "{fill:?}");
    assert!((fill[2] - DEFAULT_CLOUD_LIGHT).abs() < 1e-7);
    assert!((moon_gain(&row, 1.0) - DEFAULT_MOON_BOOST.exp2()).abs() < 1e-5);
    // Halfway into the dark is halfway in stops.
    assert!((moon_gain(&row, 0.5) - (0.5 * DEFAULT_MOON_BOOST).exp2()).abs() < 1e-5);
}

#[test]
fn the_clouds_read_the_night_where_the_wgsl_has_it() {
    assert!(COMMON.contains("c.night_fill = CLOUD_NIGHT_TINT * max(l.night_cloud_light, 0.0) * dark;"));
    assert!(COMMON.contains("c.moon_gain = exp2(max(l.cloud_moon_boost, 0.0) * dark);"));
    // Both the volume and the cirrus add the fill and boost the celestial lights.
    assert_eq!(TRACE.matches("c.night_fill").count(), 2);
    assert_eq!(TRACE.matches("sky_is_celestial(").count(), 2);
}
