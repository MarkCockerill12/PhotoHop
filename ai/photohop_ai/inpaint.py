"""Scoped object removal (the one place PhotoHop lets AI create pixels): SAM 3 finds the object from
a text prompt, FLUX.2 klein 4B (Apache-2.0) repaints a crop around it, and the output is an RGBA
patch whose alpha is the (feathered) object mask — nothing outside the mask can change."""

import numpy as np
import torch
from PIL import Image, ImageFilter


def object_mask(img: Image.Image, prompt: str, device: str) -> np.ndarray:
    from transformers import Sam3Model, Sam3Processor

    dtype = torch.bfloat16 if device == "cuda" else torch.float32
    proc = Sam3Processor.from_pretrained("facebook/sam3")
    model = Sam3Model.from_pretrained("facebook/sam3", torch_dtype=dtype).to(device).eval()
    inputs = proc(images=img, text=prompt, return_tensors="pt").to(device)
    inputs = {k: (v.to(dtype) if v.is_floating_point() else v) for k, v in inputs.items()}
    with torch.no_grad():
        out = model(**inputs)
    res = proc.post_process_instance_segmentation(out, threshold=0.4, mask_threshold=0.5, target_sizes=[img.size[::-1]])[0]
    if len(res["masks"]) == 0:
        raise RuntimeError(f'nothing matching "{prompt}" was found')
    m = res["masks"].float().max(dim=0).values.cpu().numpy()
    del model
    if device == "cuda":
        torch.cuda.empty_cache()
    return m


def remove(src: str, dst: str, prompt: str, fill_prompt: str, device: str = "cuda", grow: int = 12) -> float:
    from diffusers import Flux2KleinInpaintPipeline

    img = Image.open(src).convert("RGB")
    mask = Image.fromarray((object_mask(img, prompt, device) * 255).astype(np.uint8))
    mask = mask.filter(ImageFilter.MaxFilter(2 * grow + 1))
    box = mask.getbbox()
    if box is None:
        raise RuntimeError("empty mask")
    # Crop around the object (with context) so the model works at full detail.
    x0, y0, x1, y1 = box
    pad = int(max(x1 - x0, y1 - y0) * 0.35) + 32
    cx0, cy0 = max(0, x0 - pad), max(0, y0 - pad)
    cx1, cy1 = min(img.width, x1 + pad), min(img.height, y1 + pad)
    crop, crop_mask = img.crop((cx0, cy0, cx1, cy1)), mask.crop((cx0, cy0, cx1, cy1))
    scale = min(1.0, 1024 / max(crop.size))
    size = (max(16, int(crop.width * scale) // 16 * 16), max(16, int(crop.height * scale) // 16 * 16))
    pipe = Flux2KleinInpaintPipeline.from_pretrained("black-forest-labs/FLUX.2-klein-4B", torch_dtype=torch.bfloat16)
    if device == "cuda":
        pipe.enable_model_cpu_offload()
    else:
        pipe.to(device)
    out = pipe(prompt=fill_prompt, image=crop.resize(size, Image.LANCZOS), mask_image=crop_mask.resize(size, Image.BILINEAR),
               height=size[1], width=size[0], num_inference_steps=4, generator=torch.Generator("cpu").manual_seed(7)).images[0]
    out = out.resize(crop.size, Image.LANCZOS)
    # Patch = repainted pixels, alpha = feathered mask (only the object area).
    soft = crop_mask.filter(ImageFilter.GaussianBlur(max(2, grow // 3)))
    patch = Image.new("RGBA", img.size, (0, 0, 0, 0))
    rgba = out.convert("RGBA")
    rgba.putalpha(soft)
    patch.paste(rgba, (cx0, cy0))
    patch.save(dst)
    return round(float(np.asarray(mask).mean() / 255.0), 4)
