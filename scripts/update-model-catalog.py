"""Refresh pinned public model metadata; never download GGUF weights here."""
import hashlib
import json
from pathlib import Path
from urllib.request import urlopen

MODELS = [
    ("potion", "Potion base 32M", "minishlab/potion-base-32M", "mit", ["config.json", "tokenizer.json", "model.safetensors"]),
    ("lfm", "LFM 2.5 1.2B Instruct", "LiquidAI/LFM2.5-1.2B-Instruct-GGUF", "lfm1.0", ["LFM2.5-1.2B-Instruct-Q4_K_M.gguf"]),
    ("granite", "Granite 4.1 3B", "ibm-granite/granite-4.1-3b-GGUF", "apache-2.0", ["granite-4.1-3b-Q4_K_M.gguf"]),
    ("qwen", "Qwen3.5 2B", "unsloth/Qwen3.5-2B-GGUF", "apache-2.0", ["Qwen3.5-2B-Q4_K_M.gguf"]),
    ("lfm-8b", "LFM2.5 8B-A1B", "LiquidAI/LFM2.5-8B-A1B-GGUF", "lfm1.0", ["LFM2.5-8B-A1B-Q4_K_M.gguf"]),
]

catalog = []
for model_id, name, repo, license_id, names in MODELS:
    with urlopen(f"https://huggingface.co/api/models/{repo}?blobs=true", timeout=60) as r:
        metadata = json.load(r)
    revision = metadata["sha"]
    files = []
    siblings = {s["rfilename"]: s for s in metadata["siblings"]}
    for filename in names:
        entry = siblings[filename]
        url = f"https://huggingface.co/{repo}/resolve/{revision}/{filename}"
        if "lfs" in entry:
            digest, size = entry["lfs"]["sha256"], entry["lfs"]["size"]
        else:
            with urlopen(url, timeout=60) as r:
                data = r.read(8 * 1024 * 1024 + 1)
            assert len(data) <= 8 * 1024 * 1024
            digest, size = hashlib.sha256(data).hexdigest(), len(data)
        files.append({"name": filename, "url": url, "sha256": digest, "bytes": size})
    catalog.append({"id": model_id, "name": name, "repository": repo, "revision": revision, "license": license_id, "files": files})
out = Path(__file__).resolve().parents[1] / "crates/arbiter-intake/src/catalog.json"
out.write_text(json.dumps(catalog, indent=2) + "\n", encoding="utf-8")
print("Pinned:", ", ".join(f"{m['id']} ({sum(f['bytes'] for f in m['files']):,} bytes)" for m in catalog))
