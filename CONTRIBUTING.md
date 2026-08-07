# Contributing

Thanks for helping out. Build and run instructions live in
[DEVELOPMENT.md](DEVELOPMENT.md); the engine's design rules live in
[AGENTS.md](AGENTS.md). This file is about what a reviewable pull request
looks like.

## Keep the diff to the change

A pull request should touch the lines its stated purpose requires and nothing
else. A 300-line feature buried in 1000 lines of unrelated churn cannot be
reviewed, so it will be sent back to be split even when the feature itself is
good.

**Formatting is mandatory.** The whole repo is formatted with Prettier
(`.prettierrc`) and rustfmt (`rustfmt.toml`). Run both before you open a PR:

```bash
bun run format   # Prettier over ts/svelte/md/json, then rustfmt
```

Because the tree is already clean under both, this produces no churn — it
only normalises the lines you wrote. Format-on-save is safe here and
encouraged. Do not hand-format against the tool, and do not commit changes
that the formatter would immediately revert.

Also out of scope unless the change is the point of the PR:

- Renaming things you are not otherwise modifying
- Reordering imports, fields, or functions
- Tailwind class reordering
- "While I was in here" refactors
- Regenerating lockfiles or `src/lib/modules/pipeline/generated/` without a
  matching Rust change

If you spot something worth fixing along the way, open a separate PR or an
issue. Both are welcome — just not in the same diff.

## Test on the platforms you can, and say which

This is a native audio app with three separate backends: CoreAudio and
ScreenCaptureKit on macOS, PipeWire on Linux, WASAPI on Windows. Code that
looks platform-neutral often is not, and a device path that works on one OS
routinely fails on another.

Ideally, test your change on macOS, Linux and Windows. Most people cannot, and
that is fine. What is **not** fine is leaving it unsaid.

Every PR description must state which platform you developed on, which
platforms you actually tested on, and what you did to test:

```
Developed on: macOS 15.2 (Apple Silicon)
Tested on:    macOS only — added a 96 kHz mic, recorded 30 s to WAV,
              verified the file rate and no xruns in the log
Not tested:   Linux, Windows
```

The maintainer will cover the platforms you could not. That works, but it is
manual and slow, so a PR that needs all three verified will sit longer than
one confined to a single backend. Say so up front and the wait is predictable;
stay silent and the PR gets closed rather than guessed at.

Pay particular attention when your change reaches these — they have a
per-OS file each, and touching one usually means touching all three:

```
src-tauri/src/audio/device/            macos.rs / linux.rs / windows.rs
src-tauri/src/audio/capture/
src-tauri/src/audio/volume/
src-tauri/src/audio/virtual_device/
src-tauri/src/audio/pipeline/input/
src-tauri/src/audio/pipeline/output/
```

If a platform genuinely cannot support what you are adding, return a real
error from that backend. Do not silently substitute a different value — see
the no-silent-fallback rule in AGENTS.md.

## Before you open the PR

```bash
bun run check                                  # svelte-check + tsc
cargo check --manifest-path src-tauri/Cargo.toml
bun run format                                 # Prettier + rustfmt
```

Both checks must pass, and `format` must leave the tree clean. If you changed a `#[derive(TS)]` type in Rust, run
`bun run generate` and commit the regenerated files in
`src/lib/modules/pipeline/generated/` together with the Rust change — never
by hand, and never on their own.

Anything persisted to disk — pipeline JSON, `virtual-devices.json`, the macOS
driver plist — has to keep loading for people upgrading. Add a `#[serde(default)]`
or a versioned migration; do not silently change the meaning of an existing
field.

## Scope and shape

- Smallest viable change. A long spec is an upper bound, not a target — if a
  feature can ship as three small PRs, ship three.
- One deterministic path per decision. Surface failures; never fall back
  silently to a different device, rate, or format.
- Comments explain non-obvious _why_, in one terse line. Naming covers _what_.
  Never write comments that narrate the edit ("now uses", "instead of",
  "previously") — they describe the code as it stands.
- If the same UI block or helper appears twice, factor it out before opening
  the PR.
- Say why in the PR description when you add a dependency.

## Real-time audio

Read the "RT audio path — forbidden" section of [AGENTS.md](AGENTS.md) before
touching anything inside a cpal or ScreenCaptureKit callback, or inside
`DspWorker::run`. No allocations, no locks, no syscalls, no non-deterministic
latency. This is the one area where a PR gets rejected on principle rather
than on taste — a glitch here is audible to every user.

## Commits

```
type(scope): subject
```

Following [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
Lowercase, no trailing period, body usually omitted. Types in use: `feat`,
`fix`, `chore`, `refactor`, `style`, `docs`. Keep formatting-only commits
separate from behavioural ones so they can be reviewed at a glance and
skipped in `git blame`.
