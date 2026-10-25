"""Fork-only demo for #6679: Assemble > Move on jointed parts, recorded from the desktop app
under Xvfb through its control channel. Writes PNG frames and one GIF into argv[2]."""
import json, os, secrets, socket, subprocess, sys, time

from PIL import Image

BIN, OUT = sys.argv[1], sys.argv[2]
PORT = 18879
os.makedirs(OUT, exist_ok=True)
token = secrets.token_hex(32)
env = dict(os.environ, SOLIDCRAFT_CONTROL_TOKEN=token, SOLIDCRAFT_CONTROL_DIR=os.path.join(OUT, "control"))
log = open(os.path.join(OUT, "app.log"), "wb")
app = subprocess.Popen(["xvfb-run", "-a", "-s", "-screen 0 1600x1000x24 -nolisten tcp", BIN, "--control", str(PORT)],
                       env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
deadline = time.monotonic() + 180
while True:
    try:
        conn = socket.create_connection(("127.0.0.1", PORT), timeout=120)
        break
    except OSError:
        if app.poll() is not None or time.monotonic() > deadline:
            sys.exit("the app never listened")
        time.sleep(0.3)
io = conn.makefile("rw")
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
    return reply.get("result")


def ex(command, params):
    return call("engine.execute", {"command": command, "params": params})


frames = []  # (path, ms)


def shot(ms=90):
    path = os.path.join(OUT, f"f{len(frames):03d}.png")
    call("ui.screenshot", {"path": path})
    frames.append((path, ms))


def hold(ms):
    for _ in range(3):
        shot()
    frames[-1] = (frames[-1][0], ms)


def at(**target):
    x, y = call("ui.at", target)
    return x, y


def drag(a, b, steps=50, shots=24):
    call("ui.drag", {"x0": a[0], "y0": a[1], "x1": b[0], "y1": b[1], "steps": steps, "hold_end": 20})
    for _ in range(shots):
        shot()


occ = {}


def part(name, color, make):
    ex("component.activate", {"component": "root"})
    occ[name] = ex("component.create", {"name": name, "grounded": False})["occurrence"]
    make()
    ex("component.activate", {"component": "root"})
    ex("appearance.assign", {"components": [name], "color": color})


def joint(kind, name, other, point, z, limits=None):
    side = lambda o: {"occurrence": occ[o], "point": point, "z": z}
    p = {"type": kind, "a": side("Base"), "b": side(other), "flip": True, "name": name}
    if limits:
        p["limits"] = limits
    ex("joint.create", p)


call("ui.home", {"open": False})
call("ui.resize", {"width": 1280, "height": 760})
for _ in range(10):
    call("ui.inspect")
# A small public assembly: a grounded plate, an arm on a hinge, a block on a rail (0 to 50 mm),
# and a pin on a cylindrical joint.
part("Base", "#8a9099", lambda: ex("solid.box", {"length": 160, "width": 100, "height": 10, "corner": [-20, -50, -10]}))
ex("occurrence.ground", {"occurrence": occ["Base"], "grounded": True})
part("Arm", "#e07b39", lambda: ex("solid.box", {"length": 80, "width": 14, "height": 8, "corner": [-7, -7, 0]}))
part("Block", "#3d7cc9", lambda: ex("solid.box", {"length": 20, "width": 20, "height": 16, "corner": [40, 20, 0]}))
part("Pin", "#4caf6a", lambda: ex("solid.cylinder", {"diameter": 12, "height": 30, "base": [110, -25, 0]}))
joint("revolute", "Hinge", "Arm", [0, 0, 0], [0, 0, 1])
joint("slider", "Rail", "Block", [50, 30, 0], [1, 0, 0], [[0, 50]])
joint("cylindrical", "Shaft", "Pin", [110, -25, 0], [0, 0, 1])
ex("select.clear", {})
call("ui.view", {"view": "home"})
for _ in range(5):
    call("ui.inspect")
hold(1200)

# The arm: pick its top face, Move (M) shows only the hinge's ring.
call("ui.click", dict(zip(("x", "y"), at(world=[55, 0, 8]))))
hold(600)
call("ui.key", {"key": "M"})
hold(1400)
drag(at(gizmo="rz", turn=0), at(gizmo="rz", turn=75))
hold(1600)

# The block: Move shows one arrow along its rail; the drag stops at the rail's 50 mm limit.
call("ui.key", {"key": "Escape"})
call("ui.click", dict(zip(("x", "y"), at(world=[50, 30, 16]))))
call("ui.key", {"key": "M"})
hold(1400)
drag(at(gizmo="z"), at(gizmo="z", by=[90, 0, 0]))
hold(2000)

# The pin: a cylindrical joint shows both its ring and its arrow.
call("ui.key", {"key": "Escape"})
call("ui.click", dict(zip(("x", "y"), at(world=[110, -25, 30]))))
call("ui.key", {"key": "M"})
hold(1400)
drag(at(gizmo="z"), at(gizmo="z", by=[0, 0, 18]))
hold(2000)

# The grounded plate has nothing to drag: a warning, its joints kept.
call("ui.key", {"key": "Escape"})
call("ui.click", dict(zip(("x", "y"), at(world=[130, 40, 0]))))
call("ui.key", {"key": "M"})
hold(2600)

call("ui.documents", {"action": "close", "force": True}, ok=False)
call("app.quit", {"force": True}, ok=False)

# One GIF, one shared palette; identical frames merged.
images, durations = [], []
last = None
for path, ms in frames:
    im = Image.open(path).convert("RGB")
    im = im.resize((im.width * 2 // 3, im.height * 2 // 3), Image.LANCZOS)
    if last is not None and im.tobytes() == last:
        durations[-1] += ms
        continue
    last = im.tobytes()
    images.append(im)
    durations.append(ms)
sample = Image.new("RGB", (images[0].width, images[0].height * min(6, len(images))))
for i, im in enumerate(images[:: max(1, len(images) // 6)][:6]):
    sample.paste(im, (0, i * im.height))
palette = sample.quantize(colors=255, method=Image.Quantize.MEDIANCUT)
gif = [im.quantize(palette=palette, dither=Image.Dither.NONE) for im in images]
gif[0].save(os.path.join(OUT, "assembly-move.gif"), save_all=True, append_images=gif[1:], duration=durations, loop=0, optimize=True)
print(f"{len(gif)} frames")
