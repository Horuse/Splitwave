# FEATURES.md

How a new feature should look before it is proposed. This exists so
contributors and the maintainer share the same picture of what fits the app
and what does not. Universal rules: [RULES.md](RULES.md). Concepts and
architecture: [CONCEPT.md](CONCEPT.md).

- [The one-question test](#the-one-question-test)
- [Node categories](#node-categories)
- [Everything lives in the node](#everything-lives-in-the-node)
- [Scope and size](#scope-and-size)
- [Per-platform behaviour](#per-platform-behaviour)
- [Feature lifecycle](#feature-lifecycle)

## The one-question test

Before proposing a feature, ask: does this belong in the app, or is it a
plugin? CLAP, VST3 and AU plugins are first-class citizens here — an effect
that only a small fraction of users will ever touch (beamforming, spatial
processing, exotic DSP) usually belongs in a plugin, not in the core graph.
In-app nodes are for the fundamentals everyone uses: capture, routing,
EQ/compression, metering, recording.

If a feature is genuinely useful but niche, make it a plugin. We do not host
plugins here — build it as a CLAP, VST3 or AU plugin, then share it with us
in Discussions. If something does not work in the plugin support, report it
instead of building the feature into the app.

## Node categories

A node is exactly one of five things:

- **Input** — brings audio in: device, system capture, per-app capture, file.
- **Effect** — transforms audio: gain, EQ, compression, reverb. One or more
  inputs on the left, processed output on the right.
- **Output** — consumes audio: speakers, recording, virtual device.
- **Monitor** — observes audio, does not transform it: Level Meter, EBU R128
  LUFS Meter, Waveform, Spectrum.
- **Network** — moves audio over the network: WebRTC collaborator, net
  receiver/sender.

If a node produces a transformed mono signal from several sources, it is an
**effect**, not an input. A node that does "input" things and "effect" things
at once is a design smell — split it or pick the dominant category.

## Everything lives in the node

- All parameters are editable directly in the node in the editor.
- No modal windows, no wizard dialogs, no tabbed setup panels.
- If a parameter does not fit in the node, the feature is too big for the
  app — shrink it or make it a plugin.
- Node settings are minimal by construction. A node that needs a half-dozen
  tabs of configuration is not a node; it is a subsystem wearing a node
  costume.

## Scope and size

- A feature is the smallest slice that still does something useful. A long
  spec is an upper bound, not a target.
- If a feature can ship as three small PRs, ship three.
- Anything that duplicates an existing subsystem (its own capture path, its
  own clock, its own fallback logic, its own calibration) is a red flag.
  Reuse what the engine already provides before building a parallel one.
- Every feature must state which platforms it is developed on, which it was
  actually tested on, and what was done to test it. Untested platforms are
  named, not implied.

## Per-platform behaviour

Device, capture, volume, virtual-device, and pipeline input/output modules
each carry one file per OS (`macos.rs` / `linux.rs` / `windows.rs`). Touching
one usually means touching all three:

```
src-tauri/src/audio/device/
src-tauri/src/audio/capture/
src-tauri/src/audio/volume/
src-tauri/src/audio/virtual_device/
src-tauri/src/audio/pipeline/input/
src-tauri/src/audio/pipeline/output/
```

A platform that cannot support the feature returns a real error from that
backend. It does not silently substitute a different rate, device, or format.

## Feature lifecycle

1. Describe the feature in one paragraph and check it against the
   [one-question test](#the-one-question-test).
2. Confirm the category: input, effect, output, monitor, or network.
3. Sketch the node's inline parameters — every one must fit in the node.
4. Check which per-OS files the change reaches.
5. Split into smallest viable PRs. Each PR follows
   [CONTRIBUTING.md](../CONTRIBUTING.md).
