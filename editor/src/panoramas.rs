//! Adding the CC0 Deep-Space panoramas to a project.
//!
//! The panoramas are project assets, not engine content: nothing is
//! embedded in a binary and no build touches the network. The Sky's texture
//! slot names a texture asset of the project (`sky_texture_file`), so the
//! images have to be in the project first. This module puts them there, at
//! editor time and only when the author asks (Create menu, "Sky Panoramas
//! (CC0 Deep-Space)"):
//!
//! 1. download the pinned upstream archive (Toolbench's Deep-Space Skybox
//!    layers, CC0 1.0) on a worker thread;
//! 2. verify its SHA-256;
//! 3. write the galaxy and nebula layers unchanged, a third file with the
//!    nebula brightened ([`NEBULA_GAIN`], the former default look) and added
//!    to the galaxy, and the credits and license text, into
//!    [`PROJECT_DIR`], through the virtual file system.
//!
//! The project's file index then lists them as texture assets; the author
//! picks one in the Sky's Texture field like any other texture, and the
//! project (and so the game) carries only the file it uses.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Pinned upstream archive of the layers (see `assets/deep-space/CREDITS.md`).
pub const ARCHIVE_URL: &str = "https://toolbenchdev.com/downloads/deep-space-skybox-layers-1.0.zip";
/// SHA-256 of [`ARCHIVE_URL`]'s archive.
pub const ARCHIVE_SHA256: &str = "873456f4ef8689d1ef8274d25c0524517b734be304cbbc28176b1778c5432acb";
/// Where the files go, relative to the project folder.
pub const PROJECT_DIR: &str = "assets/sky/deep-space";
/// The nebula layer is two orders of magnitude fainter than the galaxy (peak
/// 0.018 against 0.162); this is the gain it is mixed with in the combined file.
pub const NEBULA_GAIN: f32 = 6.0;

const GALAXY_ENTRY: &str = "hdr/deep_space_galaxy_4k.hdr";
const NEBULA_ENTRY: &str = "hdr/deep_space_nebula_4k.hdr";
/// File names written into [`PROJECT_DIR`].
pub const GALAXY_FILE: &str = "deep_space_galaxy_4k.hdr";
pub const NEBULA_FILE: &str = "deep_space_nebula_4k.hdr";
pub const COMBINED_FILE: &str = "deep_space_galaxy_nebula_4k.hdr";

const CREDITS: &str = include_str!("../assets/deep-space/CREDITS.md");
const LICENSE: &str = include_str!("../assets/deep-space/LICENSE-CC0-1.0.txt");

/// The two layers as the archive holds them (Radiance HDR bytes).
pub struct Layers {
    pub galaxy: Vec<u8>,
    pub nebula: Vec<u8>,
}

/// Downloads [`ARCHIVE_URL`]. Blocking; call off the UI thread.
pub fn download() -> Result<Vec<u8>, String> {
    let response = ureq::get(ARCHIVE_URL)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("download {ARCHIVE_URL}: {e}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(256 << 20)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("download {ARCHIVE_URL}: {e}"))?;
    Ok(bytes)
}

/// Checks `archive` against `sha256` (lowercase hex) and extracts the layers.
pub fn extract(archive: Vec<u8>, sha256: &str) -> Result<Layers, String> {
    let digest: String = Sha256::digest(&archive)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != sha256 {
        return Err(format!("archive checksum mismatch (got {digest})"));
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|e| e.to_string())?;
    let mut read = |entry: &str| -> Result<Vec<u8>, String> {
        let mut file = zip.by_name(entry).map_err(|e| format!("{entry}: {e}"))?;
        let mut data = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut data).map_err(|e| e.to_string())?;
        Ok(data)
    };
    Ok(Layers {
        galaxy: read(GALAXY_ENTRY)?,
        nebula: read(NEBULA_ENTRY)?,
    })
}

fn decode(bytes: &[u8]) -> Result<image::Rgb32FImage, String> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Hdr)
        .map(|image| image.to_rgb32f())
        .map_err(|e| e.to_string())
}

/// The galaxy with `nebula * gain` added, as Radiance HDR bytes.
pub fn combine(galaxy: &[u8], nebula: &[u8], gain: f32) -> Result<Vec<u8>, String> {
    let galaxy = decode(galaxy)?;
    let nebula = decode(nebula)?;
    if galaxy.dimensions() != nebula.dimensions() {
        return Err("the galaxy and nebula layers differ in size".into());
    }
    let (w, h) = galaxy.dimensions();
    let pixels: Vec<image::Rgb<f32>> = galaxy
        .pixels()
        .zip(nebula.pixels())
        .map(|(g, n)| image::Rgb([0, 1, 2].map(|c| g[c] + n[c] * gain)))
        .collect();
    let mut out = Vec::new();
    image::codecs::hdr::HdrEncoder::new(&mut out)
        .encode(&pixels, w as usize, h as usize)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Writes the layers, the combined file, the credits and the license into
/// `dir` through the virtual file system. Returns the three image paths.
pub fn write_project_files(dir: &Path, layers: &Layers) -> Result<Vec<PathBuf>, String> {
    let combined = combine(&layers.galaxy, &layers.nebula, NEBULA_GAIN)?;
    engine_fs::virtual_fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let files: [(&str, &[u8]); 5] = [
        (COMBINED_FILE, &combined),
        (GALAXY_FILE, &layers.galaxy),
        (NEBULA_FILE, &layers.nebula),
        ("CREDITS.md", CREDITS.as_bytes()),
        ("LICENSE-CC0-1.0.txt", LICENSE.as_bytes()),
    ];
    for (name, bytes) in files {
        engine_fs::virtual_fs::write_file(&dir.join(name), bytes)
            .map_err(|e| format!("{name}: {e}"))?;
    }
    Ok([COMBINED_FILE, GALAXY_FILE, NEBULA_FILE]
        .map(|name| dir.join(name))
        .to_vec())
}

/// Downloads, verifies and writes the panoramas into the project at
/// `project_root`. Blocking; call off the UI thread.
pub fn install(project_root: &Path) -> Result<Vec<PathBuf>, String> {
    let layers = extract(download()?, ARCHIVE_SHA256)?;
    write_project_files(&project_root.join(PROJECT_DIR), &layers)
}

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Starts [`install`] on a worker thread for the open project. `Err` is what
/// to tell the author when it cannot start; the outcome is logged.
pub fn install_in_background() -> Result<(), &'static str> {
    let Some(root) = engine_state::get_project_path().map(PathBuf::from) else {
        return Err("Open a project first: the panoramas are added to its assets");
    };
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("The Deep-Space panoramas are already being added");
    }
    std::thread::Builder::new()
        .name("sky-panoramas".into())
        .spawn(move || {
            match install(&root) {
                Ok(files) => tracing::info!(
                    "added the Deep-Space sky panoramas to the project: {}",
                    files
                        .iter()
                        .map(|f| f.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Err(error) => tracing::warn!("could not add the Deep-Space sky panoramas: {error}"),
            }
            RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        })
        .map(|_| ())
        .map_err(|_| {
            RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
            "Could not start the download"
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn hdr(w: usize, h: usize, value: [f32; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        image::codecs::hdr::HdrEncoder::new(&mut out)
            .encode(&vec![image::Rgb(value); w * h], w, h)
            .unwrap();
        out
    }

    fn archive(galaxy: &[u8], nebula: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (entry, bytes) in [(GALAXY_ENTRY, galaxy), (NEBULA_ENTRY, nebula)] {
            zip.start_file(entry, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn an_archive_with_the_wrong_checksum_is_refused() {
        let zip = archive(&hdr(4, 2, [1.0; 3]), &hdr(4, 2, [1.0; 3]));
        let error = extract(zip, ARCHIVE_SHA256).err().unwrap();
        assert!(error.contains("checksum mismatch"), "{error}");
    }

    #[test]
    fn the_layers_are_extracted_unchanged() {
        let (galaxy, nebula) = (hdr(4, 2, [1.0, 0.5, 0.25]), hdr(4, 2, [0.1; 3]));
        let zip = archive(&galaxy, &nebula);
        let digest = sha(&zip);
        let layers = extract(zip, &digest).unwrap();
        assert_eq!((layers.galaxy, layers.nebula), (galaxy, nebula));
    }

    #[test]
    fn the_combined_file_adds_the_gained_nebula_to_the_galaxy() {
        let combined = combine(&hdr(4, 2, [1.0, 0.5, 0.25]), &hdr(4, 2, [0.1; 3]), 6.0).unwrap();
        let image = decode(&combined).unwrap();
        let pixel = image.get_pixel(0, 0);
        for (got, want) in pixel.0.iter().zip([1.6, 1.1, 0.85]) {
            assert!((got - want).abs() < 0.05, "{pixel:?}");
        }
        assert!(combine(&hdr(4, 2, [1.0; 3]), &hdr(8, 4, [1.0; 3]), 6.0).is_err());
    }
}
