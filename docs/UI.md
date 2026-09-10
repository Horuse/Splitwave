# UI Building Guidelines

Practical reference for building graph nodes, controls, and settings screens in Splitwave. Follow this to maintain visual parity with existing components.

---

## 1. Graph Node Anatomy

Every node in the graph editor must follow this exact layout contract:

```svelte
<script lang="ts">
  import { useSvelteFlow, type NodeProps } from '@xyflow/svelte';
  import Wrapper from '../node.svelte';
  import Slider from './_slider.svelte';
  import { SomeIcon } from '$lib/components/icons';

  let { id, data }: NodeProps<MyNodeType> = $props();
</script>

<Wrapper label="My Node" icon={SomeIcon} accent="effect" hasInput hasOutput channelIo nodeId={id}>
  <div class="nowheel nodrag flex w-48 flex-col gap-1.5">
    <Slider
      label="Parameter"
      value={data.param}
      min={-24}
      max={24}
      step={0.5}
      unit=" dB"
      defaultValue={0}
      onChange={setParam}
    />
  </div>
</Wrapper>
```

### Width & Sizing Rules:
- **Base Width**:
  - `w-48` for simple nodes (Gain, Mute, Delay).
  - `w-52` for detailed nodes (Compressor, EQ).
  - Uncapped width (`wide={true}`) is allowed **only** for full-width visualizers (Waveform Scope, Spectrum, Multi-channel Level Meters).
- **Vertical Rhythm**: Controls stack with `flex flex-col gap-1.5`.
- **Canvas Drag Isolation**: All interactive inputs, buttons, sliders, and steppers **must** include CSS classes `nodrag nopan`. Any scrollable sub-container must also include `nowheel`.

---

## 2. Category Color Accents

Every node is color-coded by its functional category via the `accent` prop on `Wrapper`:

| Category | `accent` Prop | Text & Icon Class | Role in Graph |
| :--- | :--- | :--- | :--- |
| **Input** | `"input"` | `text-emerald-600 dark:text-emerald-400` | Microphones, system audio, file player |
| **Effect** | `"effect"` | `text-violet-600 dark:text-violet-400` | EQ, compressor, gate, reverb, plugins |
| **Output** | `"output"` | `text-sky-600 dark:text-sky-400` | Speakers, headphones, file recorder |
| **Monitor** | `"monitor"` | `text-amber-600 dark:text-amber-400` | Level meter, LUFS meter, waveform, spectrum |
| **Network** | `"network"` | `text-rose-600 dark:text-rose-400` | WebRTC collaborator, network sender/receiver |

---

## 3. Reusable Control Components Catalog

Do not hand-roll custom inputs or sliders. Reuse standard primitives:

1. **`Slider` (`src/lib/modules/flow/ui/effect/_slider.svelte`)**:
   - Standard control for continuous parameters (dB, ms, Hz, %).
   - Always supply `defaultValue` (double-clicking the track resets to it).
   - Double-clicking the numeric badge opens inline text input.
2. **`SegmentedButtons` (`src/lib/components/segmented_buttons.svelte`)**:
   - Mode switcher for 2 to 4 mutually exclusive states (e.g. `Stereo | Mono`, `New | Overwrite | Append`).
3. **`NumberStepper` (`src/lib/components/number_stepper.svelte`)**:
   - Stepper with `+` / `-` buttons for discrete integers (channel count, snapshot limits).
4. **`Combobox` + `RescanButton` (`src/lib/modules/form/ui/`)**:
   - Dropdown with search for hardware devices, audio formats, or apps. Always place `RescanButton` adjacent when enumerating audio endpoints.
5. **`PresetBar` (`src/lib/modules/preset/ui/preset_bar.svelte`)**:
   - Placed at the bottom of effect nodes for loading factory and user presets.
6. **`Tooltip` (`src/lib/modules/overlay/ui/`)**:
   - Hover tooltips for abbreviations, routing indicators, and warnings.

---

## 4. Typography & Audio Readouts

- **Tabular Monospace Numbers**:
  Every dB readout, frequency, latency figure, millisecond duration, or timer **must** use:
  ```html
  <span class="font-mono tabular-nums text-xs">...</span>
  ```
  This prevents layout jitter as numbers fluctuate in real time.
- **Value Formatting**:
  Use format helpers from `src/lib/components/format.ts`:
  - `formatHz(48000)` -> `"48 kHz"`
  - `formatDuration(sec)` -> `"00:15"`
  - `formatSize(bytes)` -> `"12.4 MB"`

---

## 5. Visualizers & Curves

When displaying audio graphs, transfer curves, or level meters:
- Place the visualizer inside a recessed well for contrast:
  `rounded-lg border border-neutral-400/50 bg-neutral-100/60 p-1.5`.
- SVG curve dimensions should be fixed (e.g. `130x60` for compressor transfer curves).
- Line colors on SVG:
  - Grid / axes: `stroke-neutral-400/40`
  - Active response curve: category accent color (e.g. `stroke-violet-600 dark:stroke-violet-400`).

---

## 6. Settings Pages & Dialogs

For standalone views outside the graph editor (`settings/+page.svelte`, `virtual-devices/+page.svelte`):
- **Section Layout**:
  ```html
  <section class="flex flex-col gap-3">
    <div>
      <h2 class="text-sm font-semibold text-theme">Section Title</h2>
      <p class="text-xs text-neutral-900">Brief explanation of setting.</p>
    </div>
    <!-- Options grid -->
  </section>
  ```
- **Option Selection Cards (Grid buttons)**:
  - Grid: `grid grid-cols-2 gap-2` or `grid-cols-3 gap-2`.
  - Active state: `border-neutral-900 bg-neutral-200 text-theme`.
  - Inactive state: `border-neutral-400 bg-neutral-100 text-neutral-1000 hover:bg-neutral-200`.
