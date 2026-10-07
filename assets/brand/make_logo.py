"""Render the District 2040 icon and logo (SVG -> PNG / ICO).

The mark: a dark rounded tile, a banded synthwave sun low on a magenta
horizon, a skyline of black towers with a few lit windows in front of
it, and a cyan roundabout ring on the street below: the city at night
from across the river. Run: python make_logo.py
"""
import random, cairosvg
from PIL import Image

random.seed(2040)

def skyline(x0, x1, base, rng, scale=1.0):
    """Towers as black rectangles (some with stepped tops and an antenna),
    plus lit window dots."""
    parts, wins = [], []
    x = x0
    while x < x1:
        w = rng.choice([22, 28, 34, 40, 18]) * scale
        h = rng.choice([90, 120, 150, 180, 210, 250, 300, 140]) * scale
        if x0 + (x1 - x0) * 0.42 < x < x0 + (x1 - x0) * 0.58:
            h *= 1.35  # a taller core in the middle
        top = base - h
        parts.append(f'<rect x="{x:.1f}" y="{top:.1f}" width="{w:.1f}" height="{h + 2:.1f}"/>')
        if rng.random() < 0.35:
            sw = w * 0.55
            parts.append(f'<rect x="{x + (w - sw) / 2:.1f}" y="{top - 26 * scale:.1f}" width="{sw:.1f}" height="{28 * scale:.1f}"/>')
            top -= 26 * scale
        if rng.random() < 0.3:
            parts.append(f'<rect x="{x + w / 2 - 1.2 * scale:.1f}" y="{top - 40 * scale:.1f}" width="{2.4 * scale:.1f}" height="{40 * scale:.1f}"/>')
            wins.append((x + w / 2, top - 40 * scale, "#ff2a6d", 3.2 * scale))
        for wy in range(int(top + 14 * scale), int(base - 10 * scale), int(14 * scale)):
            for wx in (x + 6 * scale, x + w / 2, x + w - 8 * scale):
                if rng.random() < 0.13:
                    col = rng.choice(["#ffd27a", "#7af0ff", "#ffd27a", "#ff7ad9"])
                    wins.append((wx, wy, col, 1.6 * scale))
        x += w + rng.choice([2, 4, 6]) * scale
    return "".join(parts), wins

def halo(shape, color, width, layers=((7, 0.10), (4, 0.16), (2.2, 0.28))):
    """Neon glow without filters: wider, fainter copies under the line."""
    return "".join(shape.format(stroke=color, w=width * k, op=op) for k, op in layers) + shape.format(stroke=color, w=width, op=1)

def sun(cx, cy, r):
    """A banded sun: horizontal slices cut out of the lower half."""
    bands = "".join(f'<rect x="{cx - r}" y="{cy + r * f:.1f}" width="{2 * r}" height="{r * (0.035 + 0.03 * i):.1f}" fill="#07060f"/>'
                    for i, f in enumerate([0.05, 0.22, 0.38, 0.52, 0.64, 0.74]))
    return (f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="url(#sun)"/>' + bands)

def icon_svg(size=1024):
    rng = random.Random(7)
    towers, wins = skyline(120, 904, 700, rng, 0.95)
    windows = "".join(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r:.1f}" fill="{c}"/>' for x, y, c, r in wins)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 1024 1024">
<defs>
  <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#05040b"/><stop offset="0.45" stop-color="#140a2e"/>
    <stop offset="0.7" stop-color="#3a0f4f"/><stop offset="0.82" stop-color="#7a1550"/><stop offset="1" stop-color="#05040b"/>
  </linearGradient>
  <linearGradient id="sun" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#ffd166"/><stop offset="0.5" stop-color="#ff5c8a"/><stop offset="1" stop-color="#b5179e"/>
  </linearGradient>
  <linearGradient id="ground" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#0b0716"/><stop offset="1" stop-color="#030208"/>
  </linearGradient>
  <filter id="glow" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="9" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
  <filter id="soft"><feGaussianBlur stdDeviation="2.2"/></filter>
  <clipPath id="tile"><rect x="24" y="24" width="976" height="976" rx="190"/></clipPath>
</defs>
<g clip-path="url(#tile)">
  <rect width="1024" height="1024" fill="url(#sky)"/>
  {sun(512, 470, 270)}
  <g fill="#050409">{towers}</g>
  <g filter="url(#soft)">{windows}</g>
  <g>{windows}</g>
  <rect x="0" y="698" width="1024" height="330" fill="url(#ground)"/>
  <!-- perspective street grid -->
  <g stroke="#ff2a6d" stroke-opacity="0.55" stroke-width="3">
    {"".join(f'<line x1="512" y1="700" x2="{512 + d}" y2="1024"/>' for d in (-900, -600, -330, -130, 130, 330, 600, 900))}
    {"".join(f'<line x1="0" y1="{y}" x2="1024" y2="{y}" stroke-opacity="{0.25 + 0.06 * i:.2f}"/>' for i, y in enumerate((716, 742, 780, 836, 916, 1010)))}
  </g>
  <!-- the roundabout -->
  {halo('<ellipse cx="512" cy="852" rx="210" ry="62" fill="none" stroke="{stroke}" stroke-opacity="{op}" stroke-width="{w}"/>', "#2de2e6", 14)}
  <ellipse cx="512" cy="852" rx="120" ry="34" fill="#030208" stroke="#2de2e6" stroke-opacity="0.45" stroke-width="5"/>
  {halo('<line x1="0" y1="700" x2="1024" y2="700" stroke="{stroke}" stroke-opacity="{op}" stroke-width="{w}"/>', "#ff2a6d", 5)}
</g>
{halo('<rect x="30" y="30" width="964" height="964" rx="186" fill="none" stroke="{stroke}" stroke-opacity="{op}" stroke-width="{w}"/>', "#2de2e6", 9, ((3, 0.12), (1.8, 0.25)))}
</svg>'''

def logo_svg():
    rng = random.Random(11)
    towers, wins = skyline(40, 1560, 300, rng, 0.62)
    windows = "".join(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r:.1f}" fill="{c}"/>' for x, y, c, r in wins)
    font = "font-family:'Bahnschrift','Segoe UI',sans-serif;font-weight:700"
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="560" viewBox="0 0 1600 560">
<defs>
  <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#05040b"/><stop offset="0.55" stop-color="#1a0b33"/><stop offset="1" stop-color="#5a1050"/></linearGradient>
  <linearGradient id="sun" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffd166"/><stop offset="0.5" stop-color="#ff5c8a"/><stop offset="1" stop-color="#b5179e"/></linearGradient>
  <linearGradient id="word" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffffff"/><stop offset="0.55" stop-color="#9ef6ff"/><stop offset="1" stop-color="#2de2e6"/></linearGradient>
  <filter id="glow" x="-20%" y="-50%" width="140%" height="200%"><feGaussianBlur stdDeviation="10" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
  <filter id="soft"><feGaussianBlur stdDeviation="1.6"/></filter>
</defs>
<rect width="1600" height="560" fill="#05040b"/>
<rect width="1600" height="300" fill="url(#sky)"/>
<clipPath id="above"><rect width="1600" height="300"/></clipPath>
<g clip-path="url(#above)">{sun(800, 250, 170)}</g>
<g fill="#050409">{towers}</g>
<g filter="url(#soft)">{windows}</g><g>{windows}</g>
{halo('<line x1="0" y1="300" x2="1600" y2="300" stroke="{stroke}" stroke-opacity="{op}" stroke-width="{w}"/>', "#ff2a6d", 4)}
<g stroke="#ff2a6d" stroke-opacity="0.35" stroke-width="2">{"".join(f'<line x1="800" y1="300" x2="{800 + d}" y2="560"/>' for d in (-1400, -900, -520, -240, 240, 520, 900, 1400))}{"".join(f'<line x1="0" y1="{y}" x2="1600" y2="{y}" stroke-opacity="{0.12 + 0.05 * i:.2f}"/>' for i, y in enumerate((312, 330, 356, 392, 440, 505)))}</g>
<radialGradient id="hush" cx="0.5" cy="0.5" r="0.5"><stop offset="0.55" stop-color="#05040b" stop-opacity="0.9"/><stop offset="1" stop-color="#05040b" stop-opacity="0"/></radialGradient>
<ellipse cx="800" cy="440" rx="640" ry="150" fill="url(#hush)"/>
{halo('<text x="800" y="452" text-anchor="middle" style="' + font + ';font-size:150px;letter-spacing:18px" fill="none" stroke="{stroke}" stroke-opacity="{op}" stroke-width="{w}">DISTRICT</text>', "#2de2e6", 3, ((8, 0.08), (4.5, 0.14), (2.2, 0.3)))}
<text x="800" y="452" text-anchor="middle" style="{font};font-size:150px;letter-spacing:18px" fill="url(#word)">DISTRICT</text>
{halo('<text x="800" y="530" text-anchor="middle" style="' + font + ';font-size:64px;letter-spacing:46px" fill="none" stroke="{stroke}" stroke-opacity="{op}" stroke-width="{w}">2040</text>', "#ff2a6d", 2, ((7, 0.1), (3.5, 0.2)))}
<text x="800" y="530" text-anchor="middle" style="{font};font-size:64px;letter-spacing:46px" fill="#ff5c8a">2040</text>
</svg>'''

open("icon.svg", "w", encoding="utf-8").write(icon_svg())
open("logo.svg", "w", encoding="utf-8").write(logo_svg())
cairosvg.svg2png(url="icon.svg", write_to="icon_1024.png", output_width=1024, output_height=1024)
cairosvg.svg2png(url="logo.svg", write_to="logo.png", output_width=1600, output_height=560)
big = Image.open("icon_1024.png").convert("RGBA")
for s in (256, 128, 64, 32):
    big.resize((s, s), Image.LANCZOS).save(f"icon_{s}.png")
big.save("icon.ico", sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)])
print("ok")

# Raw RGBA for the game (no image decoder needed there): the window icon,
# and the title logo with its edges faded out so it sits on any backdrop.
import numpy as np
icon = Image.open("icon_1024.png").convert("RGBA").resize((256, 256), Image.LANCZOS)
open("icon_256.rgba", "wb").write(icon.tobytes())
logo = np.array(Image.open("logo.png").convert("RGBA").resize((800, 280), Image.LANCZOS)).astype(np.float32)
ys, xs = np.mgrid[0:280, 0:800]
fx = np.clip(np.minimum(xs, 799 - xs) / 90.0, 0, 1)
fy = np.clip(np.minimum(ys, 279 - ys) / 40.0, 0, 1)
logo[..., 3] *= (fx * fy) ** 0.8
open("logo_800x280.rgba", "wb").write(logo.astype(np.uint8).tobytes())
print("raw ok")
