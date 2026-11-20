"""Surface normals of people with Sapiens2 (Meta, Sapiens2 licence). Output: RGB PNG encoding
camera-space normals as n * 0.5 + 0.5 with x right, y up, z towards the viewer (verified on
PhotoHop's test photo). Used only for relighting — it never changes pixels by itself."""

import numpy as np
import torch
from PIL import Image


def normals(src: str, dst: str, device: str = "cuda", size: str = "1b") -> float:
    import transformers as t

    repo = f"facebook/sapiens2-normal-{size}"
    dtype = torch.bfloat16 if device == "cuda" else torch.float32
    proc = t.Sapiens2ImageProcessor.from_pretrained(repo)
    model = t.Sapiens2ForNormalEstimation.from_pretrained(repo, torch_dtype=dtype).to(device).eval()
    img = Image.open(src).convert("RGB")
    inputs = proc(images=img, return_tensors="pt").to(device)
    inputs = {k: (v.to(dtype) if v.is_floating_point() else v) for k, v in inputs.items()}
    with torch.no_grad():
        out = model(**inputs)
    n = torch.nn.functional.interpolate(out.normals.float(), size=img.size[::-1], mode="bilinear", align_corners=False)
    n = torch.nn.functional.normalize(n, dim=1)[0].permute(1, 2, 0).cpu().numpy()
    Image.fromarray((np.clip(n * 0.5 + 0.5, 0, 1) * 255).astype(np.uint8)).save(dst)
    return round(float(n[..., 2].mean()), 3)
