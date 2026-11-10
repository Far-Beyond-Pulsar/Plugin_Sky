//! The Deep-Space panorama installer, end to end and headless (#1657): the
//! plugin's `StarterContent` registration run by the shell's installer
//! against a local server and a temporary project. What needs a real editor
//! session (the menu, the notifications, the asset picker, frames) is not
//! covered here.
#![cfg(feature = "editor")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use engine_fs::{FileIndexQuery, ProjectFileIndex, open_project_file_index, virtual_fs};
use pulsar_reflection::{TextureRef, TextureSrc};
use sky_component::AtmosphereComponent;
use sky_plugin::sky_editor::panoramas;
use ui_level_editor::extend::{self, InstallError, InstallJob, Phase, StarterContent};

const PACK_ID: &str = "sky.deep_space_panoramas";

fn hdr(w: usize, h: usize, value: [f32; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::hdr::HdrEncoder::new(&mut out)
        .encode(&vec![image::Rgb(value); w * h], w, h)
        .unwrap();
    out
}

/// A stand-in for the upstream archive: the two layers at the paths the real
/// one has them.
fn archive() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (entry, bytes) in [
        ("hdr/deep_space_galaxy_4k.hdr", hdr(8, 4, [0.2, 0.1, 0.05])),
        ("hdr/deep_space_nebula_4k.hdr", hdr(8, 4, [0.01; 3])),
    ] {
        zip.start_file(entry, options).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn serve(body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/deep-space.zip", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let body = body.clone();
            std::thread::spawn(move || {
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    url
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The plugin's registered pack, pointed at `url` and pinned to `sha256`.
fn pack_at(url: String, sha256: &'static str) -> StarterContent {
    let registered = extend::starter_contents()
        .into_iter()
        .find(|pack| pack.id == PACK_ID)
        .expect("the sky plugin registers its panoramas as starter content");
    StarterContent {
        origin: registered.origin,
        id: registered.id,
        title: registered.title,
        description: registered.description,
        license: registered.license,
        license_url: registered.license_url,
        archive_url: leak(url),
        sha256,
        max_bytes: registered.max_bytes,
        destination: registered.destination,
        order: registered.order,
        install: registered.install,
    }
}

/// Warnings logged while it is the thread's subscriber.
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

impl tracing::Subscriber for Warnings {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() != tracing::Level::WARN {
            return;
        }
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        self.0.lock().unwrap().push(message.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn sky_with(file: TextureSrc) -> AtmosphereComponent {
    AtmosphereComponent {
        sky_texture_enabled: true,
        sky_texture_file: file,
        ..Default::default()
    }
}

fn hdrs_in(index: &ProjectFileIndex) -> Vec<String> {
    let mut paths: Vec<String> = index
        .query(&FileIndexQuery {
            extension: Some("hdr".into()),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|file| file.path)
        .collect();
    paths.sort();
    paths
}

#[test]
fn the_panorama_installer_flow_without_an_editor() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path().to_path_buf();
    engine_state::EngineContext::new().set_global();
    engine_state::set_project_path(root.display().to_string());

    // The pack is a registration with its licence, checksum and destination
    // declared; it is not a Create entry.
    let registered = pack_at(String::new(), panoramas::ARCHIVE_SHA256);
    assert_eq!(registered.license, "CC0 1.0");
    assert_eq!(registered.destination, panoramas::PROJECT_DIR);
    assert!(extend::create_entry("sky_panoramas").is_none());

    // The index is open before the install, as in a running editor.
    let index = open_project_file_index(&root)
        .unwrap()
        .expect("a local project has an index");
    assert!(hdrs_in(&index).is_empty());

    // An archive that is not the pinned one is refused and writes nothing.
    let job = InstallJob::new();
    let refused = extend::run(
        &pack_at(serve(archive()), panoramas::ARCHIVE_SHA256),
        &root,
        &job,
    );
    assert!(
        matches!(&refused, Err(InstallError::Failed(m)) if m.contains("checksum")),
        "{refused:?}"
    );
    assert!(matches!(job.phase(), Phase::Failed(_)));
    assert!(!root.join(panoramas::PROJECT_DIR).exists());
    assert!(hdrs_in(&index).is_empty());

    // An archive missing a layer passes its checksum but fails to install,
    // says which entry, and writes nothing.
    let mut partial = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    partial
        .start_file(
            "hdr/deep_space_galaxy_4k.hdr",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    let partial = partial.finish().unwrap().into_inner();
    let sha = leak(extend::sha256_hex(&partial));
    let error = extend::run(&pack_at(serve(partial), sha), &root, &InstallJob::new()).unwrap_err();
    assert!(error.to_string().contains("nebula"), "{error}");
    assert!(!root.join(panoramas::PROJECT_DIR).exists());

    // The right archive installs, through the virtual file system.
    let zip = archive();
    let sha = leak(extend::sha256_hex(&zip));
    let job = InstallJob::new();
    let files = extend::run(&pack_at(serve(zip), sha), &root, &job).unwrap();
    let dir = root.join(panoramas::PROJECT_DIR);
    let image_names = [
        panoramas::COMBINED_FILE,
        panoramas::GALAXY_FILE,
        panoramas::NEBULA_FILE,
    ];
    assert_eq!(files, image_names.map(|name| dir.join(name)));
    assert!(matches!(job.phase(), Phase::Done { .. }));
    for name in ["CREDITS.md", "LICENSE-CC0-1.0.txt"] {
        assert!(!virtual_fs::read_file(&dir.join(name)).unwrap().is_empty());
    }
    for file in &files {
        let bytes = virtual_fs::read_file(file).unwrap();
        assert!(
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Hdr).is_ok(),
            "{file:?} is a Radiance HDR"
        );
    }

    // The open file index lists the new HDRs without a restart.
    let mut expected: Vec<String> = image_names
        .iter()
        .map(|name| format!("{}/{name}", panoramas::PROJECT_DIR))
        .collect();
    expected.sort();
    assert_eq!(hdrs_in(&index), expected);

    // An asset id (what the picker stores) resolves to the file, and a
    // project-relative path from an older level still does.
    let combined = dir.join(panoramas::COMBINED_FILE);
    let id = index.ensure_typed_asset_id("texture", &combined).unwrap();
    let by_id = sky_with(TextureSrc::from_ref(TextureRef::parse(&id).unwrap()));
    assert_eq!(by_id.sky_texture_path(), Some(combined.clone()));
    assert_ne!(
        by_id.to_row().sky_texture_source,
        helio_pass_sky::sky_texture::source::NONE
    );
    let legacy = sky_with(TextureSrc::new(format!(
        "{}/{}",
        panoramas::PROJECT_DIR,
        panoramas::COMBINED_FILE
    )));
    assert_eq!(legacy.sky_texture_path(), Some(combined));

    // An id the index does not know draws the procedural sky and warns.
    let unknown = sky_with(TextureSrc::from_ref(
        TextureRef::parse("3f2c1a9e-7b4d-4e5a-9c1d-0a1b2c3d4e5f").unwrap(),
    ));
    let warnings = Warnings::default();
    let path = tracing::subscriber::with_default(warnings.clone(), || unknown.sky_texture_path());
    assert_eq!(path, None);
    assert_eq!(
        unknown.to_row().sky_texture_source,
        helio_pass_sky::sky_texture::source::NONE
    );
    let logged = warnings.0.lock().unwrap().join("\n");
    assert!(logged.contains("not indexed"), "{logged}");
}
