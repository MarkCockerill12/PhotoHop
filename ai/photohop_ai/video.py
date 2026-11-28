"""Video masks: MatAnyone 2 matting (person cut-out through a clip) and SAM 2.1 region tracking.
Both write a greyscale mask video (white = selected) with the same frame count and rate as the input."""

import shutil
import subprocess
import tempfile
from pathlib import Path

import numpy as np
import torch
from PIL import Image


def _ffmpeg() -> str:
    return shutil.which("ffmpeg") or "ffmpeg"


def _probe_fps(video: str) -> float:
    out = subprocess.run(
        [shutil.which("ffprobe") or "ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
         "stream=r_frame_rate", "-of", "csv=p=0", video], capture_output=True, text=True, check=True).stdout.strip()
    n, d = out.split("/") if "/" in out else (out, "1")
    return float(n) / float(d)


def extract_frames(video: str, folder: Path, max_side: int) -> int:
    """Decode to numbered JPEGs (scaled so the short side is at most `max_side`)."""
    folder.mkdir(parents=True, exist_ok=True)
    scale = f"scale='if(lt(iw,ih),min({max_side},iw),-2)':'if(lt(iw,ih),-2,min({max_side},ih))'"
    subprocess.run([_ffmpeg(), "-v", "error", "-y", "-i", video, "-vf", scale, "-q:v", "2", str(folder / "%05d.jpg")],
                   check=True)
    return len(list(folder.glob("*.jpg")))


def write_mask_video(frames: list[np.ndarray], out: str, fps: float) -> None:
    """Greyscale masks (H×W uint8) → high-quality H.264 (yuv444, near-lossless)."""
    h, w = frames[0].shape
    proc = subprocess.Popen(
        [_ffmpeg(), "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "gray", "-s", f"{w}x{h}", "-r", f"{fps}",
         "-i", "-", "-c:v", "libx264", "-crf", "4", "-pix_fmt", "yuv444p", out], stdin=subprocess.PIPE)
    assert proc.stdin is not None
    for f in frames:
        proc.stdin.write(np.ascontiguousarray(f, dtype=np.uint8).tobytes())
    proc.stdin.close()
    if proc.wait() != 0:
        raise RuntimeError("ffmpeg failed writing the mask video")


def track_box(video: str, out: str, box: tuple[float, float, float, float], frame: int = 0,
              device: str = "cuda", max_side: int = 1080, model: str = "facebook/sam2.1-hiera-large") -> int:
    """SAM 2.1 tracks the object inside `box` (pixels of the original video) through the clip."""
    from sam2.sam2_video_predictor import SAM2VideoPredictor

    fps = _probe_fps(video)
    with tempfile.TemporaryDirectory() as tmp:
        folder = Path(tmp)
        n = extract_frames(video, folder, max_side)
        with Image.open(folder / "00001.jpg") as im:  # close before renaming (Windows locks open files)
            fw, fh = im.size
        # Rename to 0-based names SAM 2 expects.
        for p in sorted(folder.glob("*.jpg")):
            p.rename(folder / f"{int(p.stem) - 1:05d}.jpg")
        src_w = int(subprocess.run([shutil.which("ffprobe") or "ffprobe", "-v", "error", "-select_streams", "v:0",
                                    "-show_entries", "stream=width", "-of", "csv=p=0", video],
                                   capture_output=True, text=True, check=True).stdout.strip() or fw)
        k = fw / src_w if src_w else 1.0
        b = np.array([v * k for v in box], np.float32)
        pred = SAM2VideoPredictor.from_pretrained(model, device=device)
        masks: list[np.ndarray] = [np.zeros((fh, fw), np.uint8)] * n
        ctx = torch.autocast("cuda", dtype=torch.bfloat16) if device == "cuda" else torch.autocast("cpu", enabled=False)
        with torch.inference_mode(), ctx:
            state = pred.init_state(video_path=str(folder))
            pred.add_new_points_or_box(state, frame_idx=frame, obj_id=1, box=b)
            for reverse in (False, True):
                for fi, _ids, logits in pred.propagate_in_video(state, reverse=reverse):
                    masks[fi] = ((logits[0, 0] > 0).float().cpu().numpy() * 255).astype(np.uint8)
        write_mask_video(masks, out, fps)
    return n


def matte_video(video: str, out: str, first_mask: str, device: str = "cuda", max_side: int = 720) -> int:
    """MatAnyone 2 propagates a first-frame subject mask through the whole clip (soft alpha)."""
    from matanyone2 import InferenceCore, MatAnyone2

    fps = _probe_fps(video)
    with tempfile.TemporaryDirectory() as tmp:
        folder = Path(tmp) / "frames"
        n = extract_frames(video, folder, max(max_side, 1080))
        proc = InferenceCore(MatAnyone2.from_pretrained("PeiqingYang/MatAnyone2"), device=device)
        res = Path(tmp) / "res"
        proc.process_video(input_path=str(folder), mask_path=first_mask, output_path=str(res), max_size=max_side)
        pha = next(res.rglob("*pha*.mp4"))
        subprocess.run([_ffmpeg(), "-v", "error", "-y", "-i", str(pha), "-r", f"{fps}", "-c:v", "libx264", "-crf", "4",
                        "-pix_fmt", "yuv444p", out], check=True)
    return n
