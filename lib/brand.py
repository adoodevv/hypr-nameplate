#!/usr/bin/env python3
"""Turn ASCII art into the screensaver's lettering.

Reads figlet-style art on stdin and writes <outdir>/screensaver.txt, trimmed.

Exit status 0 = written, 3 = art is wider than --max-width (nothing written).
Standard library only, so it runs on a stock Ubuntu install.
"""
import argparse
import sys


def clean(text):
    lines = [line.rstrip() for line in text.splitlines()]
    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    indent = min((len(l) - len(l.lstrip()) for l in lines if l.strip()), default=0)
    return [l[indent:] for l in lines]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("outdir")
    ap.add_argument("--max-width", type=int, default=140, help="widest art that still fits a 1080p screensaver")
    args = ap.parse_args()

    lines = clean(sys.stdin.read())
    if not lines:
        sys.exit("no art on stdin")
    if max(len(l) for l in lines) > args.max_width:
        sys.exit(3)

    with open(f"{args.outdir}/screensaver.txt", "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
