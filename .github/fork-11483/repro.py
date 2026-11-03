"""Fork-only: replay tangency-labs/solidcraft#11483's crash report with a release's solidcraft-cli.

repro.py CLI CRASH_DIR OUT_DIR
Runs three scripts on the report's design.sc: the clearance and interference checks the
Interference dialog runs, at the saved pose and after the report's joint.reach drags. Prints each
run's exit code (hex on Windows), seconds and peak memory, and keeps its output in OUT_DIR.
"""

import json
import os
import subprocess
import sys
import time

import psutil

cli, crash, out = sys.argv[1:4]
os.makedirs(out, exist_ok=True)
with open(os.path.join(crash, "replay.json"), encoding="utf-8") as f:
    replay = json.load(f)["commands"]
reach = [c for c in replay if c["command"] == "joint.reach"]
pair = ["Cut-Extrude2", "Cut-Extrude12"]
checks = [
    {"command": "inspect.clearance", "params": {"clearance": 0, "nearest": 1, "bodies": pair}},
    {"command": "inspect.interference", "params": {"bodies": pair}},
]
scripts = {
    "saved-pose": checks,
    "after-reach": reach + checks,
    "after-reach-clearance-only": reach + checks[:1],
}
for name, cmds in scripts.items():
    path = os.path.join(out, f"{name}.json")
    with open(path, "w", encoding="utf-8") as f:
        json.dump({"commands": cmds}, f)
    log = open(os.path.join(out, f"{name}.log"), "w", encoding="utf-8", errors="replace")
    t0 = time.time()
    p = subprocess.Popen(
        [cli, "run", path, "--in", os.path.join(crash, "design.sc"), "--results", "--timings"],
        stdout=log, stderr=subprocess.STDOUT,
    )
    peak = 0
    proc = psutil.Process(p.pid)
    while p.poll() is None:
        try:
            peak = max(peak, proc.memory_info().rss)
        except psutil.Error:
            pass
        if time.time() - t0 > 900:
            p.kill()
            print(f"{name}: TIMEOUT after 900 s")
            break
        time.sleep(0.1)
    code = p.wait()
    log.close()
    shown = f"0x{code & 0xFFFFFFFF:08X}" if os.name == "nt" else str(code)
    print(f"{name}: exit {shown}, {time.time() - t0:.1f} s, peak {peak / 2**20:.0f} MiB", flush=True)
    with open(os.path.join(out, f"{name}.log"), encoding="utf-8", errors="replace") as f:
        text = f.read()
    print(text[-3000:])
