# RULES.md

Universal rules that apply to every change in this repository.
Architecture background: [CONCEPT.md](CONCEPT.md).
Feature specifications: [FEATURES.md](FEATURES.md).
UI Architecture and Runes conventions: [UI.md](UI.md).
PR checklist and testing: [CONTRIBUTING.md](../CONTRIBUTING.md).

---

## Sections

- [File Size and Decomposition](#file-size-and-decomposition)
- [Cross-Platform Abstraction](#cross-platform-abstraction)
- [No Silent Fallback and Error Handling](#no-silent-fallback-and-error-handling)
- [Real-Time (RT) Audio Path Invariants](#real-time-rt-audio-path-invariants)
- [Comments and Documentation](#comments-and-documentation)
- [Formatting and Linting](#formatting-and-linting)
- [Commits](#commits)
- [Verification Checklist](#verification-checklist)

---

## File Size and Decomposition

- **Line Budget**: Target < 400 lines per file; hard ceiling of 500 lines.
- When a file exceeds 500 lines, it must be decomposed into a dedicated module folder with a clean `mod.rs` and single-responsibility submodules.
- Avoid monolith dumping grounds (e.g. putting all IPC commands or all effects in one file). Group by domain and lifecycle.
- In Svelte components, separate complex Canvas drawing, event history, or specialized sub-controls into helper classes (`.ts`) or internal sub-components (`_prefixed.svelte`).

---

## Cross-Platform Abstraction

- Platform-specific files (`macos.rs`, `windows.rs`, `linux.rs`) must contain **only** code directly interfacing with OS-specific APIs (CoreAudio/SCK, WASAPI/SetupAPI, PipeWire/PulseAudio).
- Shared logic (e.g., CPAL device enumeration and stream building, caching layers, input validation, channel conversion, data normalization) MUST live in `mod.rs` or a shared helper.
- If identical code appears in two or more platform files, factor it into `mod.rs` immediately.
- A platform that cannot support a given capability returns a typed `AppError`; it never pretends to succeed or quietly substitutes another device or sample rate.

---

## No Silent Fallback and Error Handling

- **One deterministic path per decision**: Surface failures to the user. Never substitute a different audio device, sample rate, or channel format behind the user's back.
- **Typed Errors**: Use `AppError` variants (`Host`, `Device`, `Stream`, `Validation`, `Plugin`).
- **No Uncontrolled Panics**: Never call `.unwrap()` or `.expect()` in runtime code paths, especially inside IPC commands or audio threads.
- Error states must propagate cleanly to the UI through explicit events (`audio://input_error`, `audio://speaker_error`, `error://panic`) rather than failing silently.

---

## Real-Time (RT) Audio Path Invariants

The real-time path comprises all callbacks executed by cpal, ScreenCaptureKit, CoreAudio, PipeWire, WASAPI, and the inner loop of `DspWorker::run`.

### Forbidden inside RT Audio Path:

- ❌ **Allocations**: Growing vectors (`Vec::push`, `Vec::resize`), strings (`String::from`), box allocations (`Box::new`), hash maps.
- ❌ **System Locks**: `Mutex::lock`, `RwLock::write`. (Only lock-free atomic swaps or non-blocking `try_lock` if dropping a block is strictly acceptable).
- ❌ **Syscalls and I/O**: File access, sockets, logging macros (`tracing::info!`, `println!`), IPC.
- ❌ **Unbounded Loops**: Catch-up loops that iterate indefinitely without yielding to the transport clock.

### Permitted inside RT Audio Path:

- ✅ **Preallocated Buffers**: Slices and arrays allocated during stream initialization.
- ✅ **Lock-Free Rings**: `rtrb` SPSC ring buffers using bulk operations (`bulk_pop`, `bulk_push`).
- ✅ **Atomics**: `Arc<AtomicU32>`, `Arc<AtomicBool>` with `Ordering::Relaxed` for runtime controls and meter telemetry.
- ✅ **Deterministic DSP**: Fixed-frame mathematical transformations and inline filter evaluations.

---

## Comments and Documentation

- **Focus on the Non-Obvious WHY**: Comments explain hidden invariants, hardware workarounds, concurrency assumptions, and mathematical rationale. Naming handles WHAT.
- **Terse and Timeless**: Describe the code as it currently exists.
- **Forbidden Comments**:
    - ❌ Never write conversational change logs ("now instead of", "was previously", "changed to fix bug", "old implementation").
    - ❌ Never narrate trivial mechanics (`// return result`, `// increment counter`).
    - ❌ Never leave abandoned `TODO` or `FIXME` without a tracking issue.
- **Mandatory Comments**:
    - ✅ Invariants on memory ordering (`Ordering::Relaxed` vs `Ordering::SeqCst`).
    - ✅ Hardware or OS quirks (e.g. CoreAudio reference cycles, Windows MTA COM initialization, PipeWire RTKit quantum semantics).
    - ✅ Buffer sizing assumptions (e.g. ring buffer capacities, FFT chunk requirements).

---

## Formatting and Linting

- Run `bun run format` (Prettier for TS/Svelte, rustfmt for Rust) before every commit.
- Tree must remain 100% clean under both formatters with zero manual formatting conflicts.
- Svelte runes must satisfy `bun run check` with 0 errors.
- Rust code must satisfy `cargo check` and `cargo test` with 0 warnings/errors.

---

## Commits

Format:

```
type(scope): subject
```

- Standard [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
- Valid types: `feat`, `fix`, `refactor`, `chore`, `docs`, `test`, `style`.
- Keep formatting-only commits separate from behavioral changes to preserve clean `git blame`.

---

## Verification Checklist

Before considering any refactoring complete:

1. `cargo check --manifest-path src-tauri/Cargo.toml` passes.
2. `cargo test --manifest-path src-tauri/Cargo.toml` passes all unit and integration tests.
3. `bun run check` passes with 0 errors.
4. If Rust structs with `#[derive(TS)]` were modified, run `bun run generate` and commit the generated types.
5. Run `bun run format`.
