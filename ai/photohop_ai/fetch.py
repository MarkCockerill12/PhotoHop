"""Download a GitHub repository snapshot into PhotoHop's AI cache on first use (for model code
whose licence we don't want to vendor: Harmonizer, SAM 3D Body)."""

import io
import os
import urllib.request
import zipfile
from pathlib import Path


def cache_root() -> Path:
    return Path(os.environ.get("PHOTOHOP_AI_CACHE") or Path.home() / ".cache" / "photohop-ai")


def ensure_repo(name: str, zip_url: str, marker: str) -> Path:
    """Return the extracted repo folder containing `marker` (a relative path), downloading if needed."""
    root = cache_root() / name

    def find():
        if root.is_dir():
            for hit in root.glob(f"*/{marker}"):
                return Path(str(hit)[: -len(marker)].rstrip("/\\"))
        return None

    found = find()
    if found is None:
        root.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(zip_url, timeout=300) as r:
            zipfile.ZipFile(io.BytesIO(r.read())).extractall(root)
        found = find()
    if found is None:
        raise RuntimeError(f"{name} download did not contain {marker}")
    return found
