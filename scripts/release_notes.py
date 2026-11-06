"""Check the app's version has a CHANGELOG.md section, and save it as release notes if given a path."""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def section(text: str, version: str) -> str:
    """The notes for one version, without its heading."""
    found = re.search(rf"^## {re.escape(version)}\n(.*?)(?=^## |\Z)", text, re.M | re.S)
    return found.group(1).strip() if found else ""


def main() -> None:
    config = (ROOT / "app/src-tauri/tauri.conf.json").read_text(encoding="utf-8")
    version = json.loads(config)["version"]
    notes = section((ROOT / "CHANGELOG.md").read_text(encoding="utf-8"), version)
    if not re.search(r"^- ", notes, re.M):
        sys.exit(f"CHANGELOG.md has no changes listed for {version}")
    if len(sys.argv) > 1:
        Path(sys.argv[1]).write_text(notes + "\n", encoding="utf-8")
    print(f"Change list for {version} found")


if __name__ == "__main__":
    main()
