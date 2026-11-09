//! Regression coverage for the sky texture layer's place in the composite
//! shader, and for the lights the sky keeps honouring.
use helio_pass_sky::sky_texture::{
    build_image, decode, direction_to_uv, file_path, file_source, rotation, source, MAX_WIDTH,
};
use std::path::{Path, PathBuf};

const COMPOSITE: &str = include_str!("../src/shaders/atmosphere_composite.wgsl");

fn body_of<'a>(source: &'a str, function: &str) -> &'a str {
    let start = source
        .find(function)
        .unwrap_or_else(|| panic!("{function} exists"));
    let rest = &source[start..];
    let end = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn composite_layers_the_panorama_under_the_procedural_sky() {
    let texture = COMPOSITE
        .find("sky_texture_radiance(d, pixel_angle)")
        .unwrap();
    let disks = COMPOSITE
        .find("radiance += directional_disk_radiance(d, through)")
        .unwrap();
    let stars = COMPOSITE.find("radiance += night_radiance(d, ").unwrap();
    assert!(
        texture < disks && disks < stars,
        "panorama, then emitter disks, then stars"
    );
    assert!(
        COMPOSITE.contains("radiance += sky_texture_radiance"),
        "additive: it never replaces the atmosphere"
    );
    // The panorama and the procedural stars are separate switches.
    assert!(!body_of(COMPOSITE, "fn sky_darkness").contains("night_sky_enabled"));
    assert!(!body_of(COMPOSITE, "fn sky_texture_radiance").contains("night_sky_enabled"));
    // It uses the wrapping sampler, not the clamped LUT sampler.
    assert!(body_of(COMPOSITE, "fn sky_texture_radiance").contains("sky_sampler"));
}

#[test]
fn composite_keeps_every_directional_light() {
    for function in [
        "fn march_from_space",
        "fn sky_daylight",
        "fn directional_disk_radiance",
        "fn emitter_disk_radiance",
    ] {
        assert!(
            body_of(COMPOSITE, function).contains("directional_light_count.x"),
            "{function} loops over all the lights"
        );
    }
    assert!(
        !COMPOSITE.contains("directional_lights[0]"),
        "no first-light-only lighting"
    );
}

#[test]
fn the_wgsl_orientation_matches_the_rust_reference() {
    let wgsl = body_of(COMPOSITE, "fn sky_texture_rotation");
    assert!(wgsl.contains("return ry * rx * rz;"));
    let sampled = body_of(COMPOSITE, "fn sky_texture_radiance");
    assert!(sampled.contains("transpose(sky_texture_rotation()) * d"));
    assert!(sampled.contains("atan2(t.x, -t.z)"));
    assert!(sampled.contains("0.5 - asin(clamp(t.y, -1.0, 1.0))"));
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
}

#[test]
fn equirect_mapping_covers_the_whole_sky() {
    let id = rotation(0.0, 0.0, 0.0);
    assert!(close(direction_to_uv(id, [0.0, 0.0, -1.0]), [0.5, 0.5]));
    // u is arbitrary at the poles; only the row matters there.
    assert!(direction_to_uv(id, [0.0, 1.0, 0.0])[1].abs() < 1e-4);
    assert!((direction_to_uv(id, [0.0, -1.0, 0.0])[1] - 1.0).abs() < 1e-4);
    assert!(close(direction_to_uv(id, [1.0, 0.0, 0.0]), [0.75, 0.5]));
    assert!(close(direction_to_uv(id, [-1.0, 0.0, 0.0]), [0.25, 0.5]));
}

#[test]
fn yaw_turns_the_panorama_about_the_vertical_axis() {
    let d = [0.0, 0.3, -1.0];
    let before = direction_to_uv(rotation(0.0, 0.0, 0.0), d);
    let after = direction_to_uv(rotation(std::f32::consts::FRAC_PI_2, 0.0, 0.0), d);
    assert!((before[1] - after[1]).abs() < 1e-5, "height is unchanged");
    let shift = (after[0] - before[0]).rem_euclid(1.0);
    assert!(
        (shift - 0.25).abs() < 1e-4 || (shift - 0.75).abs() < 1e-4,
        "a quarter turn moves u by a quarter: {shift}"
    );
}

#[test]
fn rotation_is_orthonormal() {
    let r = rotation(0.4, -0.7, 1.1);
    for a in 0..3 {
        for b in 0..3 {
            let dot: f32 = (0..3).map(|k| r[a][k] * r[b][k]).sum();
            assert!((dot - if a == b { 1.0 } else { 0.0 }).abs() < 1e-5);
        }
    }
}

#[test]
fn files_get_stable_distinct_codes() {
    let a = file_source(Path::new("sky_a.exr"));
    let b = file_source(Path::new("sky_b.hdr"));
    assert_ne!(a, b);
    assert_eq!(a, file_source(Path::new("sky_a.exr")));
    assert_ne!(a & source::FILE, 0);
    assert_eq!(file_path(a).unwrap(), PathBuf::from("sky_a.exr"));
    assert_eq!(file_source(Path::new("")), source::NONE);
    assert!(file_path(source::NONE).is_none());
}

#[test]
fn size_cap_and_mip_chain() {
    let image = build_image(
        2 * MAX_WIDTH,
        MAX_WIDTH,
        vec![0.5; (2 * MAX_WIDTH * MAX_WIDTH * 3) as usize],
    );
    assert_eq!((image.width, image.height), (MAX_WIDTH, MAX_WIDTH / 2));
    assert_eq!(image.mips.len(), 13);
    assert_eq!(image.mips[0].len() as u32, MAX_WIDTH * (MAX_WIDTH / 2) * 4);
    assert_eq!(half::f16::from_bits(image.mips[12][0]).to_f32(), 0.5);
}

/// Writes a small Radiance HDR panorama whose pixels are `value` into the
/// temp dir and returns its path.
fn write_hdr(name: &str, value: f32) -> PathBuf {
    use image::codecs::hdr::HdrEncoder;
    let (w, h) = (8usize, 4usize);
    let pixels = vec![image::Rgb([value, value * 0.5, value * 0.25]); w * h];
    let dir = std::env::temp_dir().join("pulsar_sky_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}_{name}.hdr", std::process::id()));
    let file = std::fs::File::create(&path).unwrap();
    HdrEncoder::new(std::io::BufWriter::new(file))
        .encode(&pixels, w, h)
        .unwrap();
    path
}

#[test]
fn a_project_hdr_decodes_as_linear_light_with_a_mip_chain() {
    let path = write_hdr("linear", 4.0);
    let image = decode(file_source(&path)).unwrap();
    assert_eq!((image.width, image.height), (8, 4));
    assert_eq!(image.mips.len(), 4);
    // HDR is scene-linear: no sRGB curve, values above 1 survive.
    let red = half::f16::from_bits(image.mips[0][0]).to_f32();
    assert!((red - 4.0).abs() < 0.1, "red was {red}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_missing_file_is_an_error_not_a_panic() {
    assert!(decode(file_source(Path::new("definitely/not/here.hdr"))).is_err());
    assert!(decode(source::NONE).is_err());
}

#[test]
fn night_follows_light_brightness_not_a_horizon_test() {
    let daylight = body_of(COMPOSITE, "fn sky_daylight");
    // Brightness (luminance) of every light, weighted by elevation.
    assert!(daylight.contains("luminance"));
    assert!(daylight.contains("daylight +="));
    assert!(!COMPOSITE.contains("sky_is_night"));
    // Stars and the texture share the same fade.
    assert!(COMPOSITE.contains("night_radiance(d, "));
    assert!(COMPOSITE.contains("* sky_darkness()"));
}
