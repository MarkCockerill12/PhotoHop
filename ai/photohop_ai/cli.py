"""`photohop-ai` command line: `matte`, `segment`, `info`. Prints one JSON line with the result."""

import argparse
import json
import sys
import time


def _device() -> str:
    import torch

    if torch.cuda.is_available():
        return "cuda"
    if getattr(torch.backends, "mps", None) and torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def cmd_info(_args) -> dict:
    import torch

    dev = _device()
    out = {"device": dev, "torch": torch.__version__}
    if dev == "cuda":
        out["gpu"] = torch.cuda.get_device_name(0)
        out["vram_gb"] = round(torch.cuda.get_device_properties(0).total_memory / 2**30, 1)
    return out


def cmd_matte(args) -> dict:
    from .matte import matte

    dev = _device()
    res = args.res or (2048 if dev == "cuda" else 1024)
    cover = matte(args.input, args.output, model=args.model, device=dev, res=res)
    return {"output": args.output, "coverage": cover}


def cmd_segment(args) -> dict:
    from .segment import segment

    n, cover = segment(args.input, args.output, args.prompt, device=_device(), threshold=args.threshold)
    return {"output": args.output, "instances": n, "coverage": cover}


def cmd_normals(args) -> dict:
    from .normals import normals

    mean_z = normals(args.input, args.output, device=_device(), size=args.size)
    return {"output": args.output, "mean_z": mean_z}


def cmd_track(args) -> dict:
    from .video import track_box

    box = tuple(float(v) for v in args.box.split(","))
    n = track_box(args.input, args.output, box, frame=args.frame, device=_device())
    return {"output": args.output, "frames": n}


def cmd_video_matte(args) -> dict:
    import tempfile
    from pathlib import Path

    from .matte import matte
    from .video import extract_frames, matte_video

    with tempfile.TemporaryDirectory() as tmp:
        first = Path(tmp) / "f0"
        extract_frames(args.input, first, 1080)
        mask = Path(tmp) / "mask0.png"
        dev = _device()
        matte(str(first / "00001.jpg"), str(mask), device=dev, res=2048 if dev == "cuda" else 1024)
        # MatAnyone wants a binary first-frame mask.
        from PIL import Image
        Image.open(mask).point(lambda v: 255 if v > 127 else 0).save(mask)
        n = matte_video(args.input, args.output, str(mask), device=dev, max_side=args.max_side)
    return {"output": args.output, "frames": n}


def cmd_remove(args) -> dict:
    from .inpaint import remove

    cover = remove(args.input, args.output, args.prompt, args.fill, device=_device())
    return {"output": args.output, "coverage": cover}


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="photohop-ai")
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("info", help="report device / GPU")
    m = sub.add_parser("matte", help="automatic subject cutout -> greyscale alpha PNG")
    m.add_argument("input")
    m.add_argument("output")
    m.add_argument("--model", default="ZhengPeng7/BiRefNet_HR")
    m.add_argument("--res", type=int, default=0, help="network resolution (default: 2048 on GPU, 1024 on CPU)")
    s = sub.add_parser("segment", help="text-prompted mask (SAM 3) -> greyscale PNG")
    s.add_argument("input")
    s.add_argument("output")
    s.add_argument("--prompt", required=True)
    s.add_argument("--threshold", type=float, default=0.4)
    nrm = sub.add_parser("normals", help="surface normals of people (Sapiens2) -> RGB normal map PNG")
    nrm.add_argument("input")
    nrm.add_argument("output")
    nrm.add_argument("--size", default="1b", choices=["0.4b", "0.8b", "1b"])
    tr = sub.add_parser("track", help="track a boxed region through a video (SAM 2.1) -> mask video")
    tr.add_argument("input")
    tr.add_argument("output")
    tr.add_argument("--box", required=True, help="x0,y0,x1,y1 in video pixels")
    tr.add_argument("--frame", type=int, default=0)
    vm = sub.add_parser("video-matte", help="cut the main subject out of a video (BiRefNet + MatAnyone 2) -> mask video")
    vm.add_argument("input")
    vm.add_argument("output")
    vm.add_argument("--max-side", type=int, default=720)
    rm = sub.add_parser("remove", help="repaint an object away (SAM 3 mask + FLUX.2 klein) -> RGBA patch PNG")
    rm.add_argument("input")
    rm.add_argument("output")
    rm.add_argument("--prompt", required=True, help="what to remove, e.g. 'toy gun'")
    rm.add_argument("--fill", default="empty background, seamless continuation of the surrounding scene, no people, no faces, no objects, photo",
                    help="what should be there instead")
    args = p.parse_args(argv)
    t0 = time.time()
    try:
        cmds = {"info": cmd_info, "matte": cmd_matte, "segment": cmd_segment, "normals": cmd_normals,
                "track": cmd_track, "video-matte": cmd_video_matte, "remove": cmd_remove}
        result = cmds[args.cmd](args)
        result.update(ok=True, seconds=round(time.time() - t0, 2))
    except Exception as e:  # noqa: BLE001 - report every failure as JSON for the app
        result = {"ok": False, "error": f"{type(e).__name__}: {e}"}
    print(json.dumps(result), flush=True)
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    sys.exit(main())
