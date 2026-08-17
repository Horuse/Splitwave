# Microphone Array architecture and provenance

This document describes the implemented architecture, validation boundaries,
and external material reviewed while developing the feature. It is not a claim
that every hardware combination has been physically tested.

## Data model and graph boundary

`MicrophoneArrayData` stores a dynamic list of physical sources and a dynamic
list of members. A member refers to a source ID and channel index, and carries
position, enable/quality state, weight, gain, polarity, and fixed calibration
delay. The model has no left/right fields or channel-count-specific node kinds.
Validation requires at least two usable members, preserves missing sources in
serialized data, and rejects invalid mappings with an explicit error.

The graph treats Microphone Array as an input. The capture/DSP path produces
one mono spatial signal. A compatibility adapter duplicates that mono signal at
the existing stereo graph boundary; the processor itself remains mono. Metrics
and A/B monitoring travel through control/telemetry APIs, not extra graph edges.

Generated TypeScript bindings mirror the Rust enums and records. The factory
template stores its own stable ID/version as provenance in each created
pipeline so later catalog edits do not silently reinterpret an existing graph.

## Capture ownership and clock domains

Graph validation resolves every distinct physical source before activation.
The array `Capture` object then owns all physical streams, runtime controls,
metrics, the stop signal, and one worker thread. It stops producers before
joining the worker, preventing callbacks from outliving their ring consumers.

There is exactly one native capture stream and one bounded SPSC ring per
physical source. All selected channels of a multichannel device stay
interleaved in that stream and form one clock domain. Windows and macOS use
CPAL/WASAPI or CPAL/CoreAudio; Linux uses a configured PipeWire stream. Capture
callbacks only bulk-copy complete interleaved frames into preallocated bounded
rings and update atomics. They do not run FFT, calibration, spatial DSP, file
I/O, logging, IPC, or blocking locks.

The worker processes 256 frames at a time. Each domain has one multichannel
resampler. The master fixes transport cadence. Each slave domain has one timing
model and one dynamic ASRC ratio driven by ring occupancy; all of that domain's
channels share the mapping so their internal phase relationship is retained.
Correction is bounded to 1,000 ppm and slewed by at most 5 ppm per update. The
target fill is 3,072 frames, and 20 stable updates are required before lock.

A discontinuity, underrun, or device error resets that domain's synchronizer.
Missing slave samples are zero-filled only as a transient alignment-preserving
recovery measure; failures remain visible in metrics.

## Calibration

Calibration captures three seconds through the same physical-domain plan. It
computes pairwise GCC-PHAT measurements, rejects weak/outlier edges, and solves
relative channel delays over the weighted connected graph instead of trusting
one reference pair. It also estimates per-channel gain and polarity, assigns
quality, and emits an aggregate score and residual delay.

The calibration fingerprint covers the source/channel topology, stream format,
geometry, and target. Runtime adaptive processing requires a matching Ready
calibration with a score of at least 60. Changed inputs become Needs review;
calibration is never silently deleted or accepted as current.

## Spatial processor

Geometry steering supports fixed far-field direction and fixed near-field point
targets. Propagation uses 343 m/s and produces non-negative relative delays.
A third-order fractional-delay interpolator applies calibration plus acoustic
steering. Buffers are allocated when the worker is built, not while processing.

**Delay-and-Sum** aligns healthy channels and computes a weighted normalized
sum. It is also the stable fallback used during synchronization or when a more
expensive algorithm cannot run safely.

**GSC** builds blocking references from the aligned channels and updates a soft
adaptive cancellation path. Two channels reduce to a sum/difference blocking
structure. Adaptation is disabled when synchronization is not stable, avoiding
learning clock errors as acoustics.

**MVDR** uses a 512-frame Hann-window STFT with 256-frame hop/overlap. It updates
the spatial covariance, uses diagonal loading, solves the complex linear system
without forming an explicit inverse, and falls back per frequency bin to
Delay-and-Sum weights when the solve or normalization is unsafe. Output overlap
is reset after discontinuity. The optional postfilter is a bounded soft gain,
not a hard time-frequency mask.

Auto selects MVDR only for a Ready calibration and locked domains. Requested
algorithm, active algorithm, fallback bins, and latency are independently
reported. Algorithm changes and all fallback transitions use a 20 ms crossfade.

## Runtime safety and fallback

The worker preallocates planar, algorithm, delay, crossfade, and resampling
buffers. Its output is fanned out once to all graph consumers. Runtime controls
are atomics and update algorithm, bypass, postfilter, and audition without
reopening devices.

The state machine distinguishes Starting, Syncing, Ready, Fallback, Bypassed,
and Error. Reasons distinguish synchronization, deliberate bypass, an unlocked
domain, no healthy channel, source error, processor error, and CPU overload.
During recovery the output crossfades to the best healthy delayed channel or a
stable Delay-and-Sum signal. MVDR has a finer per-bin fallback. No hidden
algorithm or device substitution is treated as success.

## Platform support boundary

- Windows: physical input through CPAL/WASAPI; system/app loopback remains a
  separate feature and is not accepted as an array member.
- macOS: physical input through CPAL/CoreAudio; ScreenCaptureKit remains a
  separate feature.
- Linux: one configured PipeWire capture stream per physical source; monitor
  nodes are rejected as array sources.

The common worker, calibration, synchronization, controls, and metrics are
platform-independent. Device discovery and stream construction remain in their
per-OS backends. Backend inability is returned as an error rather than replaced
with a different device, rate, or channel layout.

## Validation coverage

Pure tests cover arbitrary-N steering and Delay-and-Sum (including 2, 4, 8,
and 16 channels), two-channel GSC structure, GSC/MVDR finite output, singular
MVDR fallback, explicit frequency-domain latency, target/interferer behavior,
calibration outliers and disconnected graphs, topology fingerprints,
serialization, hot control changes, discontinuity recovery, synchronization
for multiple `K`, and acoustic TDOA preservation. The ignored release benchmark
reports worker cost for multiple channel counts rather than storing fabricated
performance numbers.

Template tests cover stable IDs, one array node, unconfigured defaults, 4-channel
shared-clock topology, mixed 4+2 topology, persistence, and missing devices.
Real hardware acceptance remains separate; use
[microphone-array-hardware-test.md](microphone-array-hardware-test.md).

## Research and implementation provenance

The Splitwave implementation was written independently in Rust against the
existing graph and real-time architecture. No source code, tests, configuration
files, or assets were copied or translated from the projects below. Papers and
project documentation were used to check terminology, numerical expectations,
failure cases, and system decomposition.

| Source                                                                                                              | License                                       | Studied                                                             | Ported                   | Independently implemented                             | NOTICE impact                             |
| ------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------- | ------------------------ | ----------------------------------------------------- | ----------------------------------------- |
| [GCC-PHAT, Knapp and Carter (1976)](https://doi.org/10.1109/TASSP.1976.1162830)                                     | Paper                                         | Generalized cross-correlation and phase transform                   | None                     | Pairwise delay estimator and quality gating           | Citation only                             |
| [GSC, Griffiths and Jim (1982)](https://doi.org/10.1109/TAP.1982.1142739)                                           | Paper                                         | Generalized sidelobe-canceller structure                            | None                     | Bounded N-channel adaptive path                       | Citation only                             |
| [MVDR/Capon (1969)](https://doi.org/10.1109/PROC.1969.7278)                                                         | Paper                                         | Minimum-variance constrained beamforming                            | None                     | Loaded complex solve and per-bin fallback             | Citation only                             |
| [pyroomacoustics](https://github.com/LCAV/pyroomacoustics) and [paper](https://doi.org/10.1109/ICASSP.2018.8461310) | MIT                                           | Room/array geometry and reference test scenarios                    | None                     | Geometry, steering, fixtures, and DSP are native Rust | No bundled component                      |
| [ODAS](https://github.com/introlab/odas) and [paper](https://doi.org/10.3389/frobt.2022.854444)                     | MIT                                           | N-channel mapping, localization/separation system boundaries        | None                     | Dynamic member model and telemetry                    | No bundled component                      |
| [paderwasn](https://github.com/fgnt/paderwasn)                                                                      | MIT                                           | Independent-clock SRO/STO concepts and simulation cases             | None                     | Occupancy-controlled per-domain ASRC and recovery     | No bundled component                      |
| [openMHA](https://github.com/HoerTech-gGmbH/openMHA)                                                                | AGPL-3.0                                      | Public descriptions of real-time beamforming and calibration        | None                     | All array DSP code                                    | No code copied; no license change         |
| [BeamformIt](https://github.com/xanguera/BeamformIt)                                                                | No repository license identified during audit | Public variable-channel feature description and papers              | None                     | All array DSP code                                    | No code copied; excluded as a code source |
| [rubato](https://github.com/HEnquist/rubato)                                                                        | MIT OR Apache-2.0                             | Existing Splitwave dependency API and real-time resampling guidance | Existing dependency only | Synchronizer/control around `SincFixedIn`             | Already listed in NOTICE                  |

In particular, no AGPL openMHA code and no unlicensed BeamformIt code was
copied, translated, or incorporated. Microphone Array adds no new third-party
runtime dependency. The existing `rubato`, `rtrb`, and `rustfft`-based engine
dependencies remain governed by the repository's `cargo-deny` policy.
