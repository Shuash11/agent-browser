"""Generate an original app icon (dark rounded tile + blue cursor ring) as .ico."""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "app.ico"


def render(size: int) -> Image.Image:
    s = size
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    r = int(s * 0.22)
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=r, fill="#1e1e1e",
                        outline="#3a3a3c", width=max(1, s // 32))
    # blue orbital cursor ring
    pad = int(s * 0.26)
    w = max(2, int(s * 0.085))
    d.ellipse([pad, pad, s - pad, s - pad], outline="#0A84FF", width=w)
    # centre dot and two ticks
    c = s // 2
    rr = max(2, int(s * 0.055))
    d.ellipse([c - rr, c - rr, c + rr, c + rr], fill="#f5f5f7")
    for ang in (0, 90, 180, 270):
        import math
        a = math.radians(ang)
        x0 = c + (s * 0.36) * math.cos(a)
        y0 = c + (s * 0.36) * math.sin(a)
        x1 = c + (s * 0.46) * math.cos(a)
        y1 = c + (s * 0.46) * math.sin(a)
        d.line([x0, y0, x1, y1], fill="#0A84FF", width=max(1, s // 48))
    return img


OUT.parent.mkdir(exist_ok=True)
sizes = [16, 24, 32, 48, 64, 128, 256]
render(sizes[-1]).save(OUT, format="ICO", sizes=[(s, s) for s in sizes])
print(f"wrote {OUT}")
