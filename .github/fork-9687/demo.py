# Fork-only demo for tangency-labs/solidcraft#11146 (#9687): a second planar joint left as a loop
# conflict (as a design saved before joint.create turned its part a half turn keeps it), then
# Edit Joint's OK on it, through the control channel under Xvfb. First two boards, then the
# report's Drucker.sc (Joint35, the rail on the 450 mm profile).
# Usage: demo.py <solidcraft binary> <output folder> <Drucker.sc or "">
import json, os, secrets, socket, subprocess, sys, time, zipfile

app_bin, out, drucker = sys.argv[1], sys.argv[2], sys.argv[3]
port = 18861
token = secrets.token_hex(32)
env = dict(os.environ, SOLIDCRAFT_CONTROL_TOKEN=token, SOLIDCRAFT_CONTROL_DIR=os.path.join(out, "control"))
log = open(os.path.join(out, "app.log"), "wb")
app = subprocess.Popen(["xvfb-run", "-n", "99", "-s", "-screen 0 1600x1000x24 -nolisten tcp", app_bin, "--control", str(port)],
                       env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
deadline = time.monotonic() + 180
while True:
    try:
        sock = socket.create_connection(("127.0.0.1", port), timeout=600)
        break
    except OSError:
        if app.poll() is not None or time.monotonic() > deadline:
            sys.exit("the app never listened")
        time.sleep(0.3)
io = sock.makefile("rw", encoding="utf-8")
n = 0


def call(method, params=None, quiet=False, ok=True):
    global n
    n += 1
    req = {"id": n, "method": method, "params": params or {}}
    if n == 1:
        req["token"] = token
    io.write(json.dumps(req) + "\n")
    io.flush()
    reply = json.loads(io.readline())
    if not quiet:
        print(">", method, json.dumps(params or {})[:300], flush=True)
    if ok and reply.get("ok") is not True:
        sys.exit(f"{method} failed: {reply}")
    return reply.get("result")


def execute(command, params=None, quiet=True):
    return call("engine.execute", {"command": command, "params": params or {}}, quiet)


def handles(prefix):
    r = call("ui.handles", {"prefix": prefix}, quiet=True)
    return r.get("handles", r) if isinstance(r, dict) else r


def find(prefix, pred, tries=40):
    for _ in range(tries):
        for h in handles(prefix):
            if pred(h["name"]):
                return h
        time.sleep(0.25)
    sys.exit(f"no handle under {prefix}: {[h['name'] for h in handles(prefix)]}")


def shot(name):
    time.sleep(1.5)
    path = os.path.join(out, name)
    r = call("ui.screenshot", {"path": path}, quiet=True, ok=False)
    if not (r is not None and os.path.exists(path)):
        subprocess.run(["import", "-display", ":99", "-window", "root", path], check=False)
    print("shot", name, flush=True)


def conflicts(label):
    r = execute("joint.solve", {"quiet": True})
    print(f"{label}: conflicts {json.dumps(r.get('conflicts'))}", flush=True)
    return r.get("conflicts") or []


def wait_joints(names, seconds=300):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        r = call("engine.execute", {"command": "joint.list", "params": {}}, quiet=True, ok=False) or {}
        have = {j.get("name") for j in (r.get("joints") or [])}
        if all(x in have for x in names):
            return
        time.sleep(0.5)
    sys.exit(f"the design never showed joints {names}")


def edit_joint_ok(joint, prefix):
    """Edit Joint on `joint` from its History row, then OK; screenshots along the way."""
    click = lambda h, double=False: call("ui.click", {"x": h["x"], "y": h["y"], "double": double}, quiet=True)
    click(find("", lambda name: name in (f"timeline-joint:{joint}", f"row:{joint}")), double=True)
    time.sleep(2.0)
    shot(f"{prefix}-2-edit-joint.png")
    click(find("dialog:", lambda name: name == "dialog:OK"))
    time.sleep(3.0)
    shot(f"{prefix}-3-after-ok.png")


call("ui.resize", {"width": 1600, "height": 1000}, ok=False)
call("ui.home", {"open": False}, ok=False)
time.sleep(1.0)

# 1. Two boards (yellow grounded), red on top of yellow on a planar joint, then a second planar
# joint between the two fronts, which point the same way: it meets with red turned a half turn.
for name, corner, color in [("Yellow", [0, 0, 0], "#e0b43c"), ("Red", [0, 300, 0], "#c8463c")]:
    execute("component.activate", {"component": "root"})
    execute("component.create", {"name": name, "grounded": False})
    execute("solid.box", {"length": 400, "width": 100, "height": 20, "corner": corner})
    if name == "Red":
        # A boss near one end, so a turn of the board shows.
        execute("solid.cylinder", {"base": [340, 350, 20], "axis": [0, 0, 1], "radius": 18, "height": 15, "operation": "join"})
    execute("appearance.assign", {"components": [name], "color": color})
execute("component.activate", {"component": "root"})
execute("occurrence.ground", {"occurrence": "Yellow:1", "grounded": True})
planar = lambda name, y, r: execute("joint.create", {"type": "planar", "name": name,
                                                       "a": {"occurrence": "Yellow", "face": y}, "b": {"occurrence": "Red", "face": r}})
planar("Top", [200, 50, 20], [200, 350, 0])
values = lambda: [v["value"] for v in next(j for j in execute("joint.list").get("joints", []) if j.get("name") == "Top")["values"]]
on_top = values()
print("Side made:", json.dumps(planar("Side", [200, 0, 10], [200, 0, 30]).get("conflicts")), flush=True)
# The design as one saved before joint.create turned red to meet Side has it: Side suppressed,
# red back on top as Top left it, saved, and Side unsuppressed in the saved file.
execute("joint.edit", {"joint": "Side", "suppressed": True})
execute("joint.drive", {"joint": "Top", "values": on_top})
print("Top values: made", json.dumps(on_top), "now", json.dumps(values()), flush=True)
saved, old = os.path.join(out, "boards.sc"), os.path.join(out, "boards-saved-as-conflict.sc")
execute("file.save", {"path": saved, "cache": False})
with zipfile.ZipFile(saved) as z, zipfile.ZipFile(old, "w", zipfile.ZIP_DEFLATED) as w:
    for item in z.infolist():
        data = z.read(item.filename)
        if item.filename == "document.json":
            d = json.loads(data)
            for j in d["assembly"]["joints"]:
                if j.get("name") == "Side":
                    j.pop("suppressed", None)
            data = json.dumps(d).encode()
        w.writestr(item, data)
execute("doc.open", {"path": old})
wait_joints(["Top", "Side"])
time.sleep(2.0)
call("ui.camera", {"yaw": -0.6, "pitch": 0.55, "fit": True}, ok=False)
conflicts("boards opened")
shot("boards-1-opened.png")
edit_joint_ok("Side", "boards")
conflicts("boards after Edit Joint OK")
execute("file.save", {"path": os.path.join(out, "boards-after.sc"), "cache": False})

# 2. The report's Drucker.sc: Joint35 holds the rail to the 450 mm profile.
if drucker:
    execute("doc.open", {"path": drucker})
    wait_joints(["Joint34", "Joint35"])
    time.sleep(3.0)
    call("ui.camera", {"yaw": 0.7, "pitch": 0.5, "fit": True}, ok=False)
    conflicts("Drucker opened")
    shot("drucker-1-opened.png")
    # Its History bar runs off the window, so the command Edit Joint's OK sends (type, flip and
    # offset as they are), not a click on the row.
    j35 = next(j for j in execute("joint.list").get("joints", []) if j.get("name") == "Joint35")
    print("Edit Joint OK on Joint35:", json.dumps(execute("joint.edit", {"joint": "Joint35", "type": j35["type"], "flip": j35["flip"],
                                                                          "offset": j35["offset"]}).get("conflicts")), flush=True)
    time.sleep(3.0)
    shot("drucker-3-after-ok.png")
    conflicts("Drucker after Edit Joint OK on Joint35")
    inspect = execute("joint.inspect", {"joint": "Joint35"})
    print(json.dumps(inspect, indent=1)[:4000], flush=True)

call("app.quit", {"force": True}, ok=False)
app.wait(timeout=60)
