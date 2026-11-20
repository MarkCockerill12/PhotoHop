"""Text-prompted masks with SAM 3 (Meta SAM licence; gated on Hugging Face). Output: greyscale PNG."""

import numpy as np
import torch
from PIL import Image


def instance_masks(src: str, prompt: str, device: str = "cuda", threshold: float = 0.4):
    """SAM 3 instances of `prompt` in the image: (image, list of HxW bool arrays, scores)."""
    from transformers import Sam3Model, Sam3Processor

    dtype = torch.bfloat16 if device == "cuda" else torch.float32
    proc = Sam3Processor.from_pretrained("facebook/sam3")
    model = Sam3Model.from_pretrained("facebook/sam3", torch_dtype=dtype).to(device).eval()
    img = Image.open(src).convert("RGB")
    inputs = proc(images=img, text=prompt, return_tensors="pt").to(device)
    inputs = {k: (v.to(dtype) if v.is_floating_point() else v) for k, v in inputs.items()}
    with torch.no_grad():
        out = model(**inputs)
    res = proc.post_process_instance_segmentation(out, threshold=threshold, mask_threshold=0.5, target_sizes=[img.size[::-1]])[0]
    masks = [m.cpu().numpy() > 0.5 for m in res["masks"]]
    scores = [float(s) for s in res.get("scores", [1.0] * len(masks))]
    return img, masks, scores


def find_glass(src: str, dst: str, prompt: str = "window pane", device: str = "cuda", threshold: float = 0.35) -> dict:
    """Window panes / screens as simple polygons (pixel coordinates) for PhotoHop's glass shapes."""
    import json
    from pathlib import Path

    import cv2

    img, masks, scores = instance_masks(src, prompt, device, threshold)
    min_area = 0.0015 * img.width * img.height
    panes = []
    for m, sc in zip(masks, scores):
        n, labels, stats, _ = cv2.connectedComponentsWithStats(m.astype(np.uint8), connectivity=8)
        for k in range(1, n):
            if stats[k, cv2.CC_STAT_AREA] < min_area:
                continue
            comp = (labels == k).astype(np.uint8)
            contours, _ = cv2.findContours(comp, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
            c = max(contours, key=cv2.contourArea)
            eps = 0.012 * cv2.arcLength(c, True)
            poly = cv2.approxPolyDP(c, eps, True).reshape(-1, 2)
            while len(poly) > 16:
                eps *= 1.4
                poly = cv2.approxPolyDP(c, eps, True).reshape(-1, 2)
            panes.append({"points": poly.astype(float).tolist(), "area": int(stats[k, cv2.CC_STAT_AREA]), "score": round(sc, 3)})
    # Drop panes inside a bigger pane (SAM sometimes returns the whole window and its panes).
    def inside(a, b):
        ca = np.mean(np.array(a["points"]), axis=0)
        return cv2.pointPolygonTest(np.array(b["points"], np.float32), (float(ca[0]), float(ca[1])), False) >= 0
    panes.sort(key=lambda p: -p["area"])
    keep = []
    for p in panes:
        if any(inside(p, q) for q in keep) and len(panes) > 1:
            # Prefer the smaller panes when the big one contains several of them.
            continue
        keep.append(p)
    contained = [p for p in panes if p not in keep]
    if len(contained) >= 2:
        keep = [p for p in panes if not any(inside(q, p) and q is not p for q in panes)]
    keep.sort(key=lambda p: (round(np.mean(np.array(p["points"])[:, 1]) / 50), np.mean(np.array(p["points"])[:, 0])))
    Path(dst).write_text(json.dumps({"width": img.width, "height": img.height, "panes": [p["points"] for p in keep]}))
    return {"panes": len(keep), "instances": len(masks)}


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
