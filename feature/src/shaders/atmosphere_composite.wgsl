//!use helio_prelude
//!use atmosphere
// Atmosphere composite over the lit scene (`pre_aa`), blended as
// dst * alpha + src: sky pixels (no geometry) are replaced by the sky and the
// sun's disk (alpha 0); geometry keeps its light attenuated by the air in
// front of it plus the light the air scatters towards the eye (aerial
// perspective, alpha = transmittance). Without an atmosphere it writes
// (0, 1): the scene is unchanged.
//
// The clouds (`cloud_trace.wgsl`, half resolution) lie in front of
// whatever is behind them: the sky, or geometry farther than their front.
// With the air in front of them already applied, the pixel becomes
// behind * (1 - opacity) + clouds.
//
// Inside the atmosphere the sky comes from the sky-view LUT and the air in
// front of geometry from the aerial-perspective volume. From space both
// would spend their resolution on empty space around a thin shell (and the
// volume would not reach the planet), so each pixel marches its own ray
// through the shell instead.

@group(0) @binding(0) var<storage, read> cameras: array<Camera, 2>;
@group(0) @binding(1) var<storage, read> frame: AtmosphereFrame;
@group(0) @binding(2) var transmittance_lut: texture_2d<f32>;
@group(0) @binding(3) var sky_view_lut: texture_2d<f32>;
@group(0) @binding(4) var aerial_lut: texture_3d<f32>;
@group(0) @binding(5) var depth_texture: texture_depth_2d;
@group(0) @binding(6) var lut_sampler: sampler;
@group(0) @binding(7) var multi_scattering_lut: texture_2d<f32>;
@group(0) @binding(8) var cloud_color: texture_2d<f32>;
@group(0) @binding(9) var cloud_depth: texture_2d<f32>;
@group(0) @binding(10) var moon_albedo: texture_2d<f32>;
// The sky texture layer: an equirectangular panorama (linear light, mipped)
// and its sampler (repeats around the horizon, clamps at the poles).
@group(0) @binding(11) var sky_texture: texture_2d<f32>;
@group(0) @binding(12) var sky_sampler: sampler;

const SOLID_EMITTER_DISK: u32 = 253u;
const TEXTURED_EMITTER_DISK: u32 = 254u;

// A cloud footprint reaching the sky (`cloud_trace.wgsl`), km.
const CLOUD_UNLIMITED: f32 = 60000.0;

const SPACE_STEPS: f32 = 32.0;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) v: u32) -> VertexOut {
    let p = vec2<f32>(f32((v << 1u) & 2u), f32(v & 2u));
    var out: VertexOut;
    out.position = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}

fn transmittance(r: f32, mu: f32) -> vec3<f32> {
    return textureSampleLevel(transmittance_lut, lut_sampler, transmittance_uv(frame.params, r, mu), 0.0).rgb;
}

// The air along `d` from the camera, marched per pixel: up to geometry
// `limit` km away, or (sky pixels, `limit` infinite) to the analytic ground,
// whose sunlit albedo it then includes.
fn march_from_space(eye: vec3<f32>, d: vec3<f32>, limit: f32) -> Scattering {
    let p = frame.params;
    let segment = atmosphere_segment(p, eye, d, limit > 1.0e37);
    let end = min(segment.y, limit);
    var out: Scattering;
    out.radiance = vec3<f32>(0.0);
    out.transmittance = vec3<f32>(1.0);
    if end <= segment.x { return out; }
    for (var i = 0u; i < frame.directional_light_count.x; i++) {
        let base = i * 3u;
        let light_dir = frame.directional_lights[base].xyz;
        let energy = frame.directional_lights[base + 1u].rgb;
        let s = atmosphere_integrate(p, light_dir, transmittance_lut, multi_scattering_lut, lut_sampler,
            eye, d, segment.x, end, SPACE_STEPS, false);
        out.radiance += s.radiance * energy;
        if i == 0u { out.transmittance = s.transmittance; }
        if segment.z > 0.5 && limit >= segment.y {
            let n = normalize(eye + d * segment.y);
            let mu_s = dot(n, light_dir);
            out.radiance += s.transmittance * energy
                * atmosphere_sun_transmittance_lut(p, transmittance_lut, lut_sampler, p.bottom_radius, mu_s)
                * max(mu_s, 0.0) * p.ground_albedo / ATMOSPHERE_PI;
        }
    }
    return out;
}

// The clouds in front of a pixel at `uv` whose geometry is `distance` km
// away (CLOUD_UNLIMITED: sky): rgb their light, a their opacity. Of the four
// half-resolution texels around it, those whose footprint reached at least
// as far count (the others stopped at nearer geometry and saw less sky);
// a texel's clouds count only where their front is nearer than the
// geometry.
fn clouds_in_front(uv: vec2<f32>, distance: f32) -> vec4<f32> {
    if !atmosphere_has_clouds(frame) { return vec4<f32>(0.0); }
    let size = vec2<i32>(textureDimensions(cloud_color));
    let at = uv * vec2<f32>(size) - 0.5;
    let base = vec2<i32>(floor(at));
    let f = at - floor(at);
    // Geometry within this of the clouds' front is partly in front of them:
    // a sliver of the layer's thickness, so terrain under a cloud seen from
    // orbit (2 km below it, 400 km away) stays behind it.
    let thickness = frame.params.cloud_top - frame.params.cloud_bottom;
    var matched = vec4<f32>(0.0);
    var matched_weight = 0.0;
    var unmatched = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let o = vec2<i32>(k & 1, k >> 1);
        let q = clamp(base + o, vec2<i32>(0), size - 1);
        let weight = select(1.0 - f.x, f.x, o.x == 1) * select(1.0 - f.y, f.y, o.y == 1);
        let depth = textureLoad(cloud_depth, q, 0);
        var clouds = textureLoad(cloud_color, q, 0);
        if distance < CLOUD_UNLIMITED * 0.98 {
            let band = clamp(depth.x * 0.01, 0.005, 0.25 * thickness);
            clouds *= clamp((distance - depth.x) / band, 0.0, 1.0);
        }
        unmatched += clouds * weight;
        if depth.y >= min(distance, CLOUD_UNLIMITED) * 0.98 {
            matched += clouds * weight;
            matched_weight += weight;
        }
    }
    if matched_weight > 1e-3 { return matched / matched_weight; }
    return unmatched;
}

// How much the scene's directional lights light the day: each light's
// luminance, scaled by how far above the horizon it stands, summed over all of
// them. The sun (~2) is daylight; a dim moon light (0.025) is not, whether or
// not it is up. Light below the horizon contributes nothing.
fn sky_daylight() -> f32 {
    var daylight = 0.0;
    for (var i = 0u; i < frame.directional_light_count.x; i++) {
        let base = i * 3u;
        let luminance = dot(frame.directional_lights[base + 1u].rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        daylight += luminance * smoothstep(-0.05, 0.15, frame.directional_lights[base].y);
    }
    return daylight;
}

// 1 when the lights leave the sky dark, 0 in full daylight, with a smooth
// twilight between. The stars, shooting stars and the sky texture all fade by
// this, so they follow the lights' brightness rather than a horizon test.
fn sky_darkness() -> f32 {
    return 1.0 - smoothstep(0.05, 0.6, sky_daylight());
}

fn directional_disk_radiance(d: vec3<f32>, through: vec3<f32>) -> vec3<f32> {
    var radiance = vec3<f32>(0.0);
    let radius = max(frame.params.sun_angular_radius, 1e-5);
    let solid_angle = max(2.0 * ATMOSPHERE_PI * (1.0 - cos(radius)), 1e-8);
    for (var i = 0u; i < frame.directional_light_count.x; i++) {
        let base = i * 3u;
        let emitter = frame.directional_lights[base + 2u];
        let kind = u32(emitter.z);
        if emitter.w > 0.5 && (kind == SOLID_EMITTER_DISK || kind == TEXTURED_EMITTER_DISK) {
            continue;
        }
        let light_dir = frame.directional_lights[base].xyz;
        if dot(d, light_dir) <= cos(radius) { continue; }
        let energy = frame.directional_lights[base + 1u].rgb;
        let edge = clamp((dot(d, light_dir) - cos(radius)) / max(1.0 - cos(radius), 1e-7), 0.0, 1.0);
        radiance += energy / solid_angle * (0.4 + 0.6 * sqrt(edge)) * through;
    }
    return radiance;
}

fn hash2(p: vec2<f32>, seed: u32) -> vec3<f32> {
    let h = vec3<f32>(
        dot(p, vec2<f32>(127.1, 311.7)) + f32(seed) * 0.013,
        dot(p, vec2<f32>(269.5, 183.3)) + f32(seed) * 0.017,
        dot(p, vec2<f32>(419.2, 371.9)) + f32(seed) * 0.019);
    return fract(sin(h) * 43758.5453);
}

// The night sky's stars, by a star's hash `h` (0..1): palette (`h` 0 hot
// blue-white to 1 cool orange), true angular radius, and peak brightness
// relative to `star_brightness`. Stars, constellation stars and shooting
// stars' heads all use these, so they are one population.
fn star_color(h: f32) -> vec3<f32> {
    return mix(vec3<f32>(0.62, 0.76, 1.0), vec3<f32>(1.0, 0.82, 0.62), h);
}

fn star_radius(h: f32) -> f32 {
    return mix(0.00016, 0.00072, pow(h, 3.0));
}

fn star_peak(h: f32) -> f32 {
    return mix(0.35, 1.0, h);
}

// The light of a point source `angular` radians from its centre: 1 there for
// a source drawn at its true size. A source is never drawn thinner than about
// a pixel (`pixel_angle`), its light spread to match over `dims` dimensions
// (2 for a star, 1 across a shooting star's train), or sampling a sub-pixel
// source would make it blink between pixels as the camera moves.
fn point_light(angular: f32, true_radius: f32, pixel_angle: f32, dims: f32) -> f32 {
    let radius = max(true_radius, pixel_angle * 0.9);
    if angular >= radius { return 0.0; }
    let core = 1.0 - smoothstep(radius * 0.12, radius, angular);
    let halo = exp(-angular * angular / max(radius * radius * 4.0, 1e-9)) * 0.22;
    return (core + halo) * pow(true_radius / radius, dims);
}

// The light across a hairline `angular` radians from its centre: a tent at
// most a pixel wide (`pixel_angle`) with no halo, 1 at the centre for a line
// drawn at its true half-width. A thinner line keeps its light per unit length
// (the tent's area): it dims rather than thickens.
fn hairline(angular: f32, true_half_width: f32, pixel_angle: f32) -> f32 {
    let half = max(true_half_width, pixel_angle * 0.5);
    return max(1.0 - angular / half, 0.0) * (true_half_width / half);
}

// Shooting stars. The head is one of the night's stars, drawn exactly as the
// others, crossing a short great-circle path. On top of it, each optional
// (0 turns it off): a flare at the burn peak (`shooting_star_flare`), a train
// behind it (`shooting_star_trail`, reaching `shooting_star_length`) and the
// train's afterglow once the head is gone (`shooting_star_afterglow`), tinted
// toward white by `shooting_star_heat`.
//
// Sporadic meteors last a fraction of a second and cross 5 to 25 degrees of
// sky. Durations are in seconds at `shooting_star_speed` 1; paths in radians.
const METEOR_DURATION = vec2<f32>(0.35, 1.1);
const METEOR_PATH = vec2<f32>(0.09, 0.44);
// The train has e^-METEOR_TRAIN_FADE of the head's light `shooting_star_length`
// behind it; the afterglow is drawn for METEOR_AFTERGLOW_LIFE of its decay
// times.
const METEOR_TRAIN_FADE: f32 = 2.5;
const METEOR_AFTERGLOW_LIFE: f32 = 4.0;
// Candidate shooting stars per time slot; the most slots one's life spans.
const METEOR_CANDIDATES: u32 = 4u;
const METEOR_SLOTS: f32 = 4.0;
// Slot ids wrap so the hash's inputs stay small enough to stay random.
const METEOR_SLOT_WRAP: f32 = 1024.0;

// The flare over a shooting star's life (`t` 0..1): it brightens as it falls
// into denser air, peaks late, then burns out quickly.
fn meteor_light_curve(t: f32) -> f32 {
    return smoothstep(0.0, 0.7, t) * (1.0 - smoothstep(0.7, 1.0, t));
}

// The shooting stars along `d`. Arrivals are a Poisson process at
// `shooting_star_rate` a minute: time is cut into slots no shorter than a
// quarter of the longest life, and each slot holds METEOR_CANDIDATES that each
// exist with the chance that gives the rate. So several can share the sky, at
// irregular times, and each pixel tests a bounded few.
fn meteor_radiance(d: vec3<f32>, pixel_angle: f32) -> vec3<f32> {
    let p = frame.params;
    let speed = max(p.shooting_star_speed, 0.05);
    let train = max(p.shooting_star_length, 0.005);
    let afterglow = max(p.shooting_star_afterglow, 0.0) / speed;
    let longest = METEOR_DURATION.y / speed + METEOR_AFTERGLOW_LIFE * afterglow;
    let slot_length = max(longest / METEOR_SLOTS, min(longest, 60.0 / max(p.shooting_star_rate, 1e-3)));
    let chance = p.shooting_star_rate * slot_length / (60.0 * f32(METEOR_CANDIDATES));
    let back = min(ceil(longest / slot_length), METEOR_SLOTS);
    let now = frame.cloud_meso_offset.w;
    let current = floor(now / slot_length);
    var result = vec3<f32>(0.0);
    for (var b = 0.0; b <= back; b += 1.0) {
        let slot = current - b;
        let key = slot - floor(slot / METEOR_SLOT_WRAP) * METEOR_SLOT_WRAP;
        for (var k = 0u; k < METEOR_CANDIDATES; k++) {
            let seed = p.star_seed + 991u + k * 7919u;
            let when = hash2(vec2<f32>(key, 17.3), seed);
            if when.x >= chance { continue; }
            let age = now - (slot + when.y) * slot_length;
            let duration = mix(METEOR_DURATION.x, METEOR_DURATION.y, when.z) / speed;
            if age < 0.0 || age > duration + METEOR_AFTERGLOW_LIFE * afterglow { continue; }
            let shape = hash2(vec2<f32>(key, 53.9), seed);
            let path = mix(METEOR_PATH.x, METEOR_PATH.y, shape.x);
            // It starts anywhere from 8 to 80 degrees up, uniform over the sky.
            let y = mix(0.14, 0.98, shape.y);
            let azimuth = shape.z * 6.2831853;
            let start = vec3<f32>(sqrt(1.0 - y * y) * sin(azimuth), y, sqrt(1.0 - y * y) * cos(azimuth));
            if dot(d, start) < cos(path + 0.01) { continue; }
            // It heads any way along the sky, mostly down: meteors radiate
            // from radiants above the horizon.
            let look = hash2(vec2<f32>(key, 91.1), seed);
            let east = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), start));
            let north = cross(start, east);
            let heading = look.x * 6.2831853;
            let m = normalize(east * cos(heading) + north * (sin(heading) - 0.6));
            // The star it is.
            let true_radius = star_radius(look.y);
            let color = star_color(look.z);
            let hot = mix(color, vec3<f32>(1.0), clamp(p.shooting_star_heat, 0.0, 1.0));
            let t = age / duration;
            let rate = path / duration;
            let head_at = rate * min(age, duration);
            var light = vec3<f32>(0.0);
            if t < 1.0 {
                // The star, fading in and out at its path's ends, plus the
                // flare.
                let ends = smoothstep(0.0, 0.1, t) * (1.0 - smoothstep(0.9, 1.0, t));
                let head = start * cos(head_at) + m * sin(head_at);
                light += (color + hot * p.shooting_star_flare * meteor_light_curve(t))
                    * point_light(length(d - head), true_radius, pixel_angle, 2.0) * ends;
            }
            if p.shooting_star_trail > 0.0 {
                // The nearest point to `d` of the path burned so far glows
                // with the light the head had there, fading behind the head.
                let burned = clamp(atan2(dot(d, m), dot(d, start)), 0.0, head_at);
                let nearest = start * cos(burned) + m * sin(burned);
                let passed = burned / rate / duration;
                var glow = p.shooting_star_trail
                    * (1.0 + p.shooting_star_flare * meteor_light_curve(passed))
                    * exp(-METEOR_TRAIN_FADE * (head_at - burned) / train)
                    * smoothstep(0.0, 0.1, passed);
                if afterglow > 0.0 {
                    glow *= exp(-max(age - duration, 0.0) / afterglow);
                } else {
                    glow *= 1.0 - smoothstep(0.9, 1.0, t);
                }
                light += hot * glow * point_light(length(d - nearest), true_radius, pixel_angle, 1.0);
            }
            result += light * p.star_brightness * star_peak(look.y);
        }
    }
    return result;
}

// The procedural star field's grid: cells in (longitude, latitude), each
// holding at most one star, about `star_count` over the sky. xy: cells per
// unit of longitude and latitude (0..1 each), z: the chance a cell has one.
fn star_grid() -> vec3<f32> {
    let count = clamp(frame.params.star_count, 0.0, 20000.0);
    let gx = max(sqrt(max(count, 1.0) * 12.56637), 1.0);
    let gy = gx * 0.5;
    return vec3<f32>(gx, gy, clamp(count / max(gx * gy, 1.0), 0.0, 1.0));
}

// The star in cell `id` of `grid` (`night_radiance` draws the same one): xyz
// its direction, w its hash's `y` (its size and brightness, `star_radius`,
// `star_peak`); w < 0 when the cell is empty.
fn star_in_cell(id: vec2<f32>, grid: vec3<f32>) -> vec4<f32> {
    let h = hash2(id, frame.params.star_seed);
    if h.z > grid.z { return vec4<f32>(0.0, 0.0, 0.0, -1.0); }
    let at = (id + h.xy) / grid.xy;
    let lon = (at.x - 0.5) * 6.2831853;
    let lat = (at.y - 0.5) * 3.14159265;
    return vec4<f32>(cos(lat) * sin(lon), sin(lat), cos(lat) * cos(lon), h.y);
}

// Constellation lines: every pair of stars is joined, the line's light the
// product of the pair's brightnesses (raised to `constellation_selectivity`)
// times a Gaussian of their separation (`constellation_reach`, in star
// spacings, sqrt(4 pi / star_count)). A pair is drawn until its line would be
// too faint to see (CONSTELLATION_MIN_WEIGHT), so the web is complete: close
// bright pairs show, far or faint ones fade out, none is chosen at random.
// The lines are faint light (`constellation_brightness`) drawn as hairlines
// (`hairline`), shaded from one star's colour to the other's, stopping short
// of both stars.
//
// Each pixel gathers the stars within the longest visible line of it (every
// line through it has both ends there) and tests each pair. The window is
// bounded (CONSTELLATION_MAX_CELLS cells across, at most
// CONSTELLATION_MAX_STARS stars), so the cost per sky pixel is too; a reach
// past it is clipped to it.
// The lines' true half-width, radians.
const CONSTELLATION_LINE_HALF_WIDTH: f32 = 0.00004;
const CONSTELLATION_MIN_WEIGHT: f32 = 0.004;
const CONSTELLATION_MAX_CELLS: i32 = 5;
const CONSTELLATION_MAX_STARS: u32 = 28u;

fn constellation_radiance(d: vec3<f32>, pixel_angle: f32) -> vec3<f32> {
    let p = frame.params;
    let grid = star_grid();
    let lon = atan2(d.x, d.z) / 6.2831853 + 0.5;
    let lat = asin(clamp(d.y, -1.0, 1.0)) / 3.14159265 + 0.5;
    let base = floor(vec2<f32>(lon, lat) * grid.xy);
    let selectivity = max(p.constellation_selectivity, 0.0);
    // The Gaussian's width, and the longest line still visible.
    let sigma = max(p.constellation_reach, 0.05) * sqrt(12.56637 / max(p.star_count, 1.0));
    let longest = sigma * sqrt(-2.0 * log(CONSTELLATION_MIN_WEIGHT));
    // That length in cells: a cell is pi / gy high and narrows in longitude
    // toward the poles.
    let cell_angle = 3.14159265 / grid.y;
    let rows = min(i32(ceil(longest / cell_angle)), CONSTELLATION_MAX_CELLS);
    let squeeze = max(cos((lat - 0.5) * 3.14159265), 0.25);
    let cols = min(i32(ceil(longest / (cell_angle * squeeze))), CONSTELLATION_MAX_CELLS * 2);
    // A star too faint to make a visible line even with the brightest
    // partner is left out.
    let faintest = pow(CONSTELLATION_MIN_WEIGHT, 1.0 / max(selectivity, 1e-3));
    var stars: array<vec4<f32>, CONSTELLATION_MAX_STARS>;
    var hues: array<f32, CONSTELLATION_MAX_STARS>;
    var count = 0u;
    for (var y = -rows; y <= rows; y++) {
        for (var x = -cols; x <= cols; x++) {
            if count >= CONSTELLATION_MAX_STARS { break; }
            let id = base + vec2<f32>(f32(x), f32(y));
            let star = star_in_cell(id, grid);
            if star.w < faintest || length(star.xyz - d) > longest { continue; }
            stars[count] = star;
            hues[count] = hash2(id, p.star_seed).z;
            count++;
        }
    }
    let gap = pixel_angle * 3.5;
    var light = vec3<f32>(0.0);
    for (var i = 0u; i < count; i++) {
        let a = stars[i];
        for (var j = i + 1u; j < count; j++) {
            let b = stars[j];
            let separation = length(a.xyz - b.xyz);
            let weight = pow(a.w * b.w, selectivity)
                * exp(-0.5 * (separation / sigma) * (separation / sigma));
            if weight < CONSTELLATION_MIN_WEIGHT { continue; }
            // Only pixels between the two stars: nearer the midpoint than
            // half the line (cheap), then on the arc's side of each end.
            if length(d - (a.xyz + b.xyz) * 0.5) > separation * 0.5 + pixel_angle { continue; }
            let normal = cross(a.xyz, b.xyz);
            if length(normal) < 1e-7 { continue; }
            let n = normalize(normal);
            if dot(cross(a.xyz, d), n) < 0.0 || dot(cross(d, b.xyz), n) < 0.0 { continue; }
            let across = hairline(abs(dot(d, n)), CONSTELLATION_LINE_HALF_WIDTH, pixel_angle);
            if across <= 0.0 { continue; }
            let to_a = length(d - a.xyz);
            let to_b = length(d - b.xyz);
            let ends = smoothstep(gap, gap * 2.0, min(to_a, to_b));
            let color = mix(star_color(hues[i]), star_color(hues[j]), to_a / max(to_a + to_b, 1e-6));
            light += color * weight * across * ends;
        }
    }
    return light * p.constellation_brightness * p.star_brightness;
}

// `pixel_angle`: the angle one pixel spans.
fn night_radiance(d: vec3<f32>, pixel_angle: f32) -> vec3<f32> {
    let p = frame.params;
    if p.night_sky_enabled == 0u || dot(d, vec3<f32>(0.0, 1.0, 0.0)) < -0.02 {
        return vec3<f32>(0.0);
    }
    let grid = star_grid();
    let lon = atan2(d.x, d.z) / (2.0 * ATMOSPHERE_PI) + 0.5;
    let lat = asin(clamp(d.y, -1.0, 1.0)) / ATMOSPHERE_PI + 0.5;
    let cell = grid.xy;
    let q = vec2<f32>(lon, lat) * cell;
    let base = floor(q);
    let cell_uv = fract(q);
    var result = vec3<f32>(0.0);
    let occupancy = grid.z;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let id = base + vec2<f32>(f32(x), f32(y));
            let h = hash2(id, p.star_seed);
            if h.z > occupancy { continue; }
            let jitter = h.xy;
            let delta = (vec2<f32>(f32(x), f32(y)) + jitter - cell_uv) / cell;
            let angular = length(vec2<f32>(delta.x * 6.2831853 * cos((lat - 0.5) * 3.14159265), delta.y * 3.14159265));
            let light = point_light(angular, star_radius(h.y), pixel_angle, 2.0);
            if light <= 0.0 { continue; }
            let twinkle = select(1.0, 0.58 + 0.42 * sin(frame.cloud_meso_offset.w * p.star_twinkle_speed * (1.5 + h.x * 3.0) + h.z * 6.2831853), h.x < p.star_twinkle_fraction);
            result += star_color(h.z) * light * twinkle * p.star_brightness * star_peak(h.y);
        }
    }
    if p.constellation_lines_enabled != 0u {
        result += constellation_radiance(d, pixel_angle);
    }
    if p.shooting_stars_enabled != 0u && p.shooting_star_rate > 0.0 {
        result += meteor_radiance(d, pixel_angle);
    }
    return result;
}

// The panorama's orientation in the world: yaw about Y, then pitch about X,
// then roll about Z (`sky_texture::rotation` is the CPU twin).
fn sky_texture_rotation() -> mat3x3<f32> {
    let p = frame.params;
    let cy = cos(p.sky_texture_yaw);
    let sy = sin(p.sky_texture_yaw);
    let cx = cos(p.sky_texture_pitch);
    let sx = sin(p.sky_texture_pitch);
    let cz = cos(p.sky_texture_roll);
    let sz = sin(p.sky_texture_roll);
    let ry = mat3x3<f32>(vec3<f32>(cy, 0.0, -sy), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(sy, 0.0, cy));
    let rx = mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, cx, sx), vec3<f32>(0.0, -sx, cx));
    let rz = mat3x3<f32>(vec3<f32>(cz, sz, 0.0), vec3<f32>(-sz, cz, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    return ry * rx * rz;
}

// The sky texture layer along `d` (before the air in front of it): the
// panorama's linear radiance times intensity and opacity. `pixel_angle` is
// the angle one pixel spans, for the mip level.
fn sky_texture_radiance(d: vec3<f32>, pixel_angle: f32) -> vec3<f32> {
    let p = frame.params;
    if p.sky_texture_source == 0u || p.sky_texture_opacity <= 0.0 { return vec3<f32>(0.0); }
    let t = transpose(sky_texture_rotation()) * d;
    let uv = vec2<f32>(
        atan2(t.x, -t.z) / (2.0 * ATMOSPHERE_PI) + 0.5,
        0.5 - asin(clamp(t.y, -1.0, 1.0)) / ATMOSPHERE_PI);
    let width = f32(textureDimensions(sky_texture).x);
    let lod = log2(max(pixel_angle * width / (2.0 * ATMOSPHERE_PI), 1.0));
    let color = textureSampleLevel(sky_texture, sky_sampler, uv, lod).rgb;
    return color * max(p.sky_texture_intensity, 0.0) * clamp(p.sky_texture_opacity, 0.0, 1.0);
}

fn emitter_disk_radiance(d: vec3<f32>) -> vec3<f32> {
    var radiance = vec3<f32>(0.0);
    for (var i = 0u; i < frame.directional_light_count.x; i++) {
        let base = i * 3u;
        let emitter = frame.directional_lights[base + 2u];
        let emitter_type = u32(emitter.z);
        if emitter.w < 0.5
            || (emitter_type != SOLID_EMITTER_DISK && emitter_type != TEXTURED_EMITTER_DISK) {
            continue;
        }
        let toward_light = frame.directional_lights[base].xyz;
        let radius = max(emitter.x, 0.0001);
        if dot(d, toward_light) < cos(radius) { continue; }
        let right = normalize(cross(toward_light, vec3<f32>(0.0, 1.0, 0.0)) + vec3<f32>(0.001, 0.0, 0.0));
        let up = normalize(cross(right, toward_light));
        let x = dot(d, right) / sin(radius);
        let y = dot(d, up) / sin(radius);
        let r2 = x * x + y * y;
        if r2 > 1.0 { continue; }
        let z = sqrt(max(1.0 - r2, 0.0));
        let normal = normalize(right * x + up * y + toward_light * z);
        let uv = vec2<f32>(atan2(x, z) / (2.0 * ATMOSPHERE_PI) + 0.5,
            0.5 - asin(clamp(y, -1.0, 1.0)) / ATMOSPHERE_PI);
        let light_energy = frame.directional_lights[base + 1u].rgb;
        let brightness = emitter.y;
        if emitter_type == TEXTURED_EMITTER_DISK {
            let albedo = textureSampleLevel(moon_albedo, lut_sampler, uv, 0.0).rgb;
            // Every other directional source illuminates the visible disk;
            // the emitter light itself controls moonlight on the scene.
            var disk_light = vec3<f32>(0.0);
            for (var j = 0u; j < frame.directional_light_count.x; j++) {
                if i == j { continue; }
                let solar_base = j * 3u;
                let solar_dir = frame.directional_lights[solar_base].xyz;
                let solar_energy = frame.directional_lights[solar_base + 1u].rgb;
                disk_light += solar_energy * max(dot(-normal, solar_dir), 0.08);
            }
            radiance += albedo * disk_light * brightness;
        } else {
            // A textureless emitter remains a visible, scalable solid disk.
            radiance += light_energy * brightness;
        }
    }
    return radiance;
}

@fragment
fn fs_composite(in: VertexOut) -> @location(0) vec4<f32> {
    if frame.planet.w < 0.5 { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
    let camera = cameras[0];
    let pixel = vec2<i32>(in.position.xy);
    let depth = textureLoad(depth_texture, pixel, 0);
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let ray = atmosphere_view_ray(camera.view, camera.proj, ndc);
    let d = normalize(ray);
    let eye = frame.eye.xyz;
    let r = length(eye);
    let in_space = r > frame.params.top_radius;
    if helio_is_sky(depth) {
        var radiance: vec3<f32>;
        // The air between the eye and the sun's disk along `d`.
        var through = vec3<f32>(1.0);
        if in_space {
            let air = march_from_space(eye, d, 3.0e38);
            radiance = air.radiance;
            through = air.transmittance;
        } else {
            through = transmittance(r, dot(eye / r, d));
            let angles = sky_view_direction_angles(frame, d);
            radiance = textureSampleLevel(sky_view_lut, lut_sampler,
                sky_view_uv(frame.params, r, angles.x, angles.y), 0.0).rgb;
        }
        // Every untextured directional emitter gets the atmosphere's normal
        // angular radius; textured or enlarged emitters use their own size.
        if ray_sphere(eye, d, frame.params.bottom_radius).x < 0.0 {
            // Backdrop panorama under the air, the emitter disks and the stars.
            let dims = textureDimensions(depth_texture);
            let pixel_angle = 2.0 / (camera.proj[1][1] * f32(dims.y));
            radiance += sky_texture_radiance(d, pixel_angle) * through * sky_darkness();
            radiance += directional_disk_radiance(d, through);
            radiance += emitter_disk_radiance(d) * through;
        }
        // Procedural stars fade by the lights' brightness (they are gated by
        // `night_sky_enabled` inside), independent of the sky texture.
        let darkness = sky_darkness();
        if darkness > 0.001 {
            radiance += night_radiance(d, abs(2.0 / (camera.proj[1][1] * f32(textureDimensions(depth_texture).y)))) * darkness;
        }
        let clouds = clouds_in_front(in.uv, CLOUD_UNLIMITED);
        return vec4<f32>(radiance * (1.0 - clouds.a) + clouds.rgb, 0.0);
    }
    let distance = length(ray) * helio_view_depth(camera.proj, depth) * 0.001;
    let clouds = clouds_in_front(in.uv, distance);
    let clear = 1.0 - clouds.a;
    if in_space {
        let air = march_from_space(eye, d, distance);
        return vec4<f32>(air.radiance * clear + clouds.rgb, dot(air.transmittance, vec3<f32>(1.0 / 3.0)) * clear);
    }
    let range = frame.sun_illuminance.w;
    // Slices hold values at their centres; the first fades in from the eye.
    let slice = aerial_slice(distance, range) * AERIAL_SLICES - 0.5;
    var aerial = textureSampleLevel(aerial_lut, lut_sampler,
        vec3<f32>(in.uv, (max(slice, 0.0) + 0.5) / AERIAL_SLICES), 0.0);
    if slice < 0.0 {
        aerial = mix(vec4<f32>(0.0, 0.0, 0.0, 1.0), aerial, clamp(slice + 1.0, 0.0, 1.0));
    }
    return vec4<f32>(aerial.rgb * clear + clouds.rgb, aerial.a * clear);
}
