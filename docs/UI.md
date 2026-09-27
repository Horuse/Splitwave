# UI.md

How node and app UI is built. Code conventions for Svelte: [CONCEPT.md](CONCEPT.md#frontend). What a node may contain: [FEATURES.md](FEATURES.md).

## Sections

- [Storybook is the catalogue](#storybook-is-the-catalogue)
- [Reuse before you build](#reuse-before-you-build)
- [Adding a node](#adding-a-node)
- [Adding a shared component](#adding-a-shared-component)
- [Mocked engine](#mocked-engine)

## Storybook is the catalogue

`bun run storybook` opens every node and every shared component on :6006, driven by a mocked engine. Start there before writing UI: it shows what already exists and how each piece looks in every state, in light and dark.

| Folder                    | Contents                                             |
| ------------------------- | ---------------------------------------------------- |
| `src/stories/nodes/`      | One file per node kind, plus `Nodes/Gallery`         |
| `src/stories/forms/`      | Inputs: combobox, toggle, slider, stepper, segmented |
| `src/stories/components/` | Display pieces: meter bar, signal bars, copy button  |
| `src/stories/harness/`    | Node canvas, mocked engine, fake signal, arg helpers |

## Reuse before you build

- Pick controls from Storybook first. A node that needs a dropdown uses `Combobox`, a parameter uses the effect `Slider`, an on/off uses `Toggle`, a small choice uses `SegmentedButtons`.
- If nothing fits, extend the existing component rather than cloning it into the node.
- A new visual element used, or likely to be used, in more than one place becomes a shared component, not markup inside a node.

## Adding a node

The node's stories land in the same commit as the node, in `src/stories/nodes/<kind>.stories.svelte`:

- Render it through `NodeCanvas` with `kind` and the story args.
- Expose every field of the node's data as a control (`dataArgs`), plus the signal (`signalArgs`), wiring (`wiringArgs`) and engine state (`envArgs`) the node reacts to.
- One named story per visual state: defaults, extremes, bypassed, unwired, multichannel, missing device or file, errors, per-platform differences.

## Adding a shared component

Its stories land in the same commit, in `src/stories/forms/` for inputs or `src/stories/components/` for everything else:

- Controls for every prop that changes how it looks.
- Controlled inputs go through `Controlled` so they respond to clicks, with changes logged in the Actions panel via `fn()`.
- One named story per state: empty, filled, disabled, compact size, overflow (long labels, long lists).

## Mocked engine

Stories never call the real Tauri backend. `harness/backend.ts` answers every command the UI sends, `harness/signal.ts` emits scope, meter, loudness, gain-reduction and transport events. When a node needs a new command or event, add it there with an `envArgs` / `signalArgs` control; do not stub it inside a single story.
