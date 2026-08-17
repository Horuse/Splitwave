# Microphone Array

Microphone Array is an input node that turns two or more physical microphone
channels into one processed mono stream named `Spatial Voice`. Add the node from
the input-node picker, open **Setup**, choose the channels, describe their
positions, set a fixed target, and calibrate. The output connects to ordinary
Splitwave effects, recorders, speakers, or an external virtual cable exactly
like any other source.

The processing is local. Splitwave does not upload microphone audio, and the
array setup does not save raw calibration or live audio unless you explicitly
connect the graph to a recording output.

## Recommended hardware

A wired multichannel audio interface is the recommended configuration. Channels
captured by one device share a hardware clock, so Splitwave opens one stream and
preserves their relative timing.

Multiple wired USB microphones can also form an array. Each device has an
independent clock, so Splitwave estimates drift and continuously adjusts one
asynchronous sample-rate converter per slave clock domain. This mode is marked
**Experimental** because device drivers, clock quality, and operating-system
scheduling make it less stable than one multichannel interface.

Bluetooth microphones are unsupported. Their variable transport latency,
codec buffering, and device-side processing do not provide the stable channel
timing required by this feature.

Disable device AGC, noise suppression, echo cancellation, and other audio
enhancements wherever possible. Different processing on individual channels
changes gain, phase, and delay, which harms calibration and spatial filtering.

## Add sources and channels

Open **Setup** on the Microphone Array node. You can add one selected channel or
all available channels from a physical input device. The summary shows two
different counts:

- `N` is the number of enabled microphone channels.
- `K` is the number of independent capture clock domains.

For example, an eight-channel interface is `N=8, K=1`; a four-channel interface
plus two USB microphones is `N=6, K=3`.

At least two enabled, non-excluded channels are required. A source that is
temporarily missing remains in the saved setup so it can reconnect later.
Choose one source as the master when `K>1`; the other domains synchronize to it.

## Geometry

Coordinates are in metres. Use the physical acoustic centre of each microphone,
not the edge of its enclosure. Setup provides four geometry modes:

- **Linear** spaces microphones along one axis.
- **Circular** places them around a centre.
- **Rectangular** lays them out as rows and columns.
- **Custom** exposes the `x`, `y`, and `z` coordinate of every member.

Accurate spacing matters. Recalibrate after moving any microphone, changing a
channel mapping, changing an input device, or enabling device processing.

## Fixed target

The target can be a direction (azimuth and elevation) or a point (`x`, `y`,
`z`). The first release uses a fixed target. Moving-source localization and
tracking are not implemented.

Two microphones do not uniquely determine a point in space. More generally,
any source with the same steering delays/TDOA as the target may pass through the
array. A point target therefore describes the expected propagation delays; it
is not speaker identification or guaranteed source isolation.

## Calibration

Place a broadband calibration source at the configured target, keep the room
quiet, and run **Calibrate**. Splitwave captures three seconds, measures
pairwise GCC-PHAT delays, solves a weighted global delay graph, estimates gain
and polarity, and assigns channel quality. Review the Array Quality score and
any marginal or excluded members before relying on adaptive processing.

The saved calibration includes a topology/format fingerprint. A device,
channel, geometry, target, or stream-format change marks incompatible
calibration for review instead of silently treating it as current.

## Algorithms

- **Auto** uses MVDR only after good calibration and stable synchronization;
  otherwise it uses Delay-and-Sum.
- **Delay-and-Sum** applies calibration and steering delays, then averages all
  healthy channels. It is the cheapest and most predictable option.
- **GSC** uses a generalized sidelobe canceller with a soft adaptive path. With
  two channels its blocking signal is the familiar sum/difference form.
- **MVDR** uses a 512-frame STFT with a 256-frame hop, diagonal loading, and a
  per-frequency Delay-and-Sum fallback when the covariance solve is unsafe.
- **Postfilter** is optional and soft; it does not use a destructive hard mask.

Delay-and-Sum adds the fractional-delay filter latency. The frequency-domain
path adds a 256-frame algorithmic latency (about 5.3 ms at 48 kHz), reported by
the node and included in Splitwave's graph delay compensation. Independent
clock domains also target a 3,072-frame synchronization buffer (64 ms at
48 kHz).

## A/B monitoring and diagnostics

While a pipeline is running, the Diagnostics section can monitor the best
single microphone, raw calibrated sum, stable Delay-and-Sum, or the selected
spatial algorithm. Switching is crossfaded; it does not add graph outputs or
duplicate array processing.

Diagnostics report the requested and active algorithm, worker load and deadline
misses, array state, fallback reason, algorithmic/synchronization latency,
calibration quality, per-member levels and health, and per-domain rate ratio,
estimated ppm, ring fill, correction, underflow/overflow counts, and lock
progress. Reported values come from runtime counters, not estimates invented by
the UI.

During startup, drift recovery, overload, a missing domain, or an algorithm
failure, the worker crossfades to a safe delayed signal or stable
Delay-and-Sum. A recoverable failure does not intentionally leave permanent
silence. Fix the reported cause; the worker resynchronizes when the domain is
healthy again.

## Factory pipeline

`Spatial Voice — Multi-Mic` has stable ID `spatial_voice_multimic` and version
`1`. It creates one unconfigured Microphone Array, then a separate Noise
Suppressor, Compressor, EQ, and unassigned Speaker output. The array
defaults to Delay-and-Sum. It does not hard-code devices, channel count, or a
virtual cable; open array Setup before activation.

## Limitations

- Spatial filtering attenuates signals by direction/delay, not by identity. A
  source with the same TDOA can pass as the target.
- Low frequencies have small phase differences across compact arrays and are
  separated weakly.
- Reverberation creates delayed copies from many directions and can cause
  target leakage, coloration, or reduced rejection.
- Channel mismatch and different AGC/NS/enhancement settings reduce quality.
- Recalibration is required after moving the microphones or target.
- Bluetooth inputs are unsupported.
- Independent USB devices are less stable than a shared-clock interface and
  remain Experimental.
- Large-N MVDR has substantially higher CPU and memory cost than
  Delay-and-Sum. CPU overload automatically falls back.
- The target is fixed; moving-target tracking and speaker identification are
  not implemented.
- The graph boundary remains stereo internally, so the mono array result is
  duplicated only at that boundary for compatibility.

## Troubleshooting

**Setup says a source is missing.** Reconnect the exact device, rescan devices,
and verify its saved channel indices still exist. The source remains persisted.

**Calibration is missing or needs review.** Restore the original topology and
format, or recalibrate. Confirm the geometry and target first.

**Array stays in Syncing or Domain unlocked.** Prefer a shared-clock interface;
otherwise choose the most stable wired device as master, close other apps that
own the microphones, and inspect ppm, ring fill, underflows, and overflows.

**Output falls back.** Check the fallback reason. Restore missing devices,
exclude a failed channel, reduce CPU load, or choose Delay-and-Sum for a large
array.

**Target rejection is weak.** Measure geometry again, disable device
enhancements, recalibrate, increase useful microphone spacing, reduce room
reverberation, and remember the same-TDOA and low-frequency limits.

For repeatable acceptance steps, see the
[manual hardware test guide](microphone-array-hardware-test.md). Developers
should also read the [architecture and provenance](microphone-array-architecture.md).
