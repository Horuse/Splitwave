# Microphone Array hardware test

Record the Splitwave commit, operating-system version, device names and driver
versions, sample rate, buffer size, geometry, target, selected algorithm, and
whether device enhancements were disabled. Do not describe a platform or device
pair as tested without completing the corresponding procedure on real hardware.

For each run, save the pipeline, close and reopen it, and confirm the same
sources, channel indices, geometry, target, algorithm, and calibration status
return. Connect the mono output to a Recorder only when a test recording is
intended.

## Four-channel shared-clock interface

1. Connect a wired interface exposing at least four simultaneous input channels.
2. Add one Microphone Array and use **Add all channels** for channels 1–4.
3. Confirm `N=4`, `K=1`, and one source in Setup.
4. Enter measured microphone positions and a fixed target.
5. Calibrate from the target; record the quality score and residual delay.
6. Activate the pipeline and verify Ready state with no growing underflow or
   overflow counters.
7. Compare Best single, Raw calibrated, Delay-and-Sum, and Spatial in A/B
   monitoring. Confirm every transition is click-free.
8. Test Delay-and-Sum, GSC, MVDR, postfilter on/off, and Bypass. Record active
   algorithm, worker p50/p95, deadline misses, and MVDR fallback bins.
9. Speak from the target and an off-target position. Record target loss and
   off-target attenuation from the captured files; do not infer SNR improvement
   from the quality score.
10. Disconnect and reconnect the interface, then verify safe fallback,
    recovery, resynchronization, and no permanent silence.
11. Put the machine to sleep and wake it; repeat the recovery checks.
12. Save, close, and reopen the pipeline and verify persistence.

## Two independent wired USB microphones

1. Connect two wired USB microphones and disable AGC, NS, echo cancellation,
   and enhancements where available. Do not use Bluetooth.
2. Add both devices to one array and confirm `N=2`, `K=2`.
3. Select the more stable device as master, enter geometry/target, and calibrate.
4. Run for at least 30 minutes. At 1, 5, 15, and 30 minutes record both domains'
   ppm, ratio, ring fill, corrections, lock state, underflows, and overflows.
5. Confirm there is no steadily increasing delay or audible drift.
6. Disconnect the slave. Confirm a crossfaded fallback and an explicit reason.
7. Reconnect it and confirm resynchronization and return to Ready without
   rebuilding the graph.
8. Repeat sleep/wake and pipeline reopen checks.

## Mixed topology: four-channel interface plus two USB microphones

1. Add all four channels from the interface and one channel from each USB mic.
2. Confirm `N=6`, `K=3`: one stream/domain for the interface and one for each
   USB device.
3. Make the interface the master, then enter geometry and calibrate.
4. Confirm the four interface channels retain stable relative phase while only
   the two slave domains receive changing ASRC ratios.
5. Run the 30-minute drift, disconnect/reconnect, sleep/wake, algorithm, A/B,
   persistence, and measurement checks from the preceding sections.

## Negative geometry test

1. Place an interferer so its steering delay/TDOA is close to the configured
   target even though it occupies another possible position.
2. Run Delay-and-Sum, GSC, and MVDR.
3. Confirm the target is not destructively self-cancelled.
4. Confirm diagnostics and the UI do not claim high certainty when separation
   is ambiguous.
5. Record the audible/result limitation as same-TDOA ambiguity rather than a
   device failure.

## Acceptance record

For each topology report measured values, not projections:

| Metric                         | Result |
| ------------------------------ | ------ |
| `N`, `K`                       |        |
| Sample rate / block / FFT      |        |
| Algorithmic latency            |        |
| Synchronization-buffer latency |        |
| Worker p50 / p95               |        |
| Deadline misses                |        |
| SNR improvement                |        |
| Target loss                    |        |
| Residual delay                 |        |
| 30-minute drift                |        |
| Sleep/wake                     |        |
| Disconnect/reconnect           |        |
| Subjective artifacts           |        |

If hardware or a platform was unavailable, mark it **Not tested** and state why.
