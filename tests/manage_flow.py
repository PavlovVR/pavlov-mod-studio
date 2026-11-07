"""Local fake mod.io: checks every management call's method, path, format and failure handling."""
import json, os, re, subprocess, sys, tempfile, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse, parse_qs

BINARY = Path(__file__).resolve().parents[1] / "target/release/pavlov-mod-studio.exe"
MOD = "/games/3959/mods/99"
USER = {"id": 7, "username": "Hyp", "avatar": {"thumb_50x50": "a.png"}, "profile_url": "u"}


def listing(id, name, author):
    return {"id": id, "name": name, "game_id": 3959, "visible": 1, "tags": [{"name": "Mod"}],
            "submitted_by": {"username": author}, "stats": {"downloads_total": 5, "subscribers_total": 2}}


S = dict(requests=[], me_status=200, refuse=None, flaky=0, file_pages=0, bad_token=False, limited=[], live=[])


class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass

    def out(self, code, obj=None, retry_after=None):
        body = b"" if obj is None else json.dumps(obj).encode()
        self.send_response(code)
        if retry_after is not None:
            self.send_header("retry-after", str(retry_after))
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def write(self):
        size = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(size)
        kind = self.headers.get("Content-Type", "")
        fields = parse_qs(raw.decode(), keep_blank_values=True) if kind.startswith("application/x-www-form-urlencoded") else {}
        if kind.startswith("multipart/form-data"):
            for name, value in re.findall(r'name="([^"]+)"\r\n\r\n(.*?)\r\n--', raw.decode(errors="replace"), re.S):
                fields.setdefault(name, []).append(value)
        S["requests"].append(dict(method=self.command, path=urlparse(self.path).path, kind=kind, fields=fields, raw=raw))
        # Real mod.io refuses anything else here, whatever its docs say about plain fields.
        if self.command == "POST" and urlparse(self.path).path == MOD and not kind.startswith("multipart/form-data"):
            return self.out(415, {"error": {"code": 415, "message": "Incorrect Content-Type header in request, must be multipart/form-data"}})
        if S["refuse"]:
            code, ref = S["refuse"]
            return self.out(code, {"error": {"code": code, "error_ref": ref, "message": "refused"}})
        self.out(204 if self.command == "DELETE" else 200, None if self.command == "DELETE" else {})

    do_POST = do_PUT = do_DELETE = write

    def do_GET(self):
        url = urlparse(self.path)
        p, q = url.path, parse_qs(url.query)
        S["requests"].append(dict(method="GET", path=p, query=q, at=time.monotonic()))
        if S["limited"]:
            seconds, ref = S["limited"].pop(0)
            return self.out(429, {"error": {"code": 429, "error_ref": ref, "message": "slow down"}}, retry_after=seconds)
        if S["flaky"] > 0:
            S["flaky"] -= 1
            return self.out(503, {})
        if S["bad_token"]:
            return self.out(401, {"error": {"code": 401, "error_ref": 11005, "message": "bad"}})
        offset = int(q.get("_offset", ["0"])[0])
        if p == "/me":
            if S["me_status"] != 200:
                return self.out(403, {"error": {"code": 403, "error_ref": 11139, "message": "token"}})
            return self.out(200, {"username": "Hyp"})
        if p == "/me/mods":
            return self.out(200, {"data": [listing(99, "Mine", "Hyp"), listing(55, "Remote only", "Hyp")], "result_total": 2})
        if p == "/games/3959/mods":
            ids = [int(i) for i in q.get("id-in", ["99"])[0].split(",")]
            known = {99: listing(99, "Mine", "Hyp"), 2: listing(2, "Team mod", "Other"), 3: listing(3, "Theirs", "Other")}
            found = [known[i] for i in ids if i in known]
            return self.out(200, {"data": found, "result_total": len(found)})
        if p.endswith("/team"):
            member = [{"user": USER, "level": 8, "position": "Founder", "invite_pending": 0}]
            return self.out(200, {"data": member if "/mods/2/" in p or "/mods/99/" in p else [], "result_total": 1})
        if p == MOD:
            return self.out(200, {**listing(99, "Mine", "Hyp"), "platforms": [{"platform": "windows", "modfile_live": id} for id in S["live"]]})
        if p == f"{MOD}/files" and "id-in" in q:
            ids = [int(i) for i in q["id-in"][0].split(",")]
            return self.out(200, {"data": [{"id": i, "version": f"v{i}", "platforms": []} for i in ids], "result_total": len(ids)})
        if p == f"{MOD}/files":
            # Two pages, to prove nothing past the first hundred is lost.
            S["file_pages"] += 1
            page = [{"id": 1000 + offset + i, "version": str(i), "platforms": []} for i in range(100 if offset == 0 else 3)]
            return self.out(200, {"data": page, "result_total": 103})
        if p == f"{MOD}/dependencies":
            return self.out(200, {"data": [listing(5, "Needed", "Other")], "result_total": 1})
        if p == f"{MOD}/metadatakvp":
            return self.out(200, {"data": [{"metakey": "a", "metavalue": "b"}], "result_total": 1})
        if p == f"{MOD}/comments":
            return self.out(200, {"data": [{"id": 1, "content": "hello", "user": USER}], "result_total": 1})
        if p == f"{MOD}/events":
            return self.out(200, {"data": [{"id": 1, "date_added": 5, "event_type": "MOD_EDITED"}]})
        self.out(404, {"error": {"code": 404, "error_ref": 15022, "message": "not found"}})


server = ThreadingHTTPServer(("127.0.0.1", 0), H)
threading.Thread(target=server.serve_forever, daemon=True).start()
ENV = dict(os.environ, MODIO_TOKEN="t", PMS_API_BASE=f"http://127.0.0.1:{server.server_port}", MSYS_NO_PATHCONV="1")


def cli(*args, ok=True):
    run = subprocess.run([str(BINARY), *map(str, args)], env=ENV, capture_output=True, text=True, timeout=120)
    assert (run.returncode == 0) == ok, (args, run.returncode, run.stdout, run.stderr)
    return run.stdout + run.stderr


def last():
    return S["requests"][-1]


def expect(method, path, fields=None, kind="application/x-www-form-urlencoded"):
    request = last()
    assert (request["method"], request["path"]) == (method, path), request
    if kind:
        assert request["kind"].startswith(kind), request
    if fields is not None:
        assert request["fields"] == fields, request["fields"]


# Which mods are yours: if mod.io says who the token is, that includes mods not in the modkit.
out = cli("mods", 99, 3)
assert "99  Mine" in out and "yours" in out, out
assert "55  Remote only" in out, out
assert "3  Theirs  by Other  5 downloads  2 subscribers  not yours" in out, out

# A personal token can't, so nothing is claimed unless the user gives a username.
S["me_status"] = 403
out = cli("mods", 99, 3)
assert out.count("unknown") == 2, out
out = cli("mods", 99, 2, 3, "--username", "hyp")
assert "99  Mine  by Hyp  5 downloads  2 subscribers  yours" in out, out
assert "2  Team mod  by Other  5 downloads  2 subscribers  yours" in out, out
assert "not yours" in out, out
S["me_status"] = 200

# Lists.
assert cli("files", 99).count("\n") == 103 and S["file_pages"] >= 2
assert "5  Needed" in cli("dependencies", 99)
assert "a: b" in cli("metadata", 99)
assert "Hyp  level 8  Founder" in cli("team", 99)
assert "hello" in cli("comments", 99)
assert "MOD_EDITED" in cli("activity", 99)

# Files.
cli("edit-file", 99, 1234, "--file-version", "12", "--changelog", "Fixed it")
expect("PUT", f"{MOD}/files/1234", {"version": ["12"], "changelog": ["Fixed it"]})
cli("delete-file", 99, 1234)
expect("DELETE", f"{MOD}/files/1234", kind=None)

# The page.
cli("page", 99, "--description", "<p>Hi</p>", "--homepage", "https://a.example", "--name-id", "my-mod", "--maturity", 4, "--community", 1, "--credits", 578)
expect("POST", MOD, {"description": ["<p>Hi</p>"], "homepage_url": ["https://a.example"], "name_id": ["my-mod"],
                     "maturity_option": ["4"], "community_options": ["1"], "credit_options": ["578"]}, kind="multipart/form-data")
cli("hide", 99)
expect("POST", MOD, {"visible": ["0"]}, kind="multipart/form-data")
cli("show", 99)
expect("POST", MOD, {"visible": ["1"]}, kind="multipart/form-data")
cli("edit", 99, "--name", "New name", "--summary", "New summary")
expect("POST", MOD, {"name": ["New name"], "summary": ["New summary"]}, kind="multipart/form-data")

# Closing needs a clear yes.
before = len(S["requests"])
assert "--yes" in cli("close", 99, ok=False)
assert len(S["requests"]) == before
cli("close", 99, "--yes")
expect("DELETE", MOD, kind=None)

# Gallery.
with tempfile.TemporaryDirectory() as folder:
    image = Path(folder) / "shot.png"
    image.write_bytes(b"\x89PNG fake")
    cli("add-images", 99, image)
    expect("POST", f"{MOD}/media", kind="multipart/form-data")
    assert b'name="image0"' in last()["raw"] and b'filename="shot.png"' in last()["raw"], last()["raw"]
    text = Path(folder) / "notes.txt"
    text.write_bytes(b"x")
    before = len(S["requests"])
    assert "PNG or JPG" in cli("add-images", 99, text, ok=False)
    big = Path(folder) / "big.png"
    big.write_bytes(b"x" * (8 * 1024 * 1024 + 1))
    assert "8 MB" in cli("add-images", 99, big, ok=False)
    assert len(S["requests"]) == before
cli("add-links", 99, "--youtube", "https://youtu.be/x", "--sketchfab", "https://sketchfab.com/m/y")
expect("POST", f"{MOD}/media", kind="multipart/form-data")
assert b'name="youtube[]"' in last()["raw"] and b'name="sketchfab[]"' in last()["raw"]
assert "not a YouTube link" in cli("add-links", 99, "--youtube", "https://evil.example", ok=False)
cli("remove-media", 99, "--image", "a.png", "--image", "b.png", "--youtube", "https://youtu.be/x")
expect("DELETE", f"{MOD}/media", {"images[]": ["a.png", "b.png"], "youtube[]": ["https://youtu.be/x"]})
cli("reorder-images", 99, "b.png", "a.png")
expect("PUT", f"{MOD}/media/reorder", {"images[]": ["b.png", "a.png"]})

# Dependencies and metadata. Each write is followed by a read of the list.
writes = lambda: [r for r in S["requests"] if r["method"] != "GET"]
cli("dependencies", 99, "--add", 5)
assert (writes()[-1]["method"], writes()[-1]["path"], writes()[-1]["fields"]) == ("POST", f"{MOD}/dependencies", {"dependencies[]": ["5"]})
cli("dependencies", 99, "--remove", 5)
assert (writes()[-1]["method"], writes()[-1]["fields"]) == ("DELETE", {"dependencies[]": ["5"]})
assert "depend on itself" in cli("dependencies", 99, "--add", 99, ok=False)
cli("metadata", 99, "--add", "colour:dark:blue")
assert writes()[-1]["fields"] == {"metadata[]": ["colour:dark:blue"]}, writes()[-1]
cli("metadata", 99, "--remove", "colour:dark:blue")
assert writes()[-1]["method"] == "DELETE"

# Comments.
cli("comment", 99, "Thanks")
expect("POST", f"{MOD}/comments", {"content": ["Thanks"]})
cli("comment", 99, "Replying", "--reply-to", 8)
expect("POST", f"{MOD}/comments", {"content": ["Replying"], "reply_id": ["8"]})
assert "empty" in cli("comment", 99, "   ", ok=False)
cli("delete-comment", 99, 8)
expect("DELETE", f"{MOD}/comments/8", kind=None)

# Choosing what's in a build. Selections live outside the plugin, so they never ship.
with tempfile.TemporaryDirectory() as project:
    content = Path(project) / "Plugins/UGC99/Content"
    (content / "Music").mkdir(parents=True)
    (content / "definition.uasset").write_bytes(b"x")
    (content / "Music/a.uasset").write_bytes(b"\x00/UGC99/Music/b\x00")
    (content / "Music/b.uasset").write_bytes(b"x")
    (content / "Music/c.uasset").write_bytes(b"x")
    where = ("--project", project)
    assert "Music/a  " in cli("assets", 99, *where)
    assert "1 assets left out" in cli("exclude", 99, "Music/c", *where)
    saved = Path(project) / "Saved/PavlovModStudio/UGC99.selection.json"
    assert saved.is_file() and not list((Path(project) / "Plugins").rglob("*.selection.json"))
    assert "left out" in cli("assets", 99, *where)
    warned = cli("exclude", 99, "Music/b", *where)
    assert "2 assets left out" in warned and "Music/a uses Music/b" in warned, warned
    assert "definition" in cli("exclude", 99, "definition", *where, ok=False)

    # Another mod's use counts, and a pin beats leaving out.
    other = Path(project) / "Plugins/UGC55/Content"
    other.mkdir(parents=True)
    (other / "definition.uasset").write_bytes(b"x")
    wide = "/UGC99/Music/c".encode("utf-16-le")
    (other / "Map.umap").write_bytes(bytes(2) + wide + bytes(2))
    assert "UGC55:Map uses Music/c" in cli("exclude", 99, "--clear", "--project", project) + cli("exclude", 99, "Music/c", *where)
    assert "1 assets left out" in cli("exclude", 99, "--include", "Music/b", *where)
    assert "0 assets left out" in cli("exclude", 99, "--pin", "Music/c", *where)
    assert "0 assets left out" in cli("exclude", 99, "Music/c", *where)
    assert saved.is_file(), "a pin alone must be saved"
    cli("exclude", 99, "--unpin", "Music/c", *where)
    assert "1 assets left out" in cli("exclude", 99, "Music/c", *where)
    assert "0 assets left out" in cli("exclude", 99, "--clear", *where)
    assert not saved.exists()

# mod.io's limit. After a 429, nothing more is sent until the wait is over.
S["requests"].clear()
S["limited"] = [(2, 11008)]
assert "Founder" in cli("team", 99)
sent = S["requests"]
assert len(sent) == 2, [r["path"] for r in sent]
assert sent[1]["at"] - sent[0]["at"] >= 1.9, sent[1]["at"] - sent[0]["at"]

# A limit on one endpoint is waited out the same way.
S["requests"].clear()
S["limited"] = [(1, 11009)]
assert "Founder" in cli("team", 99)
assert len(S["requests"]) == 2

# What's live costs one read of the mod and one of its live files, however many files it has.
S["requests"].clear()
S["live"] = [4001, 4002]
out = cli("status", 99)
assert "version v4001" in out and "version v4002" in out, out
assert [r["path"] for r in S["requests"]] == [MOD, f"{MOD}/files"], [r["path"] for r in S["requests"]]
assert S["requests"][1]["query"]["id-in"] == ["4001,4002"], S["requests"][1]["query"]
S["live"] = []

# Refusals read as advice rather than raw JSON.
S["refuse"] = (403, 15009)
out = cli("delete-file", 99, 1, ok=False)
assert "Make another file live" in out and "error_ref" not in out, out
S["refuse"] = (403, 15013)
assert "not on this mod's team" in cli("hide", 99, ok=False)
S["refuse"] = (401, 11005)
assert "Make a new one in Settings" in cli("hide", 99, ok=False)
S["refuse"] = (401, 11003)
assert "Read and Write ticked" in cli("hide", 99, ok=False)
S["refuse"] = (403, 11139)
assert "All my mods and User actions" in cli("hide", 99, ok=False)
S["refuse"] = None

# A token is tried before it's kept, and a bad one is told apart from an expired one.
assert "accepted the token" in cli("check-token")
S["bad_token"] = True
assert "didn't accept that token" in cli("check-token", ok=False)
S["bad_token"] = False

# A brief server fault is retried.
S["flaky"] = 1
assert "Hyp  level 8" in cli("team", 99)

print("ALL PASSED")
