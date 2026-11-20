# PhotoHop

Put yourself into any scene — a game map, a wallpaper video, your own Blender render, a real photo —
and make it look like you were actually there. PhotoHop is a focused photo **and** video compositor:
cut yourself out, place yourself in the new background, then match lighting, colour, shadows, grain
and lens look so the result reads as one image.

It is deliberately small: only the parts of a GIMP + DaVinci Resolve workflow this job needs.

> **Status: early development.** The editing shell, colour grading, masks and non-AI colour matching
> work today; selections, scopes, relighting, tracking and the Blender round-trip are next (see the roadmap).

## What works now

- Photos (PNG, JPEG, WebP, TIFF, EXR) and video (MP4, MOV, MKV, WebM…) as layers on one timeline
- GPU compositing in scene-linear float (wgpu): correct blending, no 8-bit banding
- 19 blend modes with GIMP-compatible maths (Normal, Multiply, Screen, Overlay, Soft/Hard light,
  Add, Subtract, Difference, Dodge, Burn, Grain extract/merge, Luminance, …)
- Move / scale / rotate with on-canvas handles, opacity, per-layer timing (start, source in point)
- Keyframes on position, scale, rotation, opacity (and colour layers), with auto-key mode
- Crop, paint layers with a soft brush and eraser (pressure-free for now), colour layers
- Per-layer colour grade: exposure, temperature/tint, lift/gamma/gain colour wheels, contrast/pivot,
  saturation — all keyframable
- **Match colour** to another layer without AI (colour cast only, brightness & contrast only, or full
  statistical transfer), with an amount slider
- Layer masks: paint to hide (brush) or reveal (eraser), invert, toggle
- Scopes (S): waveform, RGB parade, vectorscope with skin-tone line, histogram
- Undo/redo for everything, including brush strokes
- Video preview with background decoding, hardware decode and half/quarter-resolution proxies
- Export the current frame (PNG 8/16-bit, JPEG, linear EXR) or the timeline as H.264, H.265 or
  ProRes 4444 with alpha, keeping the original audio
- Project files (`.phop`) that store media paths relative to the project

## Install & run

PhotoHop runs on **Linux and Windows** (any GPU with Vulkan, DirectX 12 or Metal).

1. Install [Rust](https://rustup.rs) (stable).
2. Install **FFmpeg** and make sure `ffmpeg` and `ffprobe` are on your `PATH`
   (Fedora: `sudo dnf install ffmpeg`; Windows: `winget install Gyan.FFmpeg`).
   You can also point PhotoHop at specific binaries with `PHOTOHOP_FFMPEG` / `PHOTOHOP_FFPROBE`.
3. Build and run:

```sh
cargo run --release -p photohop               # empty project
cargo run --release -p photohop -- photo.jpg  # open a photo, video or .phop project
```

## Shortcuts

| Key | Action | Key | Action |
|---|---|---|---|
| V | Move tool | Space | Play / pause |
| C | Crop tool (Enter applies, Esc cancels) | ← / → | Previous / next frame |
| B / E | Brush / eraser | Home | Go to start |
| H | Hand (pan) — or middle-drag / Space-drag | K | Toggle keyframes on the selected layer |
| S | Show / hide scopes | | |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo | Ctrl+0 | Fit canvas to window |
| Ctrl+O / Ctrl+S | Open / save project | Ctrl+I | Import media |
| Ctrl+D | Duplicate layer | Del | Delete layer |

Scroll to zoom (towards the cursor). In the Move tool, hold Shift for free scaling or 15° rotation steps.

## Roadmap

1. **Editing shell** — layers, timeline, keyframes, transforms, crop, paint, export *(done)*
2. **Masks & cutouts** — mask painting *(done)*; selections, bezier roto, AI cutout suggestions you can refine
3. **Colour** — wheels, grade and non-AI matching *(done)*; curves, HSL qualifier, scopes, colour management
4. **Integration** — relighting (shading only, never repainting you), cast/contact shadows, light wrap,
   grain / blur / lens matching, haze and bloom
5. **Tracking** — point and planar trackers, stabiliser, 3D camera solve
6. **Blender round-trip** — export camera, cutout and body motion to Blender, bring renders back
7. **AI auto-colour** and an optional "game-ify" look

## AI models (planned, optional)

AI in PhotoHop is optional and limited by design: models may suggest **masks** and **colour / shading
parameters**, which PhotoHop then applies with its own deterministic maths — they never repaint your
pixels. The one exception is an opt-in inpainting step limited to the area a replaced prop no longer covers.

Models are downloaded separately, never bundled, and each keeps its own licence. Any model that
fits a task can be plugged in; the defaults below were benchmarked on an RTX 4070 (12 GB):

| Task | Default | Lighter / permissive alternatives |
|---|---|---|
| Photo cutout | BiRefNet_HR (MIT) | BiRefNet-lite, BEN2 |
| Video matting | MatAnyone 2 (S-Lab, non-commercial) | SAM 2.1 masks + refinement |
| Object / region tracking | SAM 2.1 large (Apache-2.0) | SAM 2.1 small/tiny |
| Human normals (relighting) | Sapiens2 1B | MoGe-2 normals |
| Scene depth | Depth Anything 3 Metric-Large (Apache-2.0) | MoGe-2, DA3 Giant (non-commercial) |

Model setup instructions will be added with the features that use them.

## Licence

GPL-3.0-or-later — see [LICENSE](LICENSE).
