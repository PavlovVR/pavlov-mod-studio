"""Write the file installed apps read to find a new version, beside a copy of the installer."""
import json
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = "https://github.com/PavlovVR/pavlov-mod-studio"


def manifest(version: str, notes: str, signature: str, installer: str, date: datetime) -> dict:
    url = f"{REPO}/releases/download/v{version}/{installer}"
    return {
        "version": version,
        "notes": notes,
        "pub_date": date.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {"windows-x86_64": {"signature": signature, "url": url}},
    }


def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    config = (ROOT / "app/src-tauri/tauri.conf.json").read_text(encoding="utf-8")
    version = json.loads(config)["version"]
    built = ROOT / "target/release/bundle/nsis"
    source = next(built.glob(f"*_{version}_x64-setup.exe"))
    signature = source.with_name(source.name + ".sig").read_text(encoding="utf-8").strip()
    installer = f"Pavlov-Mod-Studio_{version}_x64-setup.exe"
    shutil.copyfile(source, out / installer)
    notes = (ROOT / "release-notes.md").read_text(encoding="utf-8").strip()
    data = manifest(version, notes, signature, installer, datetime.now(timezone.utc))
    (out / "latest.json").write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    print(f"Update manifest for {version} written")


if __name__ == "__main__":
    main()
