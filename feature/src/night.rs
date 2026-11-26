//! The night's light on the clouds.
//!
//! The moon is not the sky's: it is a directional light a level authors (a
//! `LightComponent` with an emitter disk), and the clouds sum every directional
//! light with its own direction and radiance. At the engine's night exposure
//! that sum alone leaves the clouds black: the authored moon is about a
//! eightieth of the sun's radiance (0.025 against 2), a cloud scatters a few
//! hundredths of it towards the eye, and the air's own night glow, which no
//! light carries, is nowhere in the sum. The row adds the two terms the clouds
//! lack, both gated by how dark the lights leave the sky ([`darkness`]), so a
//! lit day is unchanged to the bit:
//!
//! - the night sky's fill, [`AtmosphereRow::night_cloud_light`]: airglow and
//!   starlight, cool ([`CLOUD_FILL_TINT`]), an ambient in the sky table's unit
//!   (irradiance over pi) that reaches clouds the moon does not, thin parts
//!   and the sides turned from it alike;
//! - the moon's boost, [`AtmosphereRow::cloud_moon_boost`], in stops: a
//!   celestial body's light on the clouds scaled for the eye adapted to the
//!   night, which sees the clouds the authored moon would show in a long
//!   exposure. The moon's phase is its light's authored intensity (the disk
//!   already shades by the other lights' direction); its elevation is the
//!   direction the clouds are lit from, through the transmittance table.
//!
//! The functions here are the CPU twins of `sky_layers.wgsl` and
//! `clouds_common.wgsl`: `tests/night.rs` holds the constants to each other.

use crate::atmosphere::AtmosphereRow;

/// The tint of the night sky's fill (`CLOUD_NIGHT_TINT`), linear.
pub const CLOUD_FILL_TINT: [f32; 3] = [0.55, 0.72, 1.0];
/// Luminance weights of linear RGB (`SKY_LUMINANCE`).
pub const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];
/// Where a light's weight rises with its elevation's sine (`SKY_HORIZON_FADE`).
pub const HORIZON_FADE: [f32; 2] = [-0.05, 0.15];
/// The daylight that ends the night's darkness: dark below the first, day
/// above the second (`SKY_DARK_RANGE`). The sun is about 2, the moon 0.025.
pub const DARK_RANGE: [f32; 2] = [0.05, 0.6];
/// The night sky's cloud fill a new atmosphere starts with. Airglow and
/// starlight light a night at about a twentieth to a fiftieth of full
/// moonlight; against the authored moon (0.025) at its default boost
/// ([`DEFAULT_MOON_BOOST`], about 0.8 on the clouds) this is a fortieth.
pub const DEFAULT_CLOUD_LIGHT: f32 = 0.02;
/// The moon's boost a new atmosphere starts with, in stops.
pub const DEFAULT_MOON_BOOST: f32 = 5.0;

fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A directional light as the lighting sums it: the height of its direction
/// (the `y` of the unit vector towards it) and its linear RGB radiance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub toward_y: f32,
    pub radiance: [f32; 3],
}

/// How much the lights light the day (`resolve`'s `daylight`).
pub fn daylight(lights: &[Light]) -> f32 {
    lights
        .iter()
        .map(|l| {
            let luminance: f32 = l.radiance.iter().zip(LUMINANCE).map(|(r, w)| r * w).sum();
            luminance * smoothstep(HORIZON_FADE[0], HORIZON_FADE[1], l.toward_y)
        })
        .sum()
}

/// 1 when the lights leave the sky dark, 0 in full daylight
/// (`sky_darkness_of`).
pub fn darkness(daylight: f32) -> f32 {
    1.0 - smoothstep(DARK_RANGE[0], DARK_RANGE[1], daylight)
}

/// The night fill the clouds get, linear RGB (`CloudLayerParams::night_fill`).
pub fn cloud_fill(row: &AtmosphereRow, darkness: f32) -> [f32; 3] {
    CLOUD_FILL_TINT.map(|t| t * row.night_cloud_light.max(0.0) * darkness)
}

/// The factor a celestial body's light on the clouds is scaled by
/// (`CloudLayerParams::moon_gain`): 1 in daylight, `2^boost` in the dark.
pub fn moon_gain(row: &AtmosphereRow, darkness: f32) -> f32 {
    (row.cloud_moon_boost.max(0.0) * darkness).exp2()
}
