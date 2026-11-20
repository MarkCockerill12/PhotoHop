"""AI auto-colour with Harmonizer (Ke et al., ECCV 2022 — "white-box" harmonization: the network only
predicts six filter amounts: temperature, brightness, contrast, saturation, highlight, shadow).
Harmonizer's code and weights are CC BY-NC-SA 4.0, so they are downloaded on first use into a local
cache rather than shipped with PhotoHop. PhotoHop then fits its own editable curves to the change."""

import sys

import numpy as np
import torch
from PIL import Image

from .fetch import ensure_repo

REPO_ZIP = "https://github.com/ZHKKKe/Harmonizer/archive/HEAD.zip"


def _ensure_repo():
    return ensure_repo("harmonizer", REPO_ZIP, "pretrained/harmonizer.pth")


def harmonize(comp_path: str, mask_path: str, out_path: str, device: str = "cpu", max_side: int = 1024) -> list[float]:
    repo = _ensure_repo()
    if str(repo) not in sys.path:
        sys.path.insert(0, str(repo))
    from src import model as hmodel  # Harmonizer's own package

    comp = Image.open(comp_path).convert("RGB")
    mask = Image.open(mask_path).convert("L")
    scale = min(1.0, max_side / max(comp.size))
    size = (max(16, int(comp.width * scale)), max(16, int(comp.height * scale)))
    comp, mask = comp.resize(size, Image.LANCZOS), mask.resize(size, Image.BILINEAR)

    net = hmodel.Harmonizer().to(device).eval()
    state = torch.load(repo / "pretrained" / "harmonizer.pth", map_location=device)
    net.load_state_dict(state, strict=True)
    to_t = lambda im: torch.from_numpy(np.asarray(im, np.float32) / 255.0)
    c = to_t(comp).permute(2, 0, 1)[None].to(device)
    m = to_t(mask)[None, None].to(device)
    with torch.no_grad():
        args = net.predict_arguments(c, m)
        out = net.restore_image(c, m, args)[-1]
    img = (out[0].permute(1, 2, 0).clamp(0, 1).cpu().numpy() * 255 + 0.5).astype(np.uint8)
    Image.fromarray(img).save(out_path)
    return [round(float(a.flatten()[0]), 4) for a in args]
