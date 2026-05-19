[![Downloads](https://img.shields.io/github/downloads/Horuse/Splitwave/total.svg)](https://github.com/Horuse/Splitwave/releases/latest)
# Splitwave
Splitwave is a node-based audio router for macOS. Wire microphones, system audio, per-app capture, and WAV files into a visual graph, run them through a chain of effects — EQ, compression, reverb, limiting, and more — then send the result to speakers or record it in WAV, FLAC, AIFF, MP3, Opus, or AAC.

![Splitwave preview](./preview.png)


## Installation

Download the latest `.dmg` from [Releases](https://github.com/Horuse/Splitwave/releases/latest),
open it, and drag Splitwave to Applications.

**macOS will block the app on first launch** ("cannot verify developer") because the
binary is not notarized. To allow it, run once in Terminal:

```bash
xattr -cr /Applications/Splitwave.app
```

Then open Splitwave normally.

**After each update, Screen Recording permission resets** (macOS revokes it when the binary changes and the app is unsigned). To re-grant it: open System Settings → Privacy & Security → Screen Recording, click **−** to remove Splitwave, then click **+** and add it back.

## Features

- **Inputs:** microphones, system audio, per-application audio
  (ScreenCaptureKit), WAV files, virtual device loopback
- **Outputs:** physical speakers/interfaces, file recording in WAV (16/24-bit
  PCM + 32-float), FLAC, AIFF, Opus, MP3, AAC (M4A), virtual devices
- **Effects:** Gain, Mute, Channel Balance, Saturator, 10-band Graphic EQ,
  Brick-wall Limiter with look-ahead, Compressor (with sidechain), Noise Gate
  (with sidechain), Stereo Delay, Algorithmic Reverb (Freeverb), Level Meter,
  EBU R128 LUFS Meter
- **Virtual devices:** create named virtual audio devices that appear system-wide.
  Use them to capture loopback audio from any app or to feed processed audio into
  apps that accept a microphone input (DAWs, Discord, etc.)

## Stack

- **Frontend:** Svelte, Tauri, @xyflow/svelte
- **Engine:** Rust -- `cpal` (device I/O), `rtrb` (SPSC ring buffers), `rubato`
  (resampling), `hound` (WAV), `flac-codec`, `opus`, `mp3lame-encoder`,
  `ebur128`
- **macOS-specific:** custom Swift static library for ScreenCaptureKit,
  compiled by `build.rs` via `swiftc`; CoreAudio HAL FFI for device
  enumeration; libASPL-based AudioServerPlugin for virtual device driver


## Development

### Prerequisites

- macOS 13+ with Xcode Command Line Tools (`xcode-select --install`) -- gives
  you `swiftc` and the SDKs Tauri needs.
- [Rust](https://rustup.rs) (stable toolchain)
- [Bun](https://bun.sh) (`curl -fsSL https://bun.sh/install | bash`)

### Setup

```bash
bun install
bun run tauri dev
```

### Useful commands

```bash
bun run check                      # svelte-check + tsc
bun run generate                   # regenerate TypeScript types from Rust (ts-rs)
cargo check --manifest-path src-tauri/Cargo.toml
bun run tauri build --bundles app  # local .app build
```

### Project layout

```
src/lib/modules/
  audio/        cpal/SCK bridge, device enumeration, meter store
  flow/         xyflow node graph editor, sidebar, context menu
  pipeline/     pipeline state, snapshot history, ts-rs generated types
  form/         shared form primitives (combobox, slider)
  error/        global error modal (Rust panics + JS errors)
  updater/      auto-update modal + skip-version persistence
  app_info/     OS / app version cache
src-tauri/src/
  audio/        DSP engine, effects, encoders, pipeline DAG
  commands.rs   Tauri command surface
  lib.rs        app entry, plugin wiring, panic hook
src-tauri/native/virtual_driver/
  SplitAudioDriver.cpp  AudioServerPlugin implementation (libASPL)
  Info.plist            CFPlugin manifest
```
