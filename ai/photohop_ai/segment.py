"""Text-prompted masks with SAM 3 (Meta SAM licence; gated on Hugging Face). Output: greyscale PNG."""

import numpy as np
import torch
from PIL import Image


def segment(src: str, dst: str, prompt: str, device: str = "cuda", threshold: float = 0.4) -> tuple[int, float]:
    from transformers import Sam3Model, Sam3Processor

    dtype = torch.bfloat16 if device == "cuda" else torch.float32
    proc = Sam3Processor.from_pretrained("facebook/sam3")
    model = Sam3Model.from_pretrained("facebook/sam3", torch_dtype=dtype).to(device).eval()
    img = Image.open(src).convert("RGB")
    inputs = proc(images=img, text=prompt, return_tensors="pt").to(device)
    inputs = {k: (v.to(dtype) if v.is_floating_point() else v) for k, v in inputs.items()}
    with torch.no_grad():
        out = model(**inputs)
    res = proc.post_process_instance_segmentation(
        out, threshold=threshold, mask_threshold=0.5, target_sizes=[img.size[::-1]]
    )[0]
    masks = res["masks"]
    if len(masks) == 0:
        m = np.zeros((img.height, img.width), np.float32)
    else:
        m = masks.float().max(dim=0).values.cpu().numpy()
    Image.fromarray((m * 255).astype(np.uint8)).save(dst)
    return len(masks), round(float(m.mean()), 4)
