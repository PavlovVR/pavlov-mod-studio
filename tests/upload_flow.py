"""Local fake mod.io: drops a part, dies mid-run, then checks resume and final bytes."""
import hashlib, json, os, re, subprocess, sys, tempfile, threading, zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse, parse_qs

BINARY = Path(__file__).resolve().parents[1] / "target/release/pavlov-mod-studio.exe"
PART = 1024
S = dict(names=[], existing=[], parts={}, fail_once={2}, kill_at=None, puts=0, sessions=0, modfile=None, auth=set())

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def out(self, code, obj):
        b = json.dumps(obj).encode()
        self.send_response(code); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def body(self):
        return self.rfile.read(int(self.headers.get("Content-Length", 0)))
    def do_GET(self):
        S["auth"].add(self.headers.get("Authorization"))
        p = urlparse(self.path).path
        if p == "/me/mods": return self.out(200, {"data": [{"id": 1}]})
        if p.endswith("/files"): return self.out(200, {"data": S["existing"]})
        if p.endswith("/sessions"): return self.out(200, {"data": [{"upload_id": "U1", "status": 3 if S.get("completed") else 0}]})
        self.out(200, {"metadata_blob": json.dumps({"ModType": "Map", "EngineVersion": "5.1.1", "VersionUUID": 7})})
    def do_PUT(self):
        data = self.body()
        if not urlparse(self.path).path.endswith("/multipart"):
            S["put"] = (urlparse(self.path).path, parse_qs(data.decode())); return self.out(200, {})
        S["puts"] += 1
        assert parse_qs(urlparse(self.path).query)["upload_id"] == ["U1"]
        rng = self.headers["Content-Range"].split()[1]
        start = int(rng.split("-")[0]); idx = start // PART
        if idx in S["fail_once"]:
            S["fail_once"].discard(idx); return self.out(503, {})
        if S["kill_at"] == idx:
            S["kill_at"] = None; self.out(503, {}); return
        S["parts"][start] = data; self.out(200, {"part_number": idx + 1})
    def do_POST(self):
        p = urlparse(self.path).path; b = self.body().decode(errors="replace")
        if p.endswith("/mods"):
            assert self.headers["Content-Type"].startswith("multipart/form-data")
            S["created"] = b; return self.out(201, {"id": 777})
        if p.endswith("/media"):
            assert self.headers["Content-Type"].startswith("multipart/form-data")
            S["media"] = b; return self.out(201, {})
        if p.endswith("/mods/99"):
            # Real mod.io refuses anything else here, whatever its docs say about plain fields.
            if not self.headers["Content-Type"].startswith("multipart/form-data"):
                return self.out(415, {"error": {"code": 415, "message": "Incorrect Content-Type header in request, must be multipart/form-data"}})
            fields = {k: [v] for k, v in re.findall(r'name="([^"]+)"\r\n\r\n(.*?)\r\n--', b, re.S)}
            if "metadata_blob" in fields: S["blob"] = fields["metadata_blob"][0]
            S["edit"] = fields; return self.out(200, {})
        if p.endswith("/files/multipart"):
            S["sessions"] += 1; S["names"] += parse_qs(b)["filename"]; return self.out(200, {"upload_id": "U1", "status": 0})
        if p.endswith("/complete"):
            S["completed"] = S.get("completed", 0) + 1; return self.out(200, {"upload_id": "U1", "status": 1})
        assert self.headers["Content-Type"].startswith("multipart/form-data"), self.headers["Content-Type"]
        S["modfile"] = {k: [v] for k, v in re.findall(r'name="([^"]+)"\s+(.*?)\s*--', b, re.S)}; self.out(201, {"id": 42})

srv = ThreadingHTTPServer(("127.0.0.1", 0), H)
threading.Thread(target=srv.serve_forever, daemon=True).start()
base = f"http://127.0.0.1:{srv.server_port}"

root = Path(tempfile.mkdtemp())
(root / "Plugins/UGC99").mkdir(parents=True)
cook = root / "Saved/Cooked/Workshop/UGC99/Android_ASTC/UGC99"; cook.mkdir(parents=True)
(cook / "UGC99pakchunk0-Android_ASTC.pak").write_bytes(os.urandom(6000))
(cook / "metadata.json").write_text('{"EngineVersion":"5.1.1","ModType":1}')
for folder, suffix in (("LinuxShackServer", "LinuxServer"), ("Windows", "Windows"), ("LinuxPavServer", "LinuxServer")):
    c = root / f"Saved/Cooked/Workshop/UGC99/{folder}/UGC99"; c.mkdir(parents=True)
    (c / f"UGC99pakchunk0-{suffix}.pak").write_bytes(os.urandom(1500))
    (c / "metadata.json").write_text("{}")
S["existing"] = [{"id": 5, "version": "7", "platforms": [{"platform": "mac"}]}]

def run(*extra, env_token="T", timeout=120, group="quest"):
    env = dict(os.environ); env.pop("MODIO_TOKEN", None)
    if env_token: env["MODIO_TOKEN"] = env_token
    return subprocess.run([str(BINARY), "upload", "99", group, "--project", str(root), "--api-base", base,
                           "--part-size", str(PART), "--poll-seconds", "0", "--skip-audit", *extra],
                          capture_output=True, text=True, env=env, timeout=timeout)

r = run("--dry-run", env_token=None); print(r.stdout, r.stderr); assert r.returncode == 0 and S["puts"] == 0
r = run(env_token=None); assert r.returncode != 0 and "MODIO_TOKEN" in r.stderr

# Interrupted run: state saved after 3 parts, as if the process was killed.
zp = cook.parent / "android.upload.zip"; st = zp.with_suffix(".state.json")
digest = hashlib.sha256(zp.read_bytes()).hexdigest(); blob = zp.read_bytes()
for s in (0, PART, 2 * PART): S["parts"][s] = blob[s:s + PART]
st.write_text(json.dumps({"upload_id": "U1", "sha256": digest, "size": len(blob), "part_size": PART, "done": [0, 1, 2]}))
S["fail_once"] = {3}

r = run("--changelog", "t", "--skip-existing"); print(r.stdout, r.stderr)
assert r.returncode == 0, r.stderr
assert "resuming: 3/" in r.stdout and "retry 1/" in r.stdout
assert S["sessions"] == 0, "resume must not open a new session"
assert b"".join(S["parts"][k] for k in sorted(S["parts"])) == blob, "reassembled bytes differ"
assert S["modfile"]["upload_id"] == ["U1"] and S["modfile"]["version"] == ["7"]
assert S["modfile"]["platforms[]"] == ["oculus"] and S["modfile"]["active"] == ["true"]
assert S["auth"] == {"Bearer T"}
want = "{" + chr(13) + chr(10) + chr(9) + '"ModType": "Map",' + chr(13) + chr(10) + chr(9) + '"EngineVersion": "5.1.1"' + chr(13) + chr(10) + "}"
assert S["modfile"]["metadata_blob"] == [want], S["modfile"]["metadata_blob"]
assert S["completed"] == 1
assert not st.exists()
with zipfile.ZipFile(zp) as z:
    assert sorted(z.namelist()) == ["UGC99pakchunk0-Android_ASTC.pak", "metadata.json"]
assert "[shack] skipped" in r.stdout
r = run("--version", "8", group="pc"); print(r.stdout, r.stderr); assert r.returncode == 0, r.stderr
assert json.loads(S["blob"]) == {"ModType": "Map", "EngineVersion": "5.1.1", "VersionUUID": 8}
assert "Successfully updated mod metadata" in r.stdout and "Upload Complete" in r.stdout
assert S["modfile"]["version"] == ["8"]
assert r.stdout.index("[windows] done") < r.stdout.index("[linux] done")
assert S["names"] == ["modfile_99.zip", "modfile_99.zip"], S["names"]
r = run(group="both", env_token=None); assert r.returncode != 0

# Write calls, using the method and fields mod.io documents.
def cli(*args):
    env = dict(os.environ, MODIO_TOKEN="T", PMS_API_BASE=base)
    return subprocess.run([str(BINARY), *args], capture_output=True, text=True, env=env, timeout=60)

r = cli("make-live", "99", "1234"); assert r.returncode == 0, r.stderr
assert S["put"][0].endswith("/mods/99/files/1234") and S["put"][1]["active"] == ["true"], S["put"]

logo = root / "logo.png"; logo.write_bytes(b"PNGDATA")
r = cli("edit", "99", "--name", "New name", "--summary", "New summary", "--logo", str(logo)); assert r.returncode == 0, r.stderr
assert S["edit"]["name"] == ["New name"] and S["edit"]["summary"] == ["New summary"]
assert 'name="logo"' in S["media"] and "PNGDATA" in S["media"]
r = cli("edit", "99", "--name", "New name", "--summary", " "); assert r.returncode != 0 and "summary" in r.stderr

# mod.io succeeds, local setup fails: the error must name the id so setup can be retried.
r = cli("create", "Test map", "Map", "--summary", "A map", "--logo", str(logo), "--project", str(root), "--engine", str(root / "no-engine"))
assert r.returncode != 0 and "777" in r.stderr and "exists on mod.io" in r.stderr, r.stderr
for field in ('name="name"', "Test map", 'name="summary"', 'name="tags[]"', 'name="metadata_blob"', 'name="logo"', "PNGDATA", "ModType"):
    assert field in S["created"], field
assert 'name="visible"' not in S["created"], "normal mods must not send visible"
hidden_root = root / "hidden"; (hidden_root / "Plugins").mkdir(parents=True)
r = cli("create", "Test map", "Map", "--summary", "A map", "--logo", str(logo), "--hidden", "--project", str(hidden_root), "--engine", str(root / "no-engine"))
assert 'name="visible"' in S["created"], "hidden mods must send visible"
r = cli("create", "ab", "Map", "--summary", "x", "--logo", str(logo), "--project", str(root)); assert r.returncode != 0 and "3-32" in r.stderr
# Sharing sends what's on the list and nothing else, and only a real choice gets that far.
import io, struct
plugin = root / "Plugins/UGC99"
for name, text in {"Content/Api/BP_Api.uasset": "api", "Content/Api/S_Data.uasset": "data", "Content/Api/S_Data.uexp": "split",
                   "Content/Secret/M_Skin.uasset": "secret", "Resources/Icon128.png": "png", "NOTES.md": "private",
                   "Saved/Config/private.ini": "private"}.items():
    (plugin / name).parent.mkdir(parents=True, exist_ok=True); (plugin / name).write_text(text)
(plugin / "UGC99.uplugin").write_text('{"FileVersion": 3, "FriendlyName": "UGC99", "CanContainContent": true}')
(plugin / "Content/definition.uasset").write_bytes(struct.pack("<I", 11) + b"Pavlov_Mod" + bytes(1))
project = ("--project", str(root))

def received():
    with zipfile.ZipFile(io.BytesIO(S["parts"][0])) as z:
        return sorted(z.namelist()), json.loads(z.read("UGC99.uplugin"))

def fresh():
    S["parts"] = {}; S["sessions"] = 0; S.pop("completed", None); S["modfile"] = None

fresh()
r = cli("share", "99", *project); assert r.returncode == 0 and "share list is empty" in r.stdout, r.stdout
r = cli("share", "99", "--upload", *project); assert r.returncode != 0 and "share list is empty" in r.stderr, r.stderr
assert S["sessions"] == 0, "nothing may be sent without a choice"
r = cli("share", "99", "Saved/Config/private.ini", *project); assert r.returncode != 0, r.stdout
r = cli("share", "99", "../other", *project); assert r.returncode != 0, r.stdout
r = cli("share", "99", "Content/Api/BP_Api.uasset", "Content/Api/S_Data.uasset", *project); assert r.returncode == 0, r.stderr
r = cli("share", "99", "--upload", "--changelog", "API only", *project); print(r.stdout, r.stderr); assert r.returncode == 0, r.stderr
names, descriptor = received()
assert names == ["Content/Api/BP_Api.uasset", "Content/Api/S_Data.uasset", "Content/Api/S_Data.uexp", "UGC99.uplugin"], names
assert descriptor["FriendlyName"] == "[SOURCE] UGC99" and descriptor["CanContainContent"] is True, descriptor
assert S["modfile"]["platforms[]"] == ["source"] and S["modfile"]["active"] == ["true"], S["modfile"]
assert S["modfile"]["changelog"] == ["API only"] and S["modfile"]["version"] == ["7"], S["modfile"]
assert "checked against your list" in r.stdout and "Upload Complete" in r.stdout
assert json.loads((plugin / "UGC99.uplugin").read_text())["FriendlyName"] == "UGC99", "the real plugin must not change"

# A zip can be looked through first, and it matches what would be sent.
out = root / "check.zip"
r = cli("share", "99", "--export", str(out), *project); assert r.returncode == 0, r.stderr
with zipfile.ZipFile(out) as z: assert sorted(z.namelist()) == names

# The check is read-only and sees what's missing.
(plugin / "Content/Api/BP_Api.uasset").write_bytes(b"x\0/UGC99/Secret/M_Skin\0/UGC12/Lib/F\0")
r = cli("share", "99", "--check", *project); assert r.returncode == 0, r.stderr
assert "Content/Api/BP_Api.uasset needs Content/Secret/M_Skin.uasset" in r.stdout, r.stdout
assert "points at UGC12" in r.stdout, r.stdout

# Downloading your own shared mod would swap the real project for the shared part.
r = cli("download", "99", *project); assert r.returncode != 0 and "replace your project" in r.stderr, r.stderr
assert (plugin / "NOTES.md").is_file()
print("ALL PASSED")
