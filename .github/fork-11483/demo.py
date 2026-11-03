# Fork-only demo for tangency-labs/solidcraft#11541 (#11483): Inspect > Interference on a plate
# with 144 pins through it, Compute, through the control channel under Xvfb. Prints how long
# the window took to answer after the click and how long the check took, with screenshots of
# the dialog while it checks (when the window still draws) and of what it found.
# Usage: demo.py <solidcraft binary> <output folder> <main|pr>
import json, os, secrets, socket, subprocess, sys, time

app_bin, out, build = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(out, exist_ok=True)
port = 18862
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
        sys.exit(f"{method} failed: {reply}")
    return reply.get("result")


def execute(command, params=None):
    return call("engine.execute", {"command": command, "params": params or {}})


def handles(prefix):
    r = call("ui.handles", {"prefix": prefix})
    return r.get("handles", r) if isinstance(r, dict) else r


def shot(name):
    time.sleep(1.5)
    path = os.path.join(out, name)
    r = call("ui.screenshot", {"path": path}, ok=False)
    if not (r is not None and os.path.exists(path)):
        subprocess.run(["import", "-display", ":99", "-window", "root", path], check=False)
    print("shot", name, flush=True)




def cancel_shown():
    return any(h["name"].startswith("dialog:button:") and h["name"].endswith(":Cancel") for h in handles("dialog:"))


call("ui.resize", {"width": 1600, "height": 1000}, ok=False)
call("ui.home", {"open": False}, ok=False)
time.sleep(1.0)
execute("solid.box", {"length": 240, "width": 240, "height": 10, "corner": [0, 0, 0]})
for i in range(12):
    for j in range(12):
        execute("solid.cylinder", {"base": [10 + 20 * i, 10 + 20 * j, -10], "axis": [0, 0, 1], "radius": 6, "height": 30, "operation": "new"})
print("overlapping pairs:", len(execute("inspect.clearance", {"clearance": 0})["overlaps"]), flush=True)
call("ui.camera", {"yaw": -0.7, "pitch": 0.55, "fit": True}, ok=False)
call("ui.start", {"command": "inspect.interference"})
time.sleep(1.5)
compute = None
for _ in range(40):
    compute = next((h for h in handles("dialog:") if h["name"].endswith(":Compute")), None)
    if compute:
        break
    time.sleep(0.25)
if not compute:
    sys.exit(f"no Compute button: {[h['name'] for h in handles('dialog:')]}")
call("ui.click", {"x": compute["x"], "y": compute["y"]})
t0 = time.monotonic()
# Poll the window for 15 s: how long each answer takes while it checks, and the Computing row.
shown_at, shot_taken, done_at, worst = None, False, None, 0.0
while time.monotonic() - t0 < 15:
    a = time.monotonic()
    s = cancel_shown()
    worst = max(worst, time.monotonic() - a)
    if s and shown_at is None:
        shown_at = time.monotonic()
    if s and not shot_taken and time.monotonic() - shown_at > 1.0:
        path = os.path.join(out, f"{build}-computing.png")
        call("ui.screenshot", {"path": path}, ok=False)
        shot_taken = True
        print("shot", path, flush=True)
    if shown_at is not None and not s and done_at is None:
        done_at = time.monotonic() - t0
    time.sleep(0.05)
print(f"{build}: longest wait for the window to answer while it checked: {worst:.2f} s", flush=True)
print(f"{build}: Computing with Cancel shown: {shown_at is not None}; check done after: "
      + (f"{done_at:.1f} s" if done_at is not None else "n/a (no Computing row)") + f"; app still running: {app.poll() is None}", flush=True)
shot(f"{build}-result.png")
call("ui.window", {"action": "close"}, ok=False)
