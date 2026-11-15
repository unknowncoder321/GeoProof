# GeoProof

Click timing analysis for Geometry Dash completion recordings. GeoProof is a standalone
Windows app. It never touches the game. You give it a recording, it finds the clicks in
the audio, places each one on the video's frame timeline, and shows you the timing.

GeoProof reports measurements. Timing patterns alone are not proof of cheating or of
legitimacy, and the app says so.

## Status

Version 0.1 is the measurement layer only: load a video, detect clicks from audio, show
each click's frame, position inside the frame, and the gap to the previous click.
There is no anomaly score yet. That needs a calibration set (see docs/DETECTION.md).

| Part | State |
| --- | --- |
| Detection algorithm (proto/*.py) | Tested on synthetic audio, including a real AAC encode through ffmpeg |
| Rust port (src/*.rs) | Written to mirror the Python reference. Not compiled by the author yet; the first CI run is the first compile |
| Real recordings | Not tested. No real footage has been run through it |

## Run

You need ffmpeg and ffprobe. Either put `ffmpeg.exe` and `ffprobe.exe` next to
`geoproof.exe`, or have them on PATH. CI builds a bundle with both.

```
cargo run --release
```

Open a recording or drop it on the window. Pick a sensitivity, narrow the frequency band
to where your click sound lives, and press "Detect again". "Save report" writes
`<video>.geoproof.json` next to the video.

## How click detection works

1. ffmpeg decodes the audio to mono 48 kHz float.
2. Two cascaded 1.5 kHz high-pass biquads.
3. Log-magnitude spectral flux (512-point FFT, 128-sample hop) over a chosen band,
   default 1.5 to 6 kHz.
4. Adaptive threshold: local median plus K robust standard deviations (MAD) over a 1 s
   window.
5. Peak picking with a 20 ms refractory gap.
6. Each peak is refined on the raw high-passed samples to find the onset to well under
   a millisecond.
7. Time is shifted by the audio/video stream start offset, then split into a frame number
   and a 0 to 1 position inside that frame.

## Validation so far (synthetic only)

40 s of synthetic music with loud hi-hats, kicks and a saw lead, plus 96 synthetic clicks,
encoded to AAC in an MP4 and decoded back through ffmpeg. Defaults: band 1.5 to 6 kHz, K=10.

| Click level vs music | Recall | Precision | Median timing error |
| --- | --- | --- | --- |
| +6 dB | 0.97 | 0.89 | -0.06 ms |
| 0 dB | 0.59 | 0.72 | -0.08 ms |
| -6 dB | 0.23 | 0.58 | -0.07 ms |

Caveats that matter:

- The synthetic audio is my own construction and the defaults were tuned on it. Real
  accuracy is unknown until it is run on real recordings.
- The median error is excellent, but the tails are not: at +6 dB a few matches were off
  by several ms (95th percentile 4.2 ms through AAC). At 0 dB and below, some matches are
  wrong events.
- Quiet clicks under loud music are not reliably found. A recording with the click audio
  on its own track (as Pointercrate and AREDL ask for) avoids this problem.

Reproduce: `python3 proto/test.py` (needs numpy, scipy, ffmpeg).

## Known limits

- Audio-only. A true per-click "frame delay" means the gap between the click and the frame
  where the game visibly reacts. That needs visual detection and is not built.
- Release (key up) events are not separated from presses.
- Variable-frame-rate recordings are flagged, and frame numbers are approximate there.
- The whole audio track is held in memory (about 11 MB per minute).

## Layout

```
src/media.rs      ffprobe/ffmpeg: probe the file, decode audio
src/detect.rs     click onset detection (port of proto/detect.py)
src/analysis.rs   time to frame, position in frame, gaps
src/app.rs        egui interface
proto/            Python reference detector and the synthetic test harness
docs/DETECTION.md research notes and open questions
```
