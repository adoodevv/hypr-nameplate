#!/usr/bin/env python3
"""Turn ASCII art into the files hypr-nameplate brands the desktop with.

Reads figlet-style art on stdin and writes:
  <outdir>/screensaver.txt   cleaned art for the terminal screensaver
  <outdir>/logo.png          the same art as a transparent PNG for the lock screen
                             (only when the art is made of block characters)

Exit status 0 = written, 3 = art is wider than --max-width (nothing written).
Standard library only, so it runs on a stock Ubuntu install.
"""
import argparse
import struct
import sys
import zlib

BLOCKS = {"█": (0.0, 1.0), "▀": (0.0, 0.5), "▄": (0.5, 1.0)}
CELL_W, CELL_H = 12, 24


def clean(text):
    lines = [line.rstrip() for line in text.splitlines()]
    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    indent = min((len(l) - len(l.lstrip()) for l in lines if l.strip()), default=0)
    return [l[indent:] for l in lines]


def write_png(path, lines, rgb):
    width = max(len(l) for l in lines) * CELL_W
    height = len(lines) * CELL_H
    on = bytes(rgb) + b"\xff"
    off = b"\x00\x00\x00\x00"
    raw = bytearray()
    for line in lines:
        cells = [BLOCKS.get(ch) for ch in line.ljust(width // CELL_W)]
        for y in range(CELL_H):
            frac = y / CELL_H
            raw.append(0)  # PNG filter type: none
            for cell in cells:
                lit = cell is not None and cell[0] <= frac < cell[1]
                raw.extend((on if lit else off) * CELL_W)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(bytes(raw), 9)))
        f.write(chunk(b"IEND", b""))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("outdir")
    ap.add_argument("--max-width", type=int, default=140, help="widest art that still fits a 1080p screensaver")
    ap.add_argument("--color", default="7aa2f7", help="logo colour as RRGGBB")
    args = ap.parse_args()

    lines = clean(sys.stdin.read())
    if not lines:
        sys.exit("no art on stdin")
    if max(len(l) for l in lines) > args.max_width:
        sys.exit(3)

    with open(f"{args.outdir}/screensaver.txt", "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")

    chars = set("".join(lines)) - {" "}
    if chars and chars <= set(BLOCKS):
        rgb = tuple(int(args.color[i:i + 2], 16) for i in (0, 2, 4))
        write_png(f"{args.outdir}/logo.png", lines, rgb)


if __name__ == "__main__":
    main()
