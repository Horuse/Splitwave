[![Downloads](https://img.shields.io/github/downloads/Horuse/Splitwave/total.svg)](https://github.com/Horuse/Splitwave/releases/latest)
[![Support](https://img.shields.io/badge/Support-donate-yellow)](https://github.com/Horuse/Splitwave#support)
# Splitwave
Splitwave is a node-based audio router for macOS, Linux, and Windows. Wire microphones, system audio, per-app capture, and WAV files into a visual graph, run them through a chain of effects — EQ, compression, reverb, limiting, and more — then send the result to speakers or record it in WAV, FLAC, AIFF, MP3, Opus, or AAC.

![Splitwave preview](./preview.webp)


## Installation

### macOS

Download the latest `.dmg` from [Releases](https://github.com/Horuse/Splitwave/releases/latest),
open it, and drag Splitwave to Applications.

**macOS will block the app on first launch** ("cannot verify developer") because the
binary is not notarized. To allow it, run once in Terminal:

```bash
xattr -cr /Applications/Splitwave.app
```

Then open Splitwave normally.

**After each update, Screen Recording permission resets** (macOS revokes it when the binary changes and the app is unsigned). To re-grant it: open System Settings → Privacy & Security → Screen Recording, click **−** to remove Splitwave, then click **+** and add it back.

### Linux

Requires a PipeWire-based audio session (default on most current distros).
Download the build for your system from [Releases](https://github.com/Horuse/Splitwave/releases/latest):

- **AppImage** — `chmod +x Splitwave_*.AppImage && ./Splitwave_*.AppImage`
- **`.deb`** (Debian/Ubuntu) — `sudo apt install ./Splitwave_*.deb`
- **`.rpm`** (Fedora/RHEL/openSUSE) — `sudo rpm -i Splitwave-*.rpm`

### Windows

Requires Windows 10 version 2004 or newer (for per-app capture) and the
[WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/)
(preinstalled on current Windows 10/11). Download the `.exe` installer from
[Releases](https://github.com/Horuse/Splitwave/releases/latest) and run it.

Virtual audio devices are not available on Windows.

## Platform support

| Feature | macOS | Linux | Windows |
| --- | :---: | :---: | :---: |
| Mic / speaker device I/O | ✅ | ✅ | ✅ |
| System audio capture | ✅ ScreenCaptureKit | ✅ PipeWire | ✅ WASAPI loopback |
| Per-app audio capture | ✅ ScreenCaptureKit | ✅ PipeWire | ✅ Process Loopback (Win10 2004+) |
| App icons in the picker | ✅ | ✅ | ✅ |
| Device volume control | ✅ | ✅ | ✅ |
| Recording: WAV / FLAC / AIFF / MP3 / Opus | ✅ | ✅ | ✅ |
| Recording: AAC (M4A) | ✅ | ❌ | ❌ |
| Virtual audio devices | ✅ AudioServerPlugin | ✅ PipeWire null-sinks | ❌ (no user-mode driver model) |
| Effects, metering, file playback | ✅ | ✅ | ✅ |

## Features

- **Inputs:** microphones, system audio, per-application audio, WAV files,
  virtual device loopback
- **Outputs:** physical speakers/interfaces, file recording in WAV (16/24-bit
  PCM + 32-float), FLAC, AIFF, Opus, MP3, AAC (M4A), virtual devices
- **Effects:** Gain, Mute, Channel Balance, Saturator, 10-band Graphic EQ,
  Brick-wall Limiter with look-ahead, Compressor (with sidechain), Noise Gate
  (with sidechain), Stereo Delay, Algorithmic Reverb (Freeverb), Level Meter,
  EBU R128 LUFS Meter
- **Virtual devices:** create named virtual audio devices that appear system-wide.
  Use them to capture loopback audio from any app or to feed processed audio into
  apps that accept a microphone input (DAWs, Discord, etc.)

System and per-app capture use **ScreenCaptureKit** on macOS, **PipeWire** on
Linux, and **WASAPI loopback** / the **Process Loopback API** on Windows. Virtual
devices are AudioServerPlugin drivers on macOS and PipeWire null-sinks on Linux;
Windows has no user-mode virtual-device model, so they are unavailable there.

## Stack

- **Frontend:** Svelte, Tauri, @xyflow/svelte
- **Engine:** Rust -- `rtrb` (SPSC ring buffers), `rubato` (resampling),
  `hound` (WAV), `flac-codec`, `opus`, `mp3lame-encoder`, `ebur128`
- **macOS:** `cpal` device I/O; custom Swift static library for ScreenCaptureKit,
  compiled by `build.rs` via `swiftc`; CoreAudio HAL FFI for device enumeration;
  libASPL-based AudioServerPlugin for the virtual device driver
- **Linux:** `pipewire` for device I/O, system/app capture, and virtual
  null-sinks; `freedesktop-desktop-entry` / `freedesktop-icons` for app icons
- **Windows:** `cpal` (WASAPI) device I/O; the `windows` crate for WASAPI
  loopback + Process Loopback capture, `IAudioEndpointVolume`, audio-session
  enumeration, and exe icon extraction (`png` for encoding)


## Development

### Prerequisites

**macOS:**

- macOS 13+ with Xcode Command Line Tools (`xcode-select --install`) -- gives
  you `swiftc` and the SDKs Tauri needs.
- [CMake](https://cmake.org) (`brew install cmake`) -- builds the bundled Opus
  encoder.

**Linux:**

- A PipeWire session and these dev packages (Debian/Ubuntu names):
  `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev`
  `libsoup-3.0-dev libpipewire-0.3-dev clang libclang-dev libasound2-dev`
  `libopus-dev`

**Windows:**

- [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/)
  with the **Desktop development with C++** workload (MSVC + Windows SDK),
  [CMake](https://cmake.org) (for the bundled Opus encoder), and the WebView2
  runtime. `winget install Rustlang.Rustup Microsoft.VisualStudio.2022.BuildTools Kitware.CMake Oven-sh.Bun`
  covers most of it.

**All platforms:**

- [Rust](https://rustup.rs) (stable toolchain)
- [Bun](https://bun.sh) (`curl -fsSL https://bun.sh/install | bash`)

### Setup

```bash
bun install
bun run tauri dev
```

### Useful commands

```bash
bun run check                          # svelte-check + tsc
bun run generate                       # regenerate TypeScript types from Rust (ts-rs)
cargo check --manifest-path src-tauri/Cargo.toml
bun run tauri build --bundles app      # local .app build (macOS)
bun run tauri build --bundles appimage # local AppImage build (Linux)
bun run tauri build --bundles nsis     # local installer build (Windows)
```

### Project layout

```
src/lib/modules/
  audio/        device enumeration, meter store
  flow/         xyflow node graph editor, sidebar, context menu
  pipeline/     pipeline state, snapshot history, ts-rs generated types
  form/         shared form primitives (combobox, slider)
  error/        global error modal (Rust panics + JS errors)
  updater/      auto-update modal + skip-version persistence
  app_info/     OS / app version cache
src-tauri/src/audio/
  capture/      system + per-app capture (macos.rs SCK / linux.rs PipeWire / windows.rs WASAPI)
  device/       device enumeration (macos.rs CoreAudio / linux.rs PipeWire / windows.rs cpal)
  volume/       device volume (macos.rs / linux.rs / windows.rs)
  virtual_device/  null-sink / driver management per OS (unsupported on Windows)
  streams/      cpal stream builders (macOS + Windows)
  playback.rs   PipeWire speaker output (Linux)
  pipeline/     DSP engine, effects, encoders, pipeline DAG (input/, output/ per OS)
src-tauri/native/virtual_driver/
  SplitAudioDriver.cpp  AudioServerPlugin implementation (libASPL, macOS)
  Info.plist            CFPlugin manifest
```

## License

Splitwave is licensed under [MIT](LICENSE).  
Third-party component notices (LGPL, MPL-2.0, etc.) are in [NOTICE](NOTICE).

## Support
If you find this app useful, consider supporting it:

- Tether USDT (TRC20): `TLhTvnn8CtVuQZruLXmRurGhR9GWd7DrWZ`
- TON: (TON) `UQCpokpaZfwmVTjKDj0LrAbEPO-65c81-MiuBQOa7lTXbMGR`
- Bitcoin (BTC): `bc1q6tusr5rht7dgmw8gqzkx7rwdg4q8932lwn2rsy`