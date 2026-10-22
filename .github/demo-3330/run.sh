#!/usr/bin/env bash
# The #3330 scenario with one CLI ($1) into a folder ($2): the plate linked into an assembly with a
# pin jointed to its hole; the plate edited as a user would (a hole of the same size inserted before
# the first in its timeline, the first moved to x = 20); Update Links; pictures before and after.
set -euo pipefail
cli="$1"; out="$2"; here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$out"; cd "$out"
cp "$here"/*.json .
"$cli" run part_a.json --save plate.solidcraft > part_a.log 2>&1
"$cli" run asm.json --save asm.solidcraft --results > asm.log 2>&1
"$cli" snapshot --in asm.solidcraft --out 1-linked.png --width 960 --height 640 --view iso
"$cli" snapshot --in asm.solidcraft --out 1-linked-front.png --width 960 --height 480 --view front
"$cli" run edit.json --in plate.solidcraft --save plate.solidcraft > edit.log 2>&1
"$cli" snapshot --in plate.solidcraft --out 2-part-edited.png --width 960 --height 640 --view iso
"$cli" run upd.json --in asm.solidcraft --save asm2.solidcraft --results --allow-errors > upd.log 2>&1 || true
"$cli" snapshot --in asm2.solidcraft --out 3-after-update.png --width 960 --height 640 --view iso
"$cli" snapshot --in asm2.solidcraft --out 3-after-update-top.png --width 960 --height 640 --view top
"$cli" snapshot --in asm2.solidcraft --out 3-after-update-front.png --width 960 --height 480 --view front
grep -n '"origin"\|"warnings"\|"errors"\|"updated"\|"why"' asm.log upd.log || true
