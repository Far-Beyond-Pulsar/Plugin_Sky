//! Runs the sky texture loader and the composite pipeline on a real device.
//! Skipped (with a message) when no GPU adapter is available.
use helio_pass_sky::sky_texture::{self, source, SkyTextureCache};
use helio_pass_sky::AtmosphereCompositePass;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(helio_core::wgpu_instance_descriptor().with_env());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .ok()?;
        adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .ok()
    })
}

#[test]
fn the_composite_pipeline_builds_on_a_device() {
    let Some((device, _queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let _pass = AtmosphereCompositePass::new(&device, wgpu::TextureFormat::Rgba16Float);
}

#[test]
fn a_project_file_becomes_resident_and_turning_it_off_frees_it() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut cache = SkyTextureCache::new(&device);
    let path = write_hdr();
    let code = sky_texture::file_source(&path);
    let start = std::time::Instant::now();
    while !cache.is_resident(code) {
        let _ = cache.view(&device, &queue, code);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(120),
            "the preset never became resident"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    eprintln!("resident after {:?}", start.elapsed());
    let _ = cache.view(&device, &queue, source::NONE);
    assert!(
        !cache.is_resident(code),
        "turning the layer off frees the texture"
    );
    let _ = std::fs::remove_file(path);
}

/// A small Radiance HDR panorama in the temp dir.
fn write_hdr() -> std::path::PathBuf {
    let pixels = vec![image::Rgb([1.0f32, 0.5, 0.25]); 16 * 8];
    let dir = std::env::temp_dir().join("pulsar_sky_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}_gpu.hdr", std::process::id()));
    let file = std::fs::File::create(&path).unwrap();
    image::codecs::hdr::HdrEncoder::new(std::io::BufWriter::new(file))
        .encode(&pixels, 16, 8)
        .unwrap();
    path
}
