"""Automatic subject matte with BiRefNet (MIT). Output: greyscale PNG, white = subject."""

import numpy as np
import torch
from PIL import Image


def matte(src: str, dst: str, model: str = "ZhengPeng7/BiRefNet_HR", device: str = "cuda", res: int = 2048) -> float:
    import torchvision.transforms as T
    from transformers import AutoModelForImageSegmentation

    dtype = torch.float16 if device == "cuda" else torch.float32
    net = AutoModelForImageSegmentation.from_pretrained(model, trust_remote_code=True).to(device).eval().to(dtype)
    img = Image.open(src).convert("RGB")
    tf = T.Compose([T.Resize((res, res)), T.ToTensor(), T.Normalize([0.485, 0.456, 0.406], [0.229, 0.224, 0.225])])
    with torch.no_grad():
        pred = net(tf(img).unsqueeze(0).to(device).to(dtype))[-1].sigmoid().float().cpu()[0, 0].numpy()
    alpha = Image.fromarray((np.clip(pred, 0, 1) * 255).astype(np.uint8)).resize(img.size, Image.BILINEAR)
    alpha.save(dst)
    return round(float(np.asarray(alpha).mean() / 255.0), 4)
