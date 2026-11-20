"""3D camera solve for a video with Depth Anything 3 (multi-view: camera poses, intrinsics and metric
depth from many frames at once). Writes camera.json: per-frame camera-to-world matrices (OpenCV
convention: x right, y down, z forward; world = first keyframe's camera), intrinsics in original
pixels, and a ground plane fitted to the depth (RANSAC), for the Blender bridge and match-moving."""

import json
import tempfile
from pathlib import Path

import numpy as np

from .video import _probe_fps, extract_frames


def _ransac_plane(pts: np.ndarray, iters: int = 400, tol: float = 0.03, seed: int = 0):
    rng = np.random.default_rng(seed)
    best, best_n = None, 0
    for _ in range(iters):
        a, b, c = pts[rng.choice(len(pts), 3, replace=False)]
        n = np.cross(b - a, c - a)
        if np.linalg.norm(n) < 1e-8:
            continue
        n = n / np.linalg.norm(n)
        d = -n @ a
        inl = np.abs(pts @ n + d) < tol
        if inl.sum() > best_n:
            best, best_n = (n, d, inl), inl.sum()
    if best is None:
        return None
    n, d, inl = best
    # Refit on inliers (least squares via SVD).
    p = pts[inl]
    centroid = p.mean(0)
    n = np.linalg.svd(p - centroid)[2][-1]
    return n, centroid, inl.mean()


def solve(video: str, out: str, device: str = "cuda", keyframes: int = 48, mask_video: str | None = None,
          model_id: str = "depth-anything/DA3NESTED-GIANT-LARGE-1.1") -> dict:
    from depth_anything_3.api import DepthAnything3
    from PIL import Image
    from scipy.spatial.transform import Rotation, Slerp

    fps = _probe_fps(video)
    with tempfile.TemporaryDirectory() as tmp:
        folder = Path(tmp) / "f"
        n = extract_frames(video, folder, 1080)
        files = sorted(folder.glob("*.jpg"))
        with Image.open(files[0]) as im:
            fw, fh = im.size
        step = max(1, int(np.ceil(n / keyframes)))
        keys = list(range(0, n, step))
        if keys[-1] != n - 1:
            keys.append(n - 1)
        model = DepthAnything3.from_pretrained(model_id).to(device).eval()
        pred = model.inference([str(files[k]) for k in keys])
        w2c = np.asarray(pred.extrinsics, np.float64)
        K = np.asarray(pred.intrinsics, np.float64)
        depth = np.asarray(pred.depth, np.float64)
        conf = np.asarray(pred.conf, np.float64)
        ph, pw = depth.shape[1:]

    # Camera-to-world per keyframe, then interpolate every frame.
    c2w = np.zeros((len(keys), 4, 4))
    for i, m in enumerate(w2c):
        full = np.eye(4)
        full[:3, :4] = m
        c2w[i] = np.linalg.inv(full)
    rots = Rotation.from_matrix(c2w[:, :3, :3])
    slerp = Slerp(keys, rots)
    frames = []
    for f in range(n):
        rot = slerp([min(max(f, keys[0]), keys[-1])]).as_matrix()[0]
        t = np.array([np.interp(f, keys, c2w[:, a, 3]) for a in range(3)])
        m = np.eye(4)
        m[:3, :3], m[:3, 3] = rot, t
        frames.append(m.round(6).tolist())

    # Ground plane from the first keyframe: lower 45 % of the image, confident depth, subject masked out.
    k0 = K[0]
    ys, xs = np.mgrid[0:ph, 0:pw]
    sel = (ys > ph * 0.55) & (conf[0] > np.percentile(conf[0], 30))
    if mask_video:
        with tempfile.TemporaryDirectory() as tmp:
            mf = Path(tmp) / "m"
            extract_frames(mask_video, mf, 1080)
            first = sorted(mf.glob("*.jpg"))[keys[0]]
            m = np.asarray(Image.open(first).convert("L").resize((pw, ph))) > 64
            sel &= ~m
    z = depth[0][sel]
    x = (xs[sel] - k0[0, 2]) / k0[0, 0] * z
    y = (ys[sel] - k0[1, 2]) / k0[1, 1] * z
    pts_cam = np.stack([x, y, z], 1)
    pts_world = (c2w[0][:3, :3] @ pts_cam.T).T + c2w[0][:3, 3]
    ground = None
    if len(pts_world) > 50:
        res = _ransac_plane(pts_world[:: max(1, len(pts_world) // 4000)], tol=0.02 * float(np.median(z)))
        if res is not None:
            nrm, centroid, frac = res
            if nrm[1] > 0:  # OpenCV world: y points down, so "up" has negative y
                nrm = -nrm
            ground = {"point": centroid.round(5).tolist(), "normal": nrm.round(6).tolist(), "inlier_fraction": round(float(frac), 3)}

    sx, sy = fw / pw, fh / ph
    result = {
        "photohop_camera": 1,
        "fps": fps,
        "frames": n,
        "width": fw,
        "height": fh,
        "metric": bool(getattr(pred, "is_metric", 0)),
        "model": model_id,
        "intrinsics": {"fx": K[0][0, 0] * sx, "fy": K[0][1, 1] * sy, "cx": K[0][0, 2] * sx, "cy": K[0][1, 2] * sy},
        "convention": "opencv camera-to-world (x right, y down, z forward)",
        "camera_to_world": frames,
        "ground": ground,
        "keyframes": keys,
    }
    Path(out).write_text(json.dumps(result))
    travel = float(np.linalg.norm(c2w[-1][:3, 3] - c2w[0][:3, 3]))
    return {"frames": n, "keyframes": len(keys), "travel_m": round(travel, 3), "ground": ground is not None}
