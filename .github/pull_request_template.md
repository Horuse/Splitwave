<!--
  Thanks for contributing! Before opening the PR, please:

  - Read docs/CONTRIBUTING.md and docs/RULES.md.
  - Keep the diff to the change. No drive-by renames, reformatting, or
    refactors of code the change does not touch.
  - Run `bun run check`, `cargo check --manifest-path src-tauri/Cargo.toml`,
    and `bun run format` — all must pass and leave the tree clean.
  - If you changed a `#[derive(TS)]` type in Rust, run `bun run generate` and
    commit the regenerated files in src/lib/modules/pipeline/generated/.
-->

## What does this PR do?

_Describe the change. What problem does it solve, and how? Keep it focused — a
PR should do one thing. If the feature is new, read docs/FEATURES.md first:
does it belong in the app, or should it be a CLAP/VST3/AU plugin?_

## Why is this the right approach?

_Explain the design choice. What alternatives were considered and rejected?
If this adds a dependency, say why nothing existing covers it._

## Checklist

- [ ] Diff is limited to the change — no unrelated edits
- [ ] `bun run check` passes
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` passes
- [ ] `bun run format` leaves the tree clean
- [ ] Generated TS types are committed with the Rust change (if any)
- [ ] No new dependency without a reason in the PR description
- [ ] I read the RT audio path section of docs/CONCEPT.md and confirmed this
      change adds no allocations, locks, or syscalls to cpal / SCK callbacks
      or `DspWorker::run`

## Platform coverage

_This app has three backends (CoreAudio + ScreenCaptureKit on macOS, PipeWire
on Linux, WASAPI on Windows). Fill in what you actually did. Untested
platforms must be named, not implied._

- Developed on:
- Tested on:
- What I did to test:
- Not tested: _Linux / Windows / macOS_

_Does this change touch any per-OS file? If so, list the files and how the
other platforms are affected._

## Related

- Fixes #\_
- Closes #\_
