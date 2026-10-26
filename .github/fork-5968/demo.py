# Fork-only demo for tangency-labs/solidcraft#7047 (#5968): Edit Joint's Find Limits, driven
# through the control channel under Xvfb. A door hinged at the origin with a block in its way
# (it meets the block opening, and a turn back the other way round), then the built-in Hinge
# Assembly sample (its leaves sit end to end: nothing is met within a turn).
# Usage: demo.py <solidcraft binary> <output folder>
import json, os, secrets, socket, subprocess, sys, time

app_bin, out = sys.argv[1], sys.argv[2]
port = 18857
token = secrets.token_hex(32)
env = dict(os.environ, SOLIDCRAFT_CONTROL_TOKEN=token, SOLIDCRAFT_CONTROL_DIR=os.path.join(out, "control"))
log = open(os.path.join(out, "app.log"), "wb")
app = subprocess.Popen(["xvfb-run", "-a", "-s", "-screen 0 1600x1000x24 -nolisten tcp", app_bin, "--control", str(port)],
                       env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)

deadline = time.monotonic() + 180
while True:
    try:
        sock = socket.create_connection(("127.0.0.1", port), timeout=300)
        break
    except OSError:
        if app.poll() is not None or time.monotonic() > deadline:
            sys.exit("the app never listened")
        time.sleep(0.3)
io = sock.makefile("rw", encoding="utf-8")
n = 0


def call(method, params=None, quiet=False):
    global n
    n += 1
    req = {"id": n, "method": method, "params": params or {}}
    if n == 1:
        req["token"] = token
    io.write(json.dumps(req) + "\n")
    io.flush()
    reply = json.loads(io.readline())
    if not quiet:
        shown = {k: ("..." if k == "token" else v) for k, v in req.items()}
        print(">", json.dumps(shown), flush=True)
    if reply.get("ok") is not True:
        sys.exit(f"{method} failed: {reply}")
    return reply["result"]


def execute(command, params=None, quiet=False):
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


def click(h, double=False):
    call("ui.click", {"x": h["x"], "y": h["y"], "double": double})


def shot(name, keep=None):
    path = os.path.join(out, name)
    call("ui.screenshot", {"path": path}, quiet=True)
    if keep is not None:
        keep.append(path)
    return path


def find_limits_in_dialog(joint, prefix, frames):
    """Edit Joint on `joint`, its Limits opened, Find Limits clicked; screenshots along the way."""
    click(find("", lambda name: name in (f"timeline-joint:{joint}", f"row:{joint}")), double=True)
    time.sleep(1.0)
    time.sleep(0.5)
    # The Limits section remembers being open: open it only when its button isn't drawn.
    if not any(h["name"].endswith(":Find Limits") for h in handles("dialog:button:")):
        click(find("dialog:section:", lambda name: name == "dialog:section:Limits"))
        time.sleep(0.8)
    shot(f"{prefix}-1-before.png", frames)
    click(find("dialog:button:", lambda name: name.endswith(":Find Limits")))
    # The search runs off the window thread: a few frames of it, until the button is back.
    for k in range(300):
        time.sleep(0.4)
        if k < 4:
            shot(f"{prefix}-2-searching-{k}.png", frames)
        if any(h["name"].endswith(":Find Limits") for h in handles("dialog:button:")):
            break
    time.sleep(0.6)
    shot(f"{prefix}-3-after.png", frames)


def gif(frames, path, first=1500, middle=300, last=4000):
    try:
        from PIL import Image
        images = [Image.open(p).convert("RGB") for p in frames if os.path.exists(p)]
        scale = 1200 / images[0].width
        images = [f.resize((1200, int(f.height * scale))) for f in images]
        durations = [first] + [middle] * (len(images) - 2) + [last]
        images[0].save(path, save_all=True, append_images=images[1:], duration=durations[:len(images)], loop=0)
    except Exception as e:  # The PNGs are there either way.
        print("no GIF:", e)


# 1. A door hinged at the origin and a block in its way.
for name, corner, size in [("Door", [0, 0, 0], [100, 4, 50]), ("Block", [60, 30, 0], [20, 20, 50])]:
    execute("component.activate", {"component": "root"}, quiet=True)
    execute("component.create", {"name": name, "grounded": False}, quiet=True)
    execute("solid.box", {"length": size[0], "width": size[1], "height": size[2], "corner": corner}, quiet=True)
execute("component.activate", {"component": "root"}, quiet=True)
execute("joint.create", {"type": "revolute", "name": "Door hinge", "flip": True,
                         "a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Door", "point": [0, 0, 0]}})
execute("occurrence.ground", {"occurrence": "Block:1", "grounded": True}, quiet=True)
call("ui.camera", {"yaw": 0.5, "pitch": 0.9, "fit": True})
time.sleep(0.8)
print(json.dumps(execute("joint.find_limits", {"joint": "Door hinge"}), indent=1), flush=True)
door = []
find_limits_in_dialog("Door hinge", "door", door)
gif(door, os.path.join(out, "find-limits-door.gif"))
click(find("dialog:", lambda name: name == "dialog:OK"))
time.sleep(1.0)
hinge = next(j for j in execute("joint.list").get("joints", []) if j.get("name") == "Door hinge")
print("limits after OK:", json.dumps(hinge.get("values")), flush=True)
lo, hi = hinge["values"][0]["limits"]
camera = call("ui.camera", {}, quiet=True)
execute("motion.export_animation", {"joint": "Door hinge", "from": lo, "to": hi, "samples": 80, "fps": 20, "width": 720, "height": 450,
                                     "path": os.path.join(out, "door-range.gif"), "camera": camera})

# 2. The Hinge Assembly sample.
try:
    call("ui.home", {"sample": 2})
    for _ in range(240):
        if any(j.get("name") == "Hinge" for j in execute("joint.list", quiet=True).get("joints", [])):
            break
        time.sleep(0.5)
    time.sleep(1.0)
    call("ui.camera", {"yaw": 0.9, "pitch": 0.45, "fit": True})
    print(json.dumps(execute("joint.find_limits", {"joint": "Hinge"}), indent=1), flush=True)
    sample = []
    find_limits_in_dialog("Hinge", "hinge", sample)
    gif(sample, os.path.join(out, "find-limits-hinge.gif"))
except SystemExit as e:
    print("the hinge part stopped:", e, flush=True)

call("app.quit", {"force": True})
app.wait(timeout=60)
