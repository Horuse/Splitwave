# Development

## Prerequisites

**macOS:**

- macOS 13+ with Xcode Command Line Tools (`xcode-select --install`) -- gives
  you `swiftc` and the SDKs Tauri needs.
- [CMake](https://cmake.org) (`brew install cmake`) -- builds the bundled Opus
  encoder.
- [Rust](https://rustup.rs) (stable toolchain)
- [Bun](https://bun.sh) (`curl -fsSL https://bun.sh/install | bash`)

**Linux:**

- A PipeWire session and these dev packages (Debian/Ubuntu names):
  `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev`
  `libsoup-3.0-dev libpipewire-0.3-dev clang libclang-dev libasound2-dev`
  `libopus-dev`
- [Rust](https://rustup.rs) (stable toolchain)
- [Bun](https://bun.sh) (`curl -fsSL https://bun.sh/install | bash`)

**Windows:**

```
winget install Rustlang.Rustup
winget install Kitware.CMake
winget install Oven-sh.Bun
winget install Microsoft.VisualStudio.2022.BuildTools
```

The last command only fetches the installer, it does **not** select any
workload. After it finishes, open the **Visual Studio Installer** app
(Start menu), click **Modify** on Build Tools 2022, and check **Desktop
development with C++**.

## Setup

```bash
bun install
bun run tauri dev
```

## Useful commands

```bash
bun run check                          # svelte-check + tsc
bun run generate                       # regenerate TypeScript types from Rust (ts-rs)
cargo check --manifest-path src-tauri/Cargo.toml
bun run tauri build --bundles app      # local .app build (macOS)
bun run tauri build --bundles appimage # local AppImage build (Linux)
bun run tauri build --bundles nsis     # local installer build (Windows)
```

## Project layout

```
src/lib/modules/
  audio/        device enumeration, meter store
  flow/         xyflow node graph editor, sidebar, context menu
  pipeline/     pipeline state, snapshot history, ts-rs generated types
  form/         shared form primitives (combobox, slider)
  preset/       shared effect presets (factory + user)
  template/     pipeline templates for the create flow
  settings/     theme, canvas, history, updates, preset management
  error/        global error modal (Rust panics + JS errors)
  updater/      auto-update modal + skip-version persistence
  app_info/     OS / app version cache
src-tauri/src/audio/
  capture/      system + per-app capture (macos.rs SCK / linux.rs PipeWire / windows.rs WASAPI)
  device/       device enumeration (macos.rs CoreAudio / linux.rs PipeWire / windows.rs cpal)
  volume/       device volume (macos.rs / linux.rs / windows.rs)
  virtual_device/  null-sink / driver management per OS (unsupported on Windows)
  plugins/      CLAP / VST3 / AU hosts behind one interface, scan + registry
                (vst3_backend.rs loads CFBundles on macOS, DLLs/.so elsewhere)
  streams/      cpal stream builders (macOS + Windows)
  playback.rs   PipeWire speaker output (Linux)
  pipeline/     DSP engine, effects, encoders, pipeline DAG (input/, output/ per OS)
src-tauri/native/virtual_driver/
  SplitAudioDriver.cpp  AudioServerPlugin implementation (libASPL, macOS)
  Info.plist            CFPlugin manifest
```
