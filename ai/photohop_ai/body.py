"""Body motion capture from a video with SAM 3D Body (Meta, SAM License): an 18k-vertex body mesh
per frame in camera space. Output for the Blender bridge:
  body.json  frames, fps, size, intrinsics, hand tracks (wrist position + orientation per frame)
  body.obj   the frame-0 mesh (OpenCV camera space: x right, y down, z forward, metres)
  body.pc2   every frame's vertices (Blender's Mesh Cache modifier reads PC2 natively)
Subject boxes come from PhotoHop's video cutout (mask video), so no person detector is needed."""

import json
import struct
import sys
import tempfile
from pathlib import Path

import numpy as np

from .fetch import ensure_repo
from .video import _probe_fps, extract_frames

REPO_ZIP = "https://github.com/facebookresearch/sam-3d-body/archive/HEAD.zip"
MODEL_ID = "facebook/sam-3d-body-dinov3"
# MHR-70 keypoints: wrist, middle / index / pinky knuckles.
HANDS = {"right": (41, 32, 28, 40), "left": (62, 53, 49, 61)}


def _hand_frame(k: np.ndarray, ids) -> list:
    """4x4 (row-major, camera space): X along the fingers, Y index→pinky side, Z out of the palm."""
    w, mid, idx, pinky = (k[i] for i in ids)
    fwd = mid - w
    fwd /= np.linalg.norm(fwd) + 1e-9
    side = idx - pinky
    up = np.cross(fwd, side)
    up /= np.linalg.norm(up) + 1e-9
    side = np.cross(up, fwd)
    m = np.eye(4)
    m[:3, 0], m[:3, 1], m[:3, 2], m[:3, 3] = fwd, side, up, w
    return m.round(5).tolist()


def _smooth(x: np.ndarray, sigma: float) -> np.ndarray:
    """Gaussian smoothing along axis 0 (time), edges clamped."""
    if sigma <= 0 or len(x) < 3:
        return x
    r = int(np.ceil(sigma * 3))
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2)
    k /= k.sum()
    pad = np.concatenate([np.repeat(x[:1], r, 0), x, np.repeat(x[-1:], r, 0)])
    out = np.zeros_like(x)
    for i, w in enumerate(k):
        out += w * pad[i : i + len(x)]
    return out


def _write_pc2(path: Path, frames: np.ndarray, fps: float):
    n, v, _ = frames.shape
    with open(path, "wb") as f:
        f.write(b"POINTCACHE2\0")
        f.write(struct.pack("<iiffi", 1, v, 0.0, 1.0, n))
        f.write(frames.astype("<f4").tobytes())


def _write_obj(path: Path, verts: np.ndarray, faces: np.ndarray):
    lines = [f"v {x:.5f} {y:.5f} {z:.5f}" for x, y, z in verts]
    lines += [f"f {a + 1} {b + 1} {c + 1}" for a, b, c in faces]
    path.write_text("\n".join(lines) + "\n")


def capture(video: str, mask_video: str, out: str, camera: str | None = None, device: str = "cuda",
            smooth: float = 1.2) -> dict:
    import cv2
    import torch
    from PIL import Image

    repo = ensure_repo("sam-3d-body", REPO_ZIP, "sam_3d_body/sam_3d_body_estimator.py")
    if str(repo) not in sys.path:
        sys.path.insert(0, str(repo))
    from sam_3d_body import SAM3DBodyEstimator, load_sam_3d_body_hf

    out_json = Path(out)
    fps = _probe_fps(video)
    with tempfile.TemporaryDirectory() as tmp:
        fdir, mdir = Path(tmp) / "f", Path(tmp) / "m"
        n = extract_frames(video, fdir, 1080)
        extract_frames(mask_video, mdir, 1080)
        files, mfiles = sorted(fdir.glob("*.jpg")), sorted(mdir.glob("*.jpg"))
        with Image.open(files[0]) as im:
            fw, fh = im.size
        K = None
        if camera:
            c = json.loads(Path(camera).read_text())
            s = fw / float(c["width"])
            i = c["intrinsics"]
            K = torch.tensor([[[i["fx"] * s, 0, i["cx"] * s], [0, i["fy"] * s, i["cy"] * s], [0, 0, 1]]], dtype=torch.float32, device=device)

        model, cfg = load_sam_3d_body_hf(MODEL_ID)
        est = SAM3DBodyEstimator(model.to(device).eval(), cfg)
        verts, kps, focal, found = [], [], None, 0
        for fi in range(n):
            img = cv2.imread(str(files[fi]))[:, :, ::-1].copy()
            mask = None
            if fi < len(mfiles):
                mask = np.asarray(Image.open(mfiles[fi]).convert("L").resize((fw, fh))) > 128
            box = None
            if mask is not None and mask.sum() > 200:
                ys, xs = np.nonzero(mask)
                box = np.array([[xs.min(), ys.min(), xs.max(), ys.max()]], np.float32)
            if box is None:
                if verts:
                    verts.append(verts[-1])
                    kps.append(kps[-1])
                continue
            res = est.process_one_image(img, bboxes=box, cam_int=K)
            if not res:
                if verts:
                    verts.append(verts[-1])
                    kps.append(kps[-1])
                continue
            o = res[0]
            t = np.asarray(o["pred_cam_t"], np.float32)
            if not verts:
                # Frames before the first detection get the first result.
                verts.extend([None] * fi)
                kps.extend([None] * fi)
            verts.append(np.asarray(o["pred_vertices"], np.float32) + t)
            kps.append(np.asarray(o["pred_keypoints_3d"], np.float32) + t)
            focal = focal or float(o["focal_length"])
            found += 1
        if not found:
            raise RuntimeError("no person found in the cutout — run Video: cut out subject first")
        first = next(v for v in verts if v is not None)
        firstk = next(k for k in kps if k is not None)
        verts = np.stack([first if v is None else v for v in verts])
        kps = np.stack([firstk if k is None else k for k in kps])
        faces = np.asarray(est.faces)

    verts, kps = _smooth(verts, smooth), _smooth(kps, smooth)
    stem = out_json.with_suffix("")
    obj, pc2 = stem.with_suffix(".obj"), stem.with_suffix(".pc2")
    _write_obj(obj, verts[0], faces)
    _write_pc2(pc2, verts, fps)
    if K is not None:
        intr = {"fx": float(K[0, 0, 0]), "fy": float(K[0, 1, 1]), "cx": float(K[0, 0, 2]), "cy": float(K[0, 1, 2])}
    else:
        intr = {"fx": focal, "fy": focal, "cx": fw / 2, "cy": fh / 2}
    lowest = int(np.argmax(verts[0][:, 1]))
    result = {
        "photohop_body": 1,
        "fps": fps,
        "frames": len(verts),
        "width": fw,
        "height": fh,
        "intrinsics": intr,
        "convention": "opencv camera space (x right, y down, z forward), metres",
        "mesh": obj.name,
        "cache": pc2.name,
        "model": MODEL_ID,
        "feet_point": verts[0][lowest].round(4).tolist(),
        "hands": {name: [_hand_frame(k, ids) for k in kps] for name, ids in HANDS.items()},
    }
    out_json.write_text(json.dumps(result))
    return {"frames": len(verts), "detected": found, "vertices": int(verts.shape[1]), "distance_m": round(float(np.median(verts[0][:, 2])), 2)}
