# Timing & animation

The **Timing & animation** section and the timeline control when a layer appears and how its settings change over time.

## Timing

- **Starts at** — the time on the timeline where the layer begins.
- **Skip first** — start a video this many seconds in (trim the beginning).

The canvas **Duration** (inspector with nothing selected) sets how long the whole project is.

## Keyframes

A **keyframe** stores a value at a moment in time; between keyframes PhotoHop moves smoothly from one value to the next.

- The **◆** next to a value sets or removes a keyframe at the playhead. Bright yellow = keyframe here; dim gold = animated, keyframe elsewhere; grey = not animated.
- **K** toggles keyframes for position, size, rotation and opacity of the selected layer at once. The ◆ button in the timeline does the same.
- **Animate: every change adds a keyframe** (auto-key) — when on, every change you make is keyed at the playhead. When off, a value that isn't animated yet changes for the whole clip; a value that already has keyframes gets a keyframe at the playhead.

## Follow the tracked region of…

If another layer has an AI-tracked mask (from **Track a region** or a video cut-out), a button for it appears here. Click it to keyframe the selected layer so it moves with that tracked region — for example a screen replacement following a phone.

## The timeline

Each layer is a bar. Diamonds on a bar are its keyframes. Drag in the ruler to scrub.
