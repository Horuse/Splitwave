Pro-audio routing. Tauri 2 + SvelteKit 5 (runes) frontend with a node graph,
Rust engine (cpal + rtrb + rubato + hound), CoreAudio HAL and Swift
ScreenCaptureKit bridge on macOS.

Docs live in `docs/`, not here:

- [docs/CONCEPT.md](docs/CONCEPT.md) — engine architecture: RT audio path,
  pacing, effects, layout, frontend conventions, Rust quirks, platforms.
- [docs/RULES.md](docs/RULES.md) — universal rules for every change.
- [docs/FEATURES.md](docs/FEATURES.md) — how a new feature must look
  (categories, inline params, scope, per-platform behaviour).
- [CONTRIBUTING.md](CONTRIBUTING.md) — diff hygiene, per-OS testing, PR
  checklist.
- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) — build and run.

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

## When in doubt

- Read the current code, not earlier explanations.
- RT path change → `cargo check`.
- Svelte change → `bun run check`.
- Rust `#[derive(TS)]` change → `bun run generate`, commit the generated
  files with the Rust change.
