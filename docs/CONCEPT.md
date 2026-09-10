# CONCEPT.md

Pro-audio routing. Tauri 2 + SvelteKit 5 (runes) frontend with a node graph,
Rust engine (cpal + rtrb + rubato + hound), CoreAudio HAL and Swift
ScreenCaptureKit bridge on macOS.

## Sections

- [RT audio path](#rt-audio-path)
- [DspWorker pacing](#dspworker-pacing)
- [Effects](#effects)
- [Layout](#layout)
- [Frontend](#frontend)
- [Rust quirks](#rust-quirks)
- [Platforms](#platforms)

## RT audio path

Inside cpal / SCK callbacks and `DspWorker::run`:

- Allocations (growing `Vec::push`, `String::from`, `Box::new`)
- Locks (`Mutex::lock`; `try_lock` only if a miss is acceptable)
- Syscalls (I/O, logging, IPC)
- Non-deterministic latency ops

Shared RT↔UI state: `Arc<AtomicU32>` (f32-as-bits, `Ordering::Relaxed`).
See `MeterHandle` / `EffectControl` in `audio/effects.rs`.

Ring buffers: `rtrb` SPSC. Use `bulk_pop` / `bulk_push`, never per-sample loops.

Resampling uses two explicit policies:

- Fixed stream rates, such as pipeline → speaker, use `FftFixedIn`. It is the
  high-throughput synchronous converter and can skip trailing physical channels
  that have never carried a route.
- A rate that follows an independent or drifting clock uses `SincFixedIn` or
  `SincFixedOut`, including ratio adjustment where the receiver owns pacing.

Dev builds require:

    [profile.dev.package.rubato]   opt-level = 3
    [profile.dev.package.realfft]  opt-level = 3
    [profile.dev.package.rustfft]  opt-level = 3

Without these, one chunk takes ~16 ms and the worker stalls.

## DspWorker pacing

- `Clock` — the single transport cadence for every worker (speaker,
  monitoring, wire sender, recording). Sleeps to a per-block deadline; a
  missed deadline produces silence, never a rate error. Recording must follow
  the wall clock, not the source — a file source decodes faster than real time
  and would otherwise over-run the encoder.
- Stall: per-source `last_pop_at`; >150 ms silence → zero-fill and proceed.
- RT promotion happens after speaker startup prefill. Every overdue Linux tick
  performs a real blocking sleep before more DSP work; an unbounded catch-up
  loop is forbidden even when the ring can absorb it.

## Effects

- `RuntimeEffect` enum dispatch, no `Box<dyn>` — LLVM inlines per variant.
- Params: `Arc<AtomicU32>` cells shared with `EffectControl`. UI writes;
  RT reads next block.
- Live updates: `update_effect(node_id, data)` Tauri command →
  `EffectControl::apply_update(&Value)`.
- Only `LevelMeter` publishes telemetry back (peak/RMS atomics, tick thread).

## Layout

    src/lib/modules/
      audio/     methods.ts, stores.svelte.ts, types.ts, ui/
      flow/      ui/ (xyflow nodes; node.svelte wrapper, editor, sidebar)
      form/      ui/ (combobox.svelte etc.)
      pipeline/  methods.ts, stores.svelte.ts, types.ts
      theme/     stores.ts

Each module's `index.ts` is a barrel. Module-internal files are
underscore-prefixed (`_slider.svelte`).

## Frontend

- Svelte 5 runes only. No `export let`, no stores in component scope.
- xyflow nodes wrap with `Wrapper` from `flow/ui/node.svelte`
  (`accent`, `hasInput`, `hasOutput`).
- Interactive elements inside nodes: `nodrag nopan` (+ `nowheel` if scrollable).
- Numeric readouts: `font-mono tabular-nums`.
- IDs: `@paralleldrive/cuid2`. Not nanoid, not uuid.
- Never serialise `-Infinity` / `NaN` over Tauri — `serde_json` emits `null`,
  which fails `isFinite()`. Use a sentinel (e.g. `-120` dB floor) or send
  amplitude.

## Rust quirks

- `audio/macos_hal.rs` — custom CoreAudio FFI. `cpal`'s
  `supported_*_configs` hides non-default routes; `default_*_config`
  errors on inactive routes.
- `audio/sck_capture.rs` — FFI to a Swift static lib in `native/`, built
  via direct `swiftc` in `build.rs`. Do not add the `screencapturekit`
  crate — its `swift build` fails on host CLT due to a `PackageDescription`
  dylib ABI mismatch.
- `audio/permission.rs` — `CGPreflightScreenCaptureAccess` (non-prompting).
- `audio/recorder.rs` — `hound`, f32 stereo PCM, periodic flush.

## Platforms

Three real backends: CoreAudio + ScreenCaptureKit (macOS), PipeWire (Linux),
WASAPI (Windows). `device/`, `capture/`, `volume/`, `virtual_device/`,
`pipeline/input/`, `pipeline/output/` each carry one file per OS — a change
in one usually needs the other two.

A backend that cannot support the feature returns an error; it does not
substitute a different rate, device, or format.

### Linux: PipeWire and RTKit

- PipeWire process callbacks and promoted DSP workers are RT code. They may
  touch only preallocated buffers, SPSC rings and relaxed atomics.
- `audio_thread_priority` obtains real-time scheduling through RTKit. Its frame
  argument is the maximum uninterrupted render quantum, not a latency target.
  Pass the actual known block size; pass `0` when PipeWire owns an unknown
  callback quantum.
- Linux `RLIMIT_RTTIME` measures CPU time spent under real-time scheduling
  without a blocking syscall. Crossing the soft limit sends `SIGXCPU`; crossing
  the hard limit sends `SIGKILL`. Preemption and `sched_yield` do not reset it.
  Startup prefill runs before promotion, and deadline catch-up must block
  between blocks. Never raise or disable the OS limit to hide an overload.
- A `SIGXCPU` followed by `SIGKILL` from the audio thread with `si_code=SI_KERNEL`
  is an RT-budget failure. A Rust panic hook and in-process crash modal cannot
  observe `SIGKILL`; preserve the previous-run unexpected-exit report.

### macOS: CoreAudio

- CoreAudio owns the device callback cadence. Callback code follows the common
  RT rules and must return within the negotiated buffer duration.
- Time-constraint scheduling and Audio Workgroups express period, computation
  and deadline to Darwin. A deadline miss normally appears as an overload or
  audio dropout; Linux `RLIMIT_RTTIME` semantics do not apply.
- Device sample-rate or channel-layout changes require rebuilding the stream.
  Do not retain callbacks, HAL objects or plugin UI objects past their documented
  owner lifetime.

### Windows: WASAPI

- WASAPI owns the render callback cadence. Use the negotiated mix/device format
  exactly and rebuild after endpoint invalidation or format change.
- Time-critical audio work belongs to MMCSS/Pro Audio scheduling. Do not use
  generic process or thread priority boosts as a substitute, and never block,
  allocate or perform COM/UI work in the render callback.
- COM initialization and endpoint management remain on control threads. The
  callback exchanges audio and status only through preallocated buffers and
  lock-free state.

### Cross-platform output contract

- The physical stream always receives its native channel width. Sample-rate
  conversion may stop at the highest routed channel; remaining channels are
  explicitly zero-filled before the device ring.
- Callback scratch storage is allocated before playback. Oversized callbacks
  are processed in channel-aligned chunks and never grow a `Vec` on the audio
  thread.
- Fixed-rate conversion, drift correction and channel mapping are separate
  decisions. Do not choose a resampler merely because two nominal rates differ.
