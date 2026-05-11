// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Embed Info.plist into the binary so macOS reads NSMicrophoneUsageDescription
// even in dev mode (where the binary runs directly, without a bundled .app).
// The bundled .app uses the same Info.plist via bundle.macOS.infoPlist.
#[cfg(target_os = "macos")]
embed_plist::embed_info_plist!("../Info.plist");

fn main() {
    betteraudio_lib::run()
}
