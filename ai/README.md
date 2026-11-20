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
| `segment IN OUT --prompt "person"` | SAM 3 (Meta SAM licence, gated: accept on Hugging Face and run `hf auth login`) | any text prompt, e.g. "toy gun" |

Models download automatically on first use into the Hugging Face cache (`HF_HOME` to move it).
