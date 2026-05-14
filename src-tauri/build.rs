use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    if cfg!(target_os = "macos") {
        compile_swift_static_lib();
    }
    tauri_build::build()
}

fn compile_swift_static_lib() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let native_dir = manifest_dir.join("native");
    let swift_sources: Vec<PathBuf> = ["SCKAudioCapture.swift", "AACEncoder.swift"]
        .iter()
        .map(|n| native_dir.join(n))
        .filter(|p| p.exists())
        .collect();
    if swift_sources.is_empty() {
        return;
    }
    for src in &swift_sources {
        println!("cargo:rerun-if-changed={}", src.display());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let lib_dir = out_dir.join("swift_lib");
    std::fs::create_dir_all(&lib_dir).expect("create swift_lib dir");

    let target_triple = env::var("TARGET").unwrap_or_else(|_| "arm64-apple-darwin".into());
    let swift_target = swift_target_for(&target_triple);

    let lib_name = "betteraudio_native";
    let lib_path = lib_dir.join(format!("lib{lib_name}.a"));

    // Use `swiftc` directly (NOT `swift build`) — SwiftPM's ManifestAPI on the
    // host's CLT has a Swift-version ABI mismatch that prevents `Package.swift`
    // manifest compilation. Single-file static-lib invocation works fine.
    let mut cmd = Command::new("swiftc");
    cmd.args([
        "-emit-library",
        "-static",
        "-parse-as-library",
        "-O",
        "-whole-module-optimization",
        "-target",
        &swift_target,
        "-module-name",
        "BetterAudioNative",
        "-o",
        lib_path.to_str().unwrap(),
    ]);
    for src in &swift_sources {
        cmd.arg(src);
    }
    let status = cmd.status().expect("invoke swiftc");
    if !status.success() {
        panic!("swiftc failed");
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static={lib_name}");

    // Note: `CoreAudioTypes` is a header-only module on modern macOS — its
    // symbols live in `CoreAudio`, so we don't link it as a framework.
    for framework in [
        "ScreenCaptureKit",
        "CoreMedia",
        "CoreFoundation",
        "Foundation",
        "AVFoundation",
        "AudioToolbox",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }

    // Swift runtime ships with macOS; link dynamically from the system path.
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn swift_target_for(cargo_triple: &str) -> String {
    if cargo_triple.starts_with("aarch64-apple-darwin") {
        "arm64-apple-macosx13.0".into()
    } else if cargo_triple.starts_with("x86_64-apple-darwin") {
        "x86_64-apple-macosx13.0".into()
    } else {
        cargo_triple.replace("-apple-darwin", "-apple-macosx13.0")
    }
}
