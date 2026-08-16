Pro-audio routing. Tauri 2 + SvelteKit 5 (runes) frontend with a node graph,
Rust engine (cpal + rtrb + rubato + hound), CoreAudio HAL and Swift
ScreenCaptureKit bridge on macOS.

Contributing rules (diff hygiene, per-OS testing, PR checklist):
`CONTRIBUTING.md`. Build and run: `DEVELOPMENT.md`.

## Style

- Smallest viable change. Long specs are upper bounds — slice them.
- Touch only the lines the change requires.
- Formatting is enforced: `bun run format` (Prettier + rustfmt) before every
  PR. The tree is clean under both, so it produces no churn.
- No drive-by renames, import reordering, or refactors of code the change
  does not otherwise touch. Separate PR.
- One deterministic path per decision. Surface failures, no silent fallback.
- Comments only for non-obvious WHY (hidden constraint, invariant, workaround).
  Naming handles WHAT. Terse, one line. Section dividers only in files >500 lines.
- Comments describe the code as it stands, never the edit or our conversation.
  No "now / instead of / previously / was", no narrating a change to the reviewer.

## RT audio path — forbidden

Inside cpal / SCK callbacks and `DspWorker::run`:

- Allocations (growing `Vec::push`, `String::from`, `Box::new`)
- Locks (`Mutex::lock`; `try_lock` only if a miss is acceptable)
- Syscalls (I/O, logging, IPC)
- Non-deterministic latency ops

Shared RT↔UI state: `Arc<AtomicU32>` (f32-as-bits, `Ordering::Relaxed`).
See `MeterHandle` / `EffectControl` in `audio/effects.rs`.

Ring buffers: `rtrb` SPSC. Use `bulk_pop` / `bulk_push`, never per-sample loops.

Resampling: `rubato` `SincFixedIn`. Dev builds require:

    [profile.dev.package.rubato]   opt-level = 3
    [profile.dev.package.realfft]  opt-level = 3
    [profile.dev.package.rustfft]  opt-level = 3

Without these, one chunk takes ~16 ms and the worker stalls.

## Effects

- `RuntimeEffect` enum dispatch, no `Box<dyn>` — LLVM inlines per variant.
- Params: `Arc<AtomicU32>` cells shared with `EffectControl`. UI writes;
  RT reads next block.
- Live updates: `update_effect(node_id, data)` Tauri command →
  `EffectControl::apply_update(&Value)`.
- Only `LevelMeter` publishes telemetry back (peak/RMS atomics, tick thread).

## DspWorker pacing

- `Clock` — the single transport cadence for every worker (speaker,
  monitoring, wire sender, recording). Sleeps to a per-block deadline; a
  missed deadline produces silence, never a rate error. Recording must follow
  the wall clock, not the source — a file source decodes faster than real time
  and would otherwise over-run the encoder.
- Stall: per-source `last_pop_at`; >150 ms silence → zero-fill and proceed.

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

## Commits

- `type(scope): subject` — lowercase, no trailing period.
- Body usually omitted.

## Platforms

Three real backends: CoreAudio + ScreenCaptureKit (macOS), PipeWire (Linux),
WASAPI (Windows). `device/`, `capture/`, `volume/`, `virtual_device/`,
`pipeline/input/`, `pipeline/output/` each carry one file per OS — a change
in one usually needs the other two.

State in the PR which OS the change was developed on, which it was actually
tested on, and what was done to test it. Untested platforms are named, not
implied. A backend that cannot support the feature returns an error; it does
not substitute a different rate, device, or format.

## When in doubt

- Read `MEMORY.md`.
- Read the current code, not earlier explanations.
- RT path change → `cargo check`.
- Svelte change → `bun run check`.
- Rust `#[derive(TS)]` change → `bun run generate`, commit the generated
  files with the Rust change.
