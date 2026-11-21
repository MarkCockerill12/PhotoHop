# photohop-ai (optional)

AI helpers that PhotoHop calls for masks and (later) colour parameters. They never repaint your
pixels — the only exception is the opt-in prop-overhang inpainting.

```sh
cd ai
uv venv && uv pip install -e .            # CPU works; for NVIDIA install torch from the CUDA index first:
uv pip install torch torchvision --index-url https://download.pytorch.org/whl/cu128
photohop-ai info                          # shows the device PhotoHop will use
```

Then point PhotoHop at the environment's Python (AI ▸ Settings, or `PHOTOHOP_AI_PYTHON`).

| Command | Model (licence) | Notes |
|---|---|---|
| `matte IN OUT` | BiRefNet_HR (MIT) | automatic cutout of the main subject |
| `normals IN OUT` | Sapiens2 normal 1B (Sapiens2 licence) | normal map of people, used by Relight |
| `video-matte IN.mp4 OUT.mp4` | BiRefNet + MatAnyone 2 (S-Lab, non-commercial) | cut a person out of a whole clip |
| `track IN.mp4 OUT.mp4 --box x0,y0,x1,y1` | SAM 2.1 large (Apache-2.0) | follow a region (e.g. a window) through a clip |
| `remove IN OUT --prompt "toy gun"` | SAM 3 + FLUX.2 klein 4B (Apache-2.0) | repaint an object away; output is an RGBA patch limited to the object (needs `diffusers`) |
| `harmonize COMP MASK OUT` | Harmonizer (CC BY-NC-SA, downloaded on first use, ~20 MB) | AI auto-colour: predicts temperature / brightness / contrast / saturation / highlight / shadow; PhotoHop fits its own curves to the change |
| `camera-solve IN.mp4 camera.json` | Depth Anything 3 (DA3 Nested Giant: CC BY-NC; DA3 Base/Metric: Apache-2.0) | per-frame 3D camera (metric), lens and ground plane for the Blender bridge |
| `body-motion IN.mp4 body.json --mask M.mp4` | SAM 3D Body (SAM License; code + weights downloaded on first use) | per-frame 3D body mesh (Blender Mesh Cache .pc2) and hand tracks for props |
| `depth IN OUT.png` | Depth Anything 3 | depth map for depth of field (white = near) |
| `find-glass IN OUT.json --prompt "window pane"` | SAM 3 + OpenCV | glass panes as polygons for Replace window / screen |
| `segment IN OUT --prompt "person"` | SAM 3 (Meta SAM licence, gated: accept on Hugging Face and run `hf auth login`) | any text prompt, e.g. "toy gun" |

Video commands need two extra packages (install without dependencies to avoid build tools):
`uv pip install --no-deps git+https://github.com/facebookresearch/sam2.git git+https://github.com/pq-yang/MatAnyone2.git`
plus FFmpeg on `PATH`. Run them on a GPU — a 17 s 4K phone clip takes ~1–2 minutes on an RTX 4070.

Models download automatically on first use into the Hugging Face cache (`HF_HOME` to move it).
