//! The sky a ray takes where it meets nothing against the sky the sky pass
//! draws, over the whole sphere of directions: the atmosphere's pass runs in
//! a render graph on a real device, and a probe pass reads, for directions
//! of a Fibonacci sphere seen from the eye, the sky-view LUT as the
//! composite samples it and the frame's sky radiance table as a ray's miss
//! takes it (`sky_radiance_eval`). Skipped without a GPU.

use std::sync::Arc;

use bytemuck::Zeroable;
use helio_core::{
    BufferHandle, BufferKey, GpuCameraUniforms, PassContext, RenderGraph, RenderPass,
    Result as HelioResult, SceneBufferProjection, SceneInput,
};
use helio_frame_api::lighting::sky::{
    SKY_FRAME, SKY_RADIANCE, SKY_RADIANCE_SNIPPET, SKY_SNIPPET, SKY_VIEW,
};
use helio_light_api::{DirectionalLight, DirectionalLights, DIRECTIONAL_LIGHTS};
use helio_pass_sky::{AtmospherePass, AtmosphereRow};

/// Directions of the sphere each pose is judged at.
const DIRECTIONS: u32 = 16384;

/// The contract, over the sphere at each pose: the miss's sky integrates
/// to the drawn sky's within 2% (luminance, every direction its solid
/// angle), its mean absolute error is within 2% of the sky's mean, and 99%
/// of directions are within 10% (of the drawn sky, or of a tenth of the
/// sky's mean where it is darker). The last 1% are the aureole of a sun on
/// the horizon, the limb seen from high up and the zenith there (radiance
/// near zero).
const RATIO: f64 = 0.02;
const MEAN: f64 = 0.02;
const P99: f64 = 0.10;

struct Gpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

fn gpu() -> Option<&'static Gpu> {
    static GPU: std::sync::OnceLock<Option<Gpu>> = std::sync::OnceLock::new();
    GPU.get_or_init(|| {
        let instance = wgpu::Instance::new(helio_core::wgpu_instance_descriptor());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok()?;
        Some(Gpu {
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    })
    .as_ref()
}

const PROBE: &str = r#"//!use frame_sky
//!use sky_radiance
@group(0) @binding(0) var<storage, read> sky_frame: SkyFrame;
@group(0) @binding(1) var sky_view_lut: texture_2d<f32>;
@group(0) @binding(2) var lut_sampler: sampler;
@group(0) @binding(3) var sky_radiance: texture_3d<f32>;
@group(0) @binding(4) var sky_radiance_sampler: sampler;
@group(0) @binding(5) var<storage, read_write> out: array<vec4<f32>>;

@compute @workgroup_size(64)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = arrayLength(&out) / 2u;
    if id.x >= n { return; }
    let z = 1.0 - (f32(id.x) + 0.5) * 2.0 / f32(n);
    let a = f32(id.x) * 2.39996323;
    let q = sqrt(max(1.0 - z * z, 0.0));
    let d = vec3<f32>(q * cos(a), z, q * sin(a));
    // The sky pass's sky (`fs_composite`, inside the atmosphere).
    let angles = sky_view_direction_angles(sky_frame, d);
    let drawn = textureSampleLevel(sky_view_lut, lut_sampler,
        sky_view_uv(sky_frame.params, length(sky_frame.eye.xyz), angles.x, angles.y), 0.0).rgb;
    out[id.x * 2u] = vec4<f32>(drawn, 0.0);
    out[id.x * 2u + 1u] = vec4<f32>(sky_radiance_eval(vec3<f32>(0.0), d), 0.0);
}
"#;

/// Reads the sky the pass draws and the sky a miss takes, for every
/// direction, once the atmosphere has published both.
struct Probe {
    pipeline: wgpu::ComputePipeline,
    sampler: wgpu::Sampler,
    table_sampler: wgpu::Sampler,
    out: wgpu::Buffer,
    ran: Arc<std::sync::atomic::AtomicBool>,
}

impl RenderPass for Probe {
    fn name(&self) -> &'static str {
        "SkyProbe"
    }
    fn reads(&self) -> &'static [&'static str] {
        &[SKY_FRAME, SKY_VIEW, SKY_RADIANCE]
    }
    fn execute(&mut self, ctx: &mut PassContext) -> HelioResult<()> {
        let frame = ctx
            .registry
            .get(helio_core::ResourceKey::<&wgpu::Buffer>::new(SKY_FRAME));
        let view = ctx
            .registry
            .get(helio_core::ResourceKey::<&wgpu::TextureView>::new(SKY_VIEW));
        let table = ctx
            .registry
            .get(helio_core::ResourceKey::<&wgpu::TextureView>::new(
                SKY_RADIANCE,
            ));
        let (Some(frame), Some(view), Some(table)) = (frame, view, table) else {
            return Ok(());
        };
        let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SkyProbe"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(table),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.table_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.out.as_entire_binding(),
                },
            ],
        });
        let mut cmds = ctx.graphics_cmds();
        let mut pass = cmds.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("SkyProbe"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, Some(&group), &[]);
        pass.dispatch_workgroups(DIRECTIONS.div_ceil(64), 1, 1);
        self.ran.store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
}

struct Input {
    gpu: &'static Gpu,
    camera: wgpu::Buffer,
    camera_data: GpuCameraUniforms,
    buffers: SceneBufferProjection,
    frame: u64,
}

impl SceneInput for Input {
    fn world_origin(&self) -> glam::DVec3 {
        glam::DVec3::ZERO
    }
    fn device(&self) -> &Arc<wgpu::Device> {
        &self.gpu.device
    }
    fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.gpu.queue
    }
    fn frame_count(&self) -> u64 {
        self.frame
    }
    fn camera(&self) -> &wgpu::Buffer {
        &self.camera
    }
    fn camera_data(&self) -> &GpuCameraUniforms {
        &self.camera_data
    }
    fn camera_generation(&self) -> u64 {
        0
    }
    fn scene_buffers(&self) -> &SceneBufferProjection {
        &self.buffers
    }
}

fn read(gpu: &Gpu, buffer: &wgpu::Buffer) -> Vec<[f32; 4]> {
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: buffer.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
    gpu.queue.submit([encoder.finish()]);
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    bytemuck::cast_slice(&staging.slice(..).get_mapped_range().unwrap()).to_vec()
}

fn storage(gpu: &Gpu, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    gpu.device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::UNIFORM
                | wgpu::BufferUsages::COPY_SRC,
        })
}

/// A sun `elevation` degrees above the horizon (azimuth `azimuth` degrees
/// from +x towards +z), of radiance `energy`.
fn sun(elevation: f32, azimuth: f32, energy: [f32; 3]) -> DirectionalLight {
    let (e, a) = (elevation.to_radians(), azimuth.to_radians());
    DirectionalLight {
        toward: [e.cos() * a.cos(), e.sin(), e.cos() * a.sin(), 1.0],
        radiance: [energy[0], energy[1], energy[2], 0.0],
        disk: [0.0; 4],
    }
}

/// For every direction: the sky the pass draws, the sky a miss takes; and
/// the wall time (ms) a frame takes more when the rows change (the tables
/// that depend on the medium alone rebuilt) than when they do not.
fn skies(
    row: AtmosphereRow,
    eye_m: f32,
    suns: &[DirectionalLight],
) -> (Vec<([f32; 3], [f32; 3])>, f64) {
    let gpu = gpu().expect("checked by the caller");
    let mut lights = DirectionalLights::zeroed();
    lights.lights[..suns.len()].copy_from_slice(suns);
    lights.count = [suns.len() as u32, 0, 0, 0];
    let atmospheres = storage(gpu, "atmospheres", bytemuck::bytes_of(&row));
    let directional = storage(gpu, "directional", bytemuck::bytes_of(&lights));
    let mut camera = GpuCameraUniforms::zeroed();
    let identity = glam::Mat4::IDENTITY.to_cols_array();
    camera.view = identity;
    camera.proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(1.0, 1.0, 0.1)
        .to_cols_array();
    camera.view_proj = camera.proj;
    camera.inv_view_proj = glam::Mat4::from_cols_array(&camera.proj)
        .inverse()
        .to_cols_array();
    camera.position_near = [0.0, eye_m, 0.0, 0.1];
    let cameras = storage(gpu, "cameras", bytemuck::cast_slice(&[camera, camera]));
    let buffers = |generation| {
        let handle = |buffer: &wgpu::Buffer, row_bytes| BufferHandle {
            buffer: buffer.clone(),
            epoch: 0,
            row_bytes,
            content_generation: generation,
        };
        SceneBufferProjection::from_handles([
            (
                BufferKey::of("atmospheres"),
                handle(&atmospheres, std::mem::size_of::<AtmosphereRow>() as u64),
            ),
            (
                BufferKey::of(DIRECTIONAL_LIGHTS),
                handle(
                    &directional,
                    std::mem::size_of::<DirectionalLights>() as u64,
                ),
            ),
        ])
    };
    let module = helio_core::shader::module_with(
        &gpu.device,
        "SkyProbe",
        PROBE,
        &[SKY_SNIPPET, SKY_RADIANCE_SNIPPET],
    );
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("SkyProbe"),
            layout: None,
            module: &module,
            entry_point: Some("probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let linear = |label| {
        gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(label),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        })
    };
    let out = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("SkyProbe out"),
        size: u64::from(DIRECTIONS) * 2 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut graph = RenderGraph::new(&gpu.device, &gpu.queue);
    graph.add_pass(Box::new(AtmospherePass::new(&gpu.device)));
    graph.add_pass(Box::new(Probe {
        pipeline,
        sampler: linear("SkyProbe view"),
        table_sampler: helio_frame_api::lighting::sky::sky_radiance_sampler(&gpu.device),
        out: out.clone(),
        ran: ran.clone(),
    }));
    graph.lock(16, 16);
    let mut input = Input {
        gpu,
        camera: cameras,
        camera_data: camera,
        buffers: buffers(1),
        frame: 0,
    };
    let texture = |format| {
        gpu.device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    let (target, depth) = (
        texture(wgpu::TextureFormat::Rgba8Unorm),
        texture(wgpu::TextureFormat::Depth32Float),
    );
    let mut frame = |input: &mut Input| {
        let mut resources = helio_core::ResourceRegistry::empty();
        graph
            .execute_with_registry(input, &target, &depth, &mut resources)
            .expect("the frame renders");
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        input.frame += 1;
    };
    // The pass's readiness: the probe ran on a frame the atmosphere drew.
    let start = std::time::Instant::now();
    while !ran.load(std::sync::atomic::Ordering::Acquire) {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(60),
            "the atmosphere never published its sky"
        );
        frame(&mut input);
    }
    let skies = read(gpu, &out)
        .chunks(2)
        .map(|p| ([p[0][0], p[0][1], p[0][2]], [p[1][0], p[1][1], p[1][2]]))
        .collect();
    // Frames with the rows unchanged, then frames with them rewritten.
    let mut timed = |input: &mut Input, changing: bool| {
        let start = std::time::Instant::now();
        for _ in 0..16 {
            if changing {
                input.buffers = buffers(input.frame + 2);
            }
            frame(input);
        }
        start.elapsed().as_secs_f64() * 1e3 / 16.0
    };
    let still = timed(&mut input, false);
    let changing = timed(&mut input, true);
    (skies, changing - still)
}

fn luminance(c: [f32; 3]) -> f64 {
    0.2126 * f64::from(c[0]) + 0.7152 * f64::from(c[1]) + 0.0722 * f64::from(c[2])
}

/// How far the miss's sky is from the drawn sky over the sphere: the ratio
/// of their means, the mean absolute error over the mean (each direction
/// weighs its solid angle: the Fibonacci sphere's are equal), and the 99th
/// percentile and worst of each direction's relative error.
struct Error {
    ratio: f64,
    mean: f64,
    p99: f64,
    worst: f64,
}

fn error(skies: &[([f32; 3], [f32; 3])]) -> Error {
    let drawn: f64 = skies.iter().map(|(d, _)| luminance(*d)).sum();
    let taken: f64 = skies.iter().map(|(_, t)| luminance(*t)).sum();
    let absolute: f64 = skies
        .iter()
        .map(|(d, t)| (0..3).map(|c| f64::from((d[c] - t[c]).abs())).sum::<f64>() / 3.0)
        .sum();
    let mean_rgb: f64 = skies
        .iter()
        .map(|(d, _)| (f64::from(d[0]) + f64::from(d[1]) + f64::from(d[2])) / 3.0)
        .sum();
    // A direction's relative error, against the sky's mean where the sky is
    // darker than a tenth of it (the night side, the ground in shadow).
    let floor = drawn / skies.len() as f64 * 0.1;
    let mut relative: Vec<f64> = skies
        .iter()
        .map(|(d, t)| (luminance(*t) - luminance(*d)).abs() / luminance(*d).max(floor))
        .collect();
    relative.sort_by(f64::total_cmp);
    Error {
        ratio: taken / drawn,
        mean: absolute / mean_rgb.max(1e-30),
        p99: relative[relative.len() * 99 / 100],
        worst: *relative.last().unwrap(),
    }
}

/// Direction `i` of the probe's Fibonacci sphere.
fn direction(i: usize) -> [f64; 3] {
    let n = f64::from(DIRECTIONS);
    let z = 1.0 - (i as f64 + 0.5) * 2.0 / n;
    let a = i as f64 * 2.39996323;
    let q = (1.0 - z * z).max(0.0).sqrt();
    [q * a.cos(), z, q * a.sin()]
}

/// What the representation the radiance table replaced could give at best:
/// the drawn sky projected on L2 harmonics (nine, any frame) and turned back
/// into radiance. The ratio of its mean to the drawn sky's over the band a
/// level camera sees (0 to 20 degrees above the horizon), and its mean
/// absolute error over the sphere's mean.
fn harmonics(skies: &[([f32; 3], [f32; 3])]) -> (f64, f64) {
    let basis = |d: [f64; 3]| {
        let [x, y, z] = d;
        [
            0.282095,
            0.488603 * y,
            0.488603 * z,
            0.488603 * x,
            1.092548 * x * y,
            1.092548 * y * z,
            0.315392 * (3.0 * z * z - 1.0),
            1.092548 * x * z,
            0.546274 * (x * x - y * y),
        ]
    };
    let mut c = [0.0f64; 9];
    for (i, (drawn, _)) in skies.iter().enumerate() {
        for (k, b) in basis(direction(i)).iter().enumerate() {
            c[k] += luminance(*drawn) * b * 4.0 * std::f64::consts::PI / skies.len() as f64;
        }
    }
    let (mut band, mut band_drawn, mut error, mut total) = (0.0, 0.0, 0.0, 0.0);
    for (i, (drawn, _)) in skies.iter().enumerate() {
        let d = direction(i);
        let l: f64 = basis(d).iter().zip(&c).map(|(b, c)| b * c).sum();
        let truth = luminance(*drawn);
        if d[1] > 0.0 && d[1] < 20f64.to_radians().sin() {
            band += l;
            band_drawn += truth;
        }
        error += (l - truth).abs();
        total += truth;
    }
    (band / band_drawn, error / total)
}

/// Over the sphere, at eyes from the ground to near the top of the air and
/// suns from high to below the horizon, one and two of them, on Earth and
/// Mars: the sky a miss takes is the sky the pass draws.
#[test]
fn a_miss_takes_the_sky_the_pass_draws() {
    if gpu().is_none() {
        eprintln!("no GPU adapter; skipping");
        return;
    }
    let white = [1.0, 1.0, 1.0];
    let cases: Vec<(&str, AtmosphereRow, f32, Vec<DirectionalLight>)> = vec![
        (
            "earth, ground, sun 60",
            AtmosphereRow::earth(),
            2.0,
            vec![sun(60.0, 0.0, white)],
        ),
        (
            "earth, ground, sun 25",
            AtmosphereRow::earth(),
            2.0,
            vec![sun(25.0, 30.0, white)],
        ),
        (
            "earth, ground, sun 31.5",
            AtmosphereRow::earth(),
            2.0,
            vec![sun(31.5, 0.0, white)],
        ),
        (
            "earth, ground, sun 8",
            AtmosphereRow::earth(),
            2.0,
            vec![sun(8.0, 0.0, white)],
        ),
        (
            "earth, ground, sun 2",
            AtmosphereRow::earth(),
            2.0,
            vec![sun(2.0, 0.0, white)],
        ),
        (
            "earth, ground, sun -3",
            AtmosphereRow::earth(),
            2.0,
            vec![sun(-3.0, 0.0, white)],
        ),
        (
            "earth, 300 m, sun 40",
            AtmosphereRow::earth(),
            300.0,
            vec![sun(40.0, 0.0, white)],
        ),
        (
            "earth, 3 km, sun 40",
            AtmosphereRow::earth(),
            3000.0,
            vec![sun(40.0, 0.0, white)],
        ),
        (
            "earth, 20 km, sun 15",
            AtmosphereRow::earth(),
            20_000.0,
            vec![sun(15.0, 0.0, white)],
        ),
        (
            "earth, 80 km, sun 30",
            AtmosphereRow::earth(),
            80_000.0,
            vec![sun(30.0, 0.0, white)],
        ),
        (
            "earth, ground, two suns",
            AtmosphereRow::earth(),
            2.0,
            vec![sun(50.0, 0.0, white), sun(10.0, 140.0, [0.3, 0.2, 0.1])],
        ),
        (
            "mars, ground, sun 35",
            AtmosphereRow::mars(),
            2.0,
            vec![sun(35.0, 0.0, white)],
        ),
    ];
    let mut failures = Vec::new();
    for (name, row, eye, suns) in cases {
        let (skies, rebuild_ms) = skies(row, eye, &suns);
        let e = error(&skies);
        let (band, l2) = harmonics(&skies);
        eprintln!(
            "{name}: ratio {:.4}, mean |err| {:.4}, p99 {:.4}, worst {:.4}; L2 harmonics \
             of the drawn sky: {band:.3} of it 0-20 degrees up, mean |err| {l2:.3}; \
             tables rebuilt in {rebuild_ms:.2} ms",
            e.ratio, e.mean, e.p99, e.worst
        );
        if (e.ratio - 1.0).abs() > RATIO || e.mean > MEAN || e.p99 > P99 {
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "the miss's sky is not the drawn sky: {failures:?}"
    );
}
