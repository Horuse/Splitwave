# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0]

### Added
- **Plugin hosting** - load your own plugins as effect nodes: CLAP and VST3 on
  every platform, Audio Unit on macOS. The plugin's own editor opens in a native
  window, its parameters are editable in the node itself, and its state is saved
  with the pipeline. Linux editors need an X11 session (XWayland included).
- **Separate channel routing** (#12) - each channel of a device gets its own
  handle, so you can wire, process and mix individual channels instead of fixed
  stereo pairs. Existing pipelines are migrated automatically.
- **WebRTC, Net Sender and Net Receiver nodes** (#10) - stream audio between
  machines and into WebRTC sessions.
- **Mute hotkey** - bind any key or combination to a Mute node and toggle it
  system-wide, even when Splitwave is in the background. Optionally a spoken cue
  confirms the switch on an output device of your choice, at its own volume.
- **Spectrum analyzer** node with per-channel FFT.
- **De-esser** node with presets, and a **Declick** node for removing clicks and
  crackle.
- **Effect presets** - save any effect's settings and reuse them anywhere, with
  factory presets included.
- **Pipeline templates** - start from a ready-made graph instead of an empty
  canvas.
- **Settings window** - theme, canvas behaviour, history depth, update checks
  and preset management in one place.
- Analyzer node now reports signal metrics and delivery profiles.
- Crashes are saved and shown on the next launch instead of vanishing with the
  window.
- Node icons and a refreshed modal design.

### Changed
- Plugins receive the node's full channel width instead of being forced into
  stereo.
- Effect nodes got a visual pass.

### Fixed
- Cables that point at a channel a device does not have are now flagged, and
  edges left dangling blink with a warning on the node.
- Fan-out nodes are computed once and shared, monitoring included, instead of
  being processed per branch.
- Gain-reduction readouts no longer freeze, and the monitor backlog is bounded.
- Spectrum taps keep updating in monitor mode.
- Virtual devices can be applied while the engine is idle.
- Node previews render without a flow canvas open.
- macOS: the private CATap capture aggregate no longer shows up in device menus.

## [0.5.0] - 2026-06-07

### Added
- Numeric readouts on sliders are now editable.
- EQ faders snap in 0.1 dB steps for finer control.

### Fixed
- Virtual audio device no longer produces garbled audio when used as a
  microphone, including when several apps capture it at once (stream format
  mismatch).

## [0.4.0] - 2026-06-05

### Added
- Noise Suppressor exposes advanced controls: a DeepFilterNet post-filter toggle
  and adjustable processing thresholds.
- New app icons and refreshed branding.

### Fixed
- Audio file playback no longer drops audio under bursty load - file sources
  keep their backlog instead of being trimmed.
- "Reveal in folder" works for recordings, and transport buttons have tooltips.
- Level meter readout settles at the dB floor on silence instead of drifting to
  -inf.
- "Check for Updates" reports the actual failure cause instead of a bare
  "builder error", with a one-click button to copy it.

## [0.3.0] - 2026-06-03

### Added
- Full Linux support (#2) and Windows support (#4), both experimental.
- Noise Suppressor node for speech denoising, powered by DeepFilterNet3.

### Fixed
- Errors show as toast notifications instead of in the header.
- Lower and more stable microphone latency.
- Correct output device selection for Bluetooth headsets with identical names.

## [0.2.0] - 2026-05-20

### Added
- Multi-format audio file decoding - the Audio File node plays more than WAV.
- Audio File transport controls: pause, stop and skip.
- Input volume control for App Audio, System Audio and Audio File.
- Separate Monitor category - meter nodes that work without an output
  connection.

### Fixed
- Speaker: more reliable device selection, by always enumerating devices.
- Speaker: the output stream recovers when the system default device switches.
- Updater: the up-to-date modal shows only on a manual menu check.
- Virtual device: `devices.plist` is set to 644 so `_coreaudiod` can read it.

## [0.1.1] - 2026-05-18

### Added
- macOS Intel (x86_64) support alongside Apple Silicon.

### Fixed
- Crash when stopping audio capture.
- Audio recording stability issues.

## [0.1.0] - 2026-05-18

Initial release.

### Added
- Inputs: microphone, system audio capture, per-app audio capture, audio file
  playback with loop and auto-stop.
- Outputs: speaker, and file recording in WAV, FLAC, AIFF, Opus, MP3 and AAC.
- Effects: Gain, Mute, Channel Balance, 10-band graphic EQ, Compressor, Limiter,
  Noise Gate, Saturator, Delay, Reverb, Level Meter, LUFS Meter, Waveform.
- Node graph routing with hot reconcile: edit the graph while the pipeline runs
  without interrupting streams.
- Undo/redo, node copy/paste, pipeline snapshot history, auto-update and the
  virtual audio driver.

[1.0.0]: https://github.com/Horuse/Splitwave/compare/v0.5.0...v1.0.0
[0.5.0]: https://github.com/Horuse/Splitwave/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/Horuse/Splitwave/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Horuse/Splitwave/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Horuse/Splitwave/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/Horuse/Splitwave/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Horuse/Splitwave/releases/tag/v0.1.0
