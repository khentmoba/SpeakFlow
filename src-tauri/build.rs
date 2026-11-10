use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

mod build_support;

use build_support::{select_engine_archive, ENGINE_ARCHIVE_CANDIDATES};

fn emit_rerun_hints() {
    for path in ENGINE_ARCHIVE_CANDIDATES {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
}

fn compute_file_fingerprint(path: &Path) -> String {
    let file = fs::File::open(path)
        .unwrap_or_else(|err| panic!("Failed to read engine archive {}: {}", path.display(), err));
    let mut reader = BufReader::with_capacity(8 * 1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 8 * 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer).unwrap_or_else(|err| {
            panic!(
                "Failed to compute engine archive digest {}: {}",
                path.display(),
                err
            )
        });
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn main() {
    emit_rerun_hints();

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set by Cargo"),
    );
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("PROFILE").as_deref() == Ok("debug")
    {
        // Library test harnesses need the same Common Controls v6 manifest.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
        // The application binaries already receive manifests from tauri_build.
        println!("cargo:rustc-link-arg-bin=speak-flow=/MANIFEST:NO");
        println!("cargo:rustc-link-arg-bin=speakflow=/MANIFEST:NO");
    }
    let allow_placeholder = env::var("PROFILE").map_or(true, |profile| profile != "release");
    let engine_archive = select_engine_archive(&manifest_dir, allow_placeholder)
        .unwrap_or_else(|message| panic!("{message}"));
    println!(
        "cargo:rustc-env=SPEAK_FLOW_ENGINE_ARCHIVE_FINGERPRINT={}",
        compute_file_fingerprint(&engine_archive)
    );

    let attributes = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("windows-app-manifest.xml")),
    );

    tauri_build::try_build(attributes).expect("failed to run tauri build")
}
