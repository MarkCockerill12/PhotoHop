"""Single-image depth for depth of field (Depth Anything 3). Output: 16-bit greyscale PNG of
normalised inverse depth (white = near, black = far), at the input's size."""

import numpy as np
from PIL import Image

MODEL_ID = "depth-anything/DA3NESTED-GIANT-LARGE-1.1"


def depth(src: str, dst: str, device: str = "cuda", model_id: str = MODEL_ID) -> dict:
    from depth_anything_3.api import DepthAnything3

    img = Image.open(src).convert("RGB")
    model = DepthAnything3.from_pretrained(model_id).to(device).eval()
    pred = model.inference([src])
    z = np.asarray(pred.depth, np.float64)[0]
    disp = 1.0 / np.maximum(z, 1e-6)
    lo, hi = np.percentile(disp, 1), np.percentile(disp, 99)
    d = np.clip((disp - lo) / max(hi - lo, 1e-9), 0.0, 1.0)
    out = Image.fromarray((d * 65535).astype(np.uint16)).resize(img.size, Image.BILINEAR)
    out.save(dst)
    return {"near_m": round(float(z.min()), 2), "far_m": round(float(np.percentile(z, 99)), 2)}
