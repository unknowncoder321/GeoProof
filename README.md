# GeoProof

Click timing analysis for Geometry Dash completion recordings. GeoProof is a standalone
Windows app. It never touches the game. You give it a recording, it finds every click in
the audio, places each one on the exact video frame, and shows them on a video-editor
timeline you can scrub.

GeoProof reports measurements. Timing patterns alone are not proof of cheating or of
legitimacy, and the app says so.

## Install and run (Windows, one command)

Paste this in PowerShell. It downloads the latest build with ffmpeg included, puts a
GeoProof shortcut on your Desktop, and starts it. No Rust or compiler needed.

```
$d="$env:TEMP\gp-install"; if (Test-Path $d) { Remove-Item $d -Recurse -Force }; git clone -q https://github.com/unknowncoder321/GeoProof.git $d; powershell -ExecutionPolicy Bypass -File "$d\install.ps1"
```

It uses the GitHub login your git already has (the repo is private). Every push builds a
new version, so running the command again updates the app.

## What you see

- Top: the video frame at the playhead, exact to the frame.
- Bottom: the timeline. A ruler, a lane of click flags, and the audio waveform with a red
  line at every detected click. The red lines are only on the audio, not the video.
  Zoom goes from the whole recording down to single audio samples; frame boundaries
  appear once you zoom in far enough.
- Right: inspector for the selected click (time, frame, position inside the frame, gap
  to the previous click), a click list, recording stats, and detection settings.

Controls: scroll to zoom, Shift+scroll or right-drag to pan, drag or click to move the
playhead, click a red marker to select it, arrows step one frame (Shift: ten), Ctrl+arrows
jump to the previous or next click.

## Teaching it your click sound

Select a few clean clicks and press "Add to click sound". GeoProof learns the spectral
shape of your click (it is stable for one keyboard or mouse and mic) and drops detections
that do not match, such as hi-hats or drum hits. Rejected detections stay visible as dim
marks so nothing disappears silently. The match slider sets how strict it is.

Tested on synthetic audio through a real AAC encode, learning from 5 clicks:

| Click level vs music | Separation (AUC) | At match 0.95 |
| --- | --- | --- |
| +6 dB | 0.98 to 0.99 | kept 84 of 91 real clicks, 0 of 114 false |
| 0 dB | 0.57 to 0.87 | unreliable |
| -6 dB | 0.54 to 0.64 | unreliable |

It works when the click is clearly louder than the music, which is what Pointercrate and
AREDL ask for. When clicks are as quiet as the music, do not trust it.

Reproduce: `python3 proto/sigtest.py`.

## How click detection works

1. ffmpeg decodes the audio to mono 48 kHz float.
2. Two cascaded 1.5 kHz high-pass biquads.
3. Log-magnitude spectral flux (512-point FFT, 128-sample hop) over a chosen band,
   default 1.5 to 6 kHz.
4. Adaptive threshold: local median plus K robust standard deviations (MAD) over a 1 s
   window.
5. Peak picking with a 20 ms refractory gap.
6. Each peak is refined on the raw samples to find the onset to well under a millisecond.
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
  by several ms (95th percentile 4.2 ms through AAC).
- Nothing here has been run on a real recording yet.

Reproduce: `python3 proto/test.py` (needs numpy, scipy, ffmpeg).

## Known limits

- No playback. The video shows the exact frame at the playhead and steps frame by frame;
  each frame is decoded on demand, so scrubbing is not real time.
- Audio-only. A true per-click "frame delay" means the gap between the click and the frame
  where the game visibly reacts. That needs visual detection and is not built.
- Release (key up) events are not separated from presses.
- Variable-frame-rate recordings are flagged, and frame numbers are approximate there.
- Two clicks inside one frame are listed separately, but frame-based navigation treats
  them as the same frame.
- The whole audio track is held in memory (about 11 MB per minute).
- CI compiles it and runs the unit tests. The interface has not been looked at on a real
  screen by the author yet.

## Layout

```
install.ps1       one-command Windows installer
src/media.rs      ffprobe/ffmpeg: probe, decode audio, decode a single frame
src/detect.rs     click detection and click-sound signatures (port of proto/)
src/analysis.rs   time to frame, position in frame, gaps, click-sound filter
src/timeline.rs   waveform pyramid and the timeline widget
src/theme.rs      colours, type, spacing
src/app.rs        layout and interaction
proto/            Python reference detector and the synthetic test harness
docs/DETECTION.md research notes and open questions
```
