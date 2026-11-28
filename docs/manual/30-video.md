# Working with video

PhotoHop edits photos and videos with the same tools. A still is simply a one-frame clip.

## Importing video

Import a video like a photo. The first video into an empty project sets the canvas size, frame rate and length. Supported: MP4, MOV, MKV, WebM, AVI, M4V, including 10-bit and HDR phone video (HDR10/PQ, HLG, BT.2020), and image sequences.

## Playing and scrubbing

- **Space** plays and pauses. **←/→** step one frame. **Home** goes to the start.
- Click or drag in the timeline ruler to move the **playhead**.
- **View > Video preview quality**: *Half* (default) or *Quarter* for smooth playback on slower computers; *Full* for checking detail. Export always uses full quality.

## Cutting yourself out of a video

On a video layer, **1 Cut out** shows **Remove background (AI, whole video)**: AI follows you through every frame (BiRefNet + MatAnyone 2). It takes a while — about 1–2 minutes for a 17-second 4K clip on a good graphics card, much longer on a laptop.

For a few problem frames, add **Pen shapes** and key them (◆) at those frames.

## Tracking a region

**AI > Track a region…** — drag a box around something (a window, a sign) on the selected video layer. AI follows it through the clip and makes a moving mask. Another layer can then follow it: select that layer and use **Follow the tracked region of** in [Timing & animation](timing).

## Replacing a window or screen

See [Replace a window or screen](window).

## Animating settings

Every value with a ◆ button can change over time. See [Timing & animation](timing).

## Exporting video

**File > Export…**, *Timeline as video*. See [Saving & exporting](export#exporting-video).
