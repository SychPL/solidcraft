# Fork-only (never upstream): drives the app under Xvfb through the control channel for
# tangency-labs/solidcraft#6257 and takes screenshots. LABEL=before runs main's build (no frames:
# the block is aligned to a fixed point), LABEL=after this branch's (the Align dialog, frames).
import json, os, secrets, socket, subprocess, sys, time

label = os.environ["LABEL"]
port = 18857
token = secrets.token_hex(32)
env = dict(os.environ, SOLIDCRAFT_CONTROL_TOKEN=token, SOLIDCRAFT_CONTROL_DIR=os.path.join(os.environ["RUNNER_TEMP"], "control"))
log = open(os.path.join(os.environ["RUNNER_TEMP"], "app.log"), "wb")
app = subprocess.Popen(["xvfb-run", "-n", "99", "-s", "-screen 0 1600x1000x24 -nolisten tcp", "target/ci/solidcraft", "--control", str(port)],
                       env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
deadline = time.monotonic() + 180
while True:
    try:
        sock = socket.create_connection(("127.0.0.1", port), timeout=180)
        break
    except OSError:
        if app.poll() is not None or time.monotonic() > deadline:
            sys.exit("the app never listened")
        time.sleep(0.3)
io = sock.makefile("rw", encoding="utf-8")
n = 0


def call(method, params=None, ok=True):
    global n
    n += 1
    req = {"id": n, "method": method, "params": params or {}}
    if n == 1:
        req["token"] = token
    io.write(json.dumps(req) + "\n")
    io.flush()
    reply = json.loads(io.readline())
    if ok and reply.get("ok") is not True:
        sys.exit(f"{method} {params} failed: {reply}")
    return reply


def ex(command, params, ok=True):
    return call("engine.execute", {"command": command, "params": params}, ok=ok)


def shot(name):
    time.sleep(1.5)
    path = os.path.abspath(f"shots/{label}-{name}.png")
    r = call("ui.screenshot", {"path": path}, ok=False)
    if r.get("ok") is not True or not os.path.exists(path):
        print(f"ui.screenshot failed ({r}), taking the X screen instead", flush=True)
        subprocess.run(["import", "-display", ":99", "-window", "root", path], check=True)
    print(f"shot {path}", flush=True)


def handles(prefix):
    r = call("ui.handles", {"prefix": prefix})["result"]
    return r if isinstance(r, list) else r.get("handles", [])


def click_xy(x, y):
    call("ui.move", {"x": x, "y": y})
    time.sleep(0.3)
    call("ui.click", {"x": x, "y": y})
    time.sleep(0.6)


def click_handle(name):
    for h in handles("dialog:"):
        if h.get("name") == name:
            click_xy(h["x"], h["y"])
            return True
    print(f"no handle {name}", flush=True)
    return False


def screen(world):
    r = call("ui.at", {"world": world})["result"]
    return (r[0], r[1]) if isinstance(r, list) else (r["x"], r["y"])


def bounds(name):
    m = ex("inspect.measure", {})["result"]
    for b in m.get("bodies", []):
        if b.get("name") == name:
            return [round(v, 3) for v in b["bbox"]["min"]], [round(v, 3) for v in b["bbox"]["max"]]
    return None


call("ui.resize", {"width": 1400, "height": 860})
call("ui.home", {"open": False}, ok=False)
time.sleep(1.0)
# A block floating off to the side, and a sketch circle on XY placed by parameter X.
ex("parameters.add", {"name": "X", "expression": "0 mm"})
ex("solid.box", {"length": 20, "width": 20, "height": 12, "corner": [30, 25, 20], "body_name": "Block"})
ex("sketch.create", {"plane": "XY", "name": "Target"})
ex("sketch.circle.center", {"center": ["X", 0], "radius": 12, "id": "ring"})
ex("sketch.finish", {})
call("ui.view", {"view": "iso"})
call("ui.view", {"view": "fit"})
shot("1-start")
top = [40, 35, 32]
if label == "after":
    # Align in the dialog: the block, its top face as a joint origin, the sketch circle as To.
    call("ui.start", {"command": "solid.align"})
    time.sleep(1.0)
    print("dialog handles:", [h.get("name") for h in handles("dialog:")], flush=True)
    click_xy(*screen(top))
    click_handle("dialog:From")
    click_xy(*screen(top))
    if click_handle("dialog:From Snap"):
        click_handle("dialog:From Snap:Joint Origin")
    click_handle("dialog:To")
    click_xy(*screen([12, 0, 0]))
    print("dialog picks:", json.dumps(call("ui.selection")["result"], ensure_ascii=False)[:3000], flush=True)
    shot("2-dialog")
    click_handle("dialog:OK")
    time.sleep(1.5)
    feats = json.dumps(call("ui.inspect")["result"], ensure_ascii=False)
    if bounds("Block") == ([30.0, 25.0, 20.0], [50.0, 45.0, 32.0]):
        print("the dialog did not align the block; running the command instead", flush=True)
        call("ui.key", {"key": "Escape"}, ok=False)
        ex("solid.align", {"bodies": ["Block"], "from_frame": {"face": top}, "to_frame": {"sketch": "Target", "curve": "ring"}})
else:
    # Main has no frames: the block's top face centre goes to a fixed point over the circle.
    ex("solid.align", {"bodies": ["Block"], "from": {"snap": "face_center", "at": top}, "to": [0, 0, 12]})
print("aligned, Block:", bounds("Block"), flush=True)
shot("3-aligned")
# The circle moves: with frames the block follows it.
ex("parameters.change", {"name": "X", "expression": "25 mm"})
print("X = 25 mm, Block:", bounds("Block"), flush=True)
shot("4-circle-moved")
call("app.quit", {"force": True}, ok=False)
app.wait(timeout=60)
