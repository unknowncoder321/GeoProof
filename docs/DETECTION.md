# Detection notes and open questions

## What the research settled

- Pointercrate and AREDL both require raw footage (30 fps or higher) with the clicks
  audible, ideally on their own track. 30 fps is a normal case, not a fallback.
- Existing tools (Frame Inspector, AutoFPCount, the Dynamic mod) need the macro file or
  hardware input hooks. None analyzes an arbitrary submitted video, so this niche is open.
- Audio from a screen recording keeps sub-frame timing: the click's position is known to
  about a millisecond, far finer than a 16.7 ms frame. The weak links are the capture
  pipeline's audio/video offset and jitter, not the audio sample rate.
- Key releases are often quiet or inaudible, so press and release cannot be relied on
  separately from audio alone.

## What the research did not settle

- There is no published data on what a legitimate top player's click-gap distribution looks
  like, and none on exact macro delay values.
- SpaceUK's exact frame-delay sequence is not documented anywhere found. Public reports say
  he was identified through ZBot watermark patterns and unnaturally uniform click spacing,
  not through a published frame-delay analysis. The "1, 3, -1 frame" example in the project
  brief is unconfirmed, so no detection logic is built around it.

## Two different signatures

1. Macro uniformity: fixed-tick macros (commonly 240 TPS) tend to give very regular,
   frame-aligned timing. Detectable as gaps that are too regular.
2. Overlay desync: a live click recorded over a macro could show irregular offsets between
   the audible click and the on-screen reaction. Theoretical, unverified, low confidence.
   Needs visual detection of the game's response.

## What is needed before any score

A calibration set: several known-legitimate extreme demon recordings and several macro
recordings made on purpose with zBot/MHv7 on a test level. Without it, thresholds would be
invented numbers.

## Next steps, in order

1. Run the detector on real recordings, with and without a separate click track.
2. Measure how often it misses or invents clicks on real footage and fix that first.
3. Build the calibration set and the uniformity statistics on top of the click gaps.
4. Decide on visual reaction detection for true frame delay.
