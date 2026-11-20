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
    args = p.parse_args(argv)
    t0 = time.time()
    try:
        cmds = {"info": cmd_info, "matte": cmd_matte, "segment": cmd_segment, "normals": cmd_normals}
        result = cmds[args.cmd](args)
        result.update(ok=True, seconds=round(time.time() - t0, 2))
    except Exception as e:  # noqa: BLE001 - report every failure as JSON for the app
        result = {"ok": False, "error": f"{type(e).__name__}: {e}"}
    print(json.dumps(result), flush=True)
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    sys.exit(main())
