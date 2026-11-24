"""Generate icons.css from local Lucide SVGs (mask-image technique)."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "assets" / "icons"
OUT = ROOT / "assets" / "terminal" / "icons.css"

lines = ["/* Generated from Lucide ISC icons; do not edit by hand. */",
         ".icon { display:inline-block; width:16px; height:16px; flex:none;",
         "  background-color:currentColor; -webkit-mask-repeat:no-repeat;",
         "  mask-repeat:no-repeat; -webkit-mask-position:center; mask-position:center;",
         "  -webkit-mask-size:contain; mask-size:contain; vertical-align:-3px; }"]
for svg in sorted(SRC.glob("*.svg")):
    name = svg.stem
    lines.append(
        f'.i-{name} {{ -webkit-mask-image:url("/icons/{name}.svg"); '
        f'mask-image:url("/icons/{name}.svg"); }}'
    )
OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
print(f"wrote {OUT} ({len(lines)} lines)")
