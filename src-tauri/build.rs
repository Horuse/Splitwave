use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Gate on the build TARGET, not the host: `cfg!(target_os = ...)` in a build
    // script reflects the host, which breaks cross-compilation.
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        compile_swift_static_lib();
        build_virtual_driver();
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

    let lib_name = "splitwave_native";
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
        "SplitwaveNative",
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

    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn build_virtual_driver() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let driver_dir = manifest_dir.join("native/virtual_driver");
    if !driver_dir.exists() {
        return;
    }

    println!("cargo:rerun-if-changed={}", driver_dir.join("SplitAudioDriver.cpp").display());
    println!("cargo:rerun-if-changed={}", driver_dir.join("Info.plist").display());

    const LIBASPL_COMMIT: &str = "633e0f70203edd87d320fc5a3cae901e1363aac5";

    let libaspl_dir = driver_dir.join("libASPL");
    if !libaspl_dir.exists() {
        let status = Command::new("git")
            .args([
                "clone",
                "https://github.com/gavv/libASPL.git",
                libaspl_dir.to_str().unwrap(),
            ])
            .status()
            .expect("git clone libASPL");
        if !status.success() {
            panic!("failed to clone libASPL — check network and try again");
        }
    }

    // Only checkout when not already pinned -- a checkout every build touches
    // .git/index.lock, which the Tauri dev watcher sees as a change and loops.
    let head = Command::new("git")
        .args(["-C", libaspl_dir.to_str().unwrap(), "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if head.as_deref() != Some(LIBASPL_COMMIT) {
        let status = Command::new("git")
            .args(["-C", libaspl_dir.to_str().unwrap(), "checkout", LIBASPL_COMMIT])
            .status()
            .expect("git checkout libASPL pin");
        if !status.success() {
            panic!(
                "failed to pin libASPL to {LIBASPL_COMMIT} — \
                 delete native/virtual_driver/libASPL and rebuild"
            );
        }
    }

    // Collect libASPL sources (all .cpp — .g.cpp files contain vtable implementations)
    let libaspl_src = libaspl_dir.join("src");
    let mut sources: Vec<PathBuf> = std::fs::read_dir(&libaspl_src)
        .expect("read libASPL/src")
        .filter_map(|e| {
            let p = e.ok()?.path();
            if p.extension().map_or(false, |e| e == "cpp") {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    sources.push(driver_dir.join("SplitAudioDriver.cpp"));

    let bundle_root = manifest_dir.join("resources/Splitwave.driver");
    let bundle_macos = bundle_root.join("Contents/MacOS");
    std::fs::create_dir_all(&bundle_macos).expect("create bundle MacOS dir");

    // Only copy Info.plist if content changed (avoids spurious mtime updates).
    let plist_src = driver_dir.join("Info.plist");
    let plist_dst = bundle_root.join("Contents/Info.plist");
    let plist_bytes = std::fs::read(&plist_src).expect("read Info.plist");
    if std::fs::read(&plist_dst).ok().as_deref() != Some(&plist_bytes) {
        std::fs::write(&plist_dst, &plist_bytes).expect("write Info.plist");
    }

    let dylib_out = bundle_macos.join("Splitwave");

    if dylib_out.exists() && !sources_newer_than(&sources, &dylib_out) {
        return;
    }

    let mut cmd = Command::new("clang++");
    cmd.args([
        "-std=c++17",
        "-dynamiclib",
        "-arch", "arm64",
        "-arch", "x86_64",
        "-mmacosx-version-min=13.0",
        "-O2",
        "-framework", "CoreAudio",
        "-framework", "CoreFoundation",
        "-I", libaspl_dir.join("include").to_str().unwrap(),
        "-o", dylib_out.to_str().unwrap(),
    ]);
    for src in &sources {
        cmd.arg(src);
    }

    let status = cmd.status().expect("invoke clang++ for virtual driver");
    if !status.success() {
        panic!("virtual driver build failed");
    }
}

fn sources_newer_than(sources: &[PathBuf], output: &PathBuf) -> bool {
    let Ok(out_mtime) = output.metadata().and_then(|m| m.modified()) else {
        return true;
    };
    sources.iter().any(|src| {
        src.metadata()
            .and_then(|m| m.modified())
            .map_or(true, |mtime| mtime > out_mtime)
    })
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
