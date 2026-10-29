"""Render the HEIFUNG://CITY icon and logo (SVG -> PNG / ICO / raw RGBA).

Three procedural candidates, each in candidates/<x>/ with the full set of
files the game uses; one of them is copied up into this folder as the live
logo and icon:

  a  Blade sign: a Hong Kong style vertical neon sign bolted to a grimy
     wall. Its glyphs are invented: an H inside an open C (HEIFUNG CITY),
     a "://" tile and a tower pictogram. The wordmark is bent tube.
  b  Terminal: the name as an address on a dirty CRT, "://" as the icon,
     scanlines, chromatic fringe and a torn line or two.
  c  Skyline: the city at night through smog and rain, one tower carrying
     a vertical "H" sign; the wordmark under it.

Everything is drawn here (no fonts besides Windows' Bahnschrift and
Consolas, no images; needs cairosvg, Pillow, numpy and scipy). Each picture is a base layer and a neon layer; the
neon layer is bloomed with Gaussian blurs and laid over the base, then
grime and grain go on top. Icons of 64 px and below come from a simpler
drawing of each candidate so they still read in the taskbar.

  python make_logo.py                 render every candidate, then wire PICK
  python make_logo.py --pick b        wire candidate b (already rendered) as the live logo
  python make_logo.py --sheet x.png   also write a contact sheet (logo, 256 and 32 icons)

Rebuild the game afterwards: the exe embeds icon.ico, icon_256.rgba and
logo_800x280.rgba.
"""
import io, os, random, shutil, sys

PICK = "a"  # the live logo and icon (a, b or c)

HERE = os.path.dirname(os.path.abspath(__file__))
FILES = ["icon.svg", "logo.svg", "icon_1024.png", "icon_256.png", "icon_128.png", "icon_64.png", "icon_32.png",
         "icon.ico", "icon_256.rgba", "logo.png", "logo_800x280.rgba"]
BAHN = "font-family:'Bahnschrift','Segoe UI',sans-serif;font-weight:700"
MONO = "font-family:'Consolas','Cascadia Mono',monospace;font-weight:700"
MONO_ADV = 0.5498  # Consolas' advance width per em


# ---------------------------------------------------------------- helpers

def svg(w, h, body, vb=None):
    vb = vb or f"0 0 {w} {h}"
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{vb}">{body}</svg>'


def poly(points, color, width, op=1.0, cap="round"):
    d = " ".join(f"{x:.1f},{y:.1f}" for x, y in points)
    return (f'<polyline points="{d}" fill="none" stroke="{color}" stroke-opacity="{op}" stroke-width="{width:.1f}" '
            f'stroke-linecap="{cap}" stroke-linejoin="round"/>')


def tube(lines, color, width, core="#fff4f0"):
    """Lit neon tube: a coloured stroke with a hot pale core."""
    return "".join(poly(p, color, width) for p in lines) + "".join(poly(p, core, width * 0.38, 0.85) for p in lines)


def dead_tube(lines, width):
    """The tube's glass and housing as seen unlit (goes in the base layer)."""
    return "".join(poly(p, "#000000", width * 1.9, 0.55) for p in lines) + "".join(poly(p, "#2a2228", width * 1.15) for p in lines)


def scaled(lines, x, y, s):
    return [[(x + px * s, y + py * s) for px, py in line] for line in lines]


def arc(cx, cy, rx, ry, a0, a1, n=10):
    import math
    return [(cx + rx * math.cos(math.radians(a0 + (a1 - a0) * k / n)), cy + ry * math.sin(math.radians(a0 + (a1 - a0) * k / n))) for k in range(n + 1)]


# A single-stroke tube alphabet (unit cap height, y down): letters a sign
# shop would bend from glass, only the ones the name needs. (Stroked
# outlines of Bahnschrift show its variable font's overlapping contours.)
TUBE_GLYPHS = {
    "H": (0.62, [[(0, 0), (0, 1)], [(0.62, 0), (0.62, 1)], [(0, 0.5), (0.62, 0.5)]]),
    "E": (0.52, [[(0.52, 0), (0, 0), (0, 1), (0.52, 1)], [(0, 0.5), (0.42, 0.5)]]),
    "I": (0.0, [[(0, 0), (0, 1)]]),
    "F": (0.5, [[(0.5, 0), (0, 0), (0, 1)], [(0, 0.5), (0.4, 0.5)]]),
    "U": (0.62, [[(0, 0)] + arc(0.31, 0.69, 0.31, 0.31, 180, 0) + [(0.62, 0)]]),
    "N": (0.62, [[(0, 1), (0, 0), (0.62, 1), (0.62, 0)]]),
    "G": (0.62, [arc(0.31, 0.3, 0.31, 0.3, -25, -180) + arc(0.31, 0.7, 0.31, 0.3, 180, 0) + [(0.62, 0.52), (0.36, 0.52)]]),
    "C": (0.6, [arc(0.31, 0.3, 0.31, 0.3, -25, -180) + arc(0.31, 0.7, 0.31, 0.3, 180, 25)]),
    "T": (0.6, [[(0, 0), (0.6, 0)], [(0.3, 0), (0.3, 1)]]),
    "Y": (0.62, [[(0, 0), (0.31, 0.5), (0.62, 0)], [(0.31, 0.5), (0.31, 1)]]),
    ":": (0.0, [[(0, 0.3), (0, 0.32)], [(0, 0.8), (0, 0.82)]]),
    "/": (0.36, [[(0, 1), (0.36, 0)]]),
}


def tube_word(text, x, y, height, gap=0.24, anchor="middle"):
    """The polylines spelling `text` in tube letters, baseline at y."""
    adv = [TUBE_GLYPHS[ch][0] + gap for ch in text]
    total = (sum(adv) - gap) * height
    x0 = x - total / 2 if anchor == "middle" else x
    out, cur = [], x0
    for ch, a in zip(text, adv):
        out += scaled(TUBE_GLYPHS[ch][1], cur, y - height, height)
        cur += a * height
    return out


def text_width(text, size):
    """Bahnschrift Bold's advance width for `text` (Windows' font file)."""
    from PIL import ImageFont
    f = ImageFont.truetype(os.path.join(os.environ.get("WINDIR", "C:/Windows"), "Fonts", "bahnschrift.ttf"), size)
    f.set_variation_by_name("Bold")
    return f.getlength(text)


def towers(x0, x1, base, rng, scale=1.0, tall_mid=True):
    """A skyline: tower rectangles with set-backs and masts, and the lit
    windows (x, y, colour, size) to draw in the neon layer."""
    parts, wins = [], []
    x = x0
    while x < x1:
        w = rng.choice([22, 28, 34, 40, 18, 46]) * scale
        h = rng.choice([90, 120, 150, 180, 210, 250, 300, 140, 110]) * scale
        if tall_mid and x0 + (x1 - x0) * 0.4 < x < x0 + (x1 - x0) * 0.6:
            h *= 1.3
        top = base - h
        parts.append(f'<rect x="{x:.1f}" y="{top:.1f}" width="{w:.1f}" height="{h + 2:.1f}"/>')
        if rng.random() < 0.35:
            sw = w * 0.6
            parts.append(f'<rect x="{x + (w - sw) / 2:.1f}" y="{top - 22 * scale:.1f}" width="{sw:.1f}" height="{24 * scale:.1f}"/>')
            top -= 22 * scale
        if rng.random() < 0.25:
            parts.append(f'<rect x="{x + w / 2 - 1.2 * scale:.1f}" y="{top - 36 * scale:.1f}" width="{2.4 * scale:.1f}" height="{36 * scale:.1f}"/>')
            wins.append((x + w / 2, top - 36 * scale, "#ff2a4d", 3.0 * scale))
        for wy in range(int(top + 12 * scale), int(base - 8 * scale), max(1, int(11 * scale))):
            for wx in (x + 5 * scale, x + w / 2, x + w - 7 * scale):
                if rng.random() < 0.11:
                    wins.append((wx, wy, rng.choice(["#ffc96b", "#ffc96b", "#7af0ff", "#ff6fae", "#e8e0c8"]), 1.5 * scale))
        x += w + rng.choice([1, 3, 5]) * scale
    return "".join(parts), wins


def windows_svg(wins, op=0.8):
    return "".join(f'<rect x="{x - r:.1f}" y="{y - r * 0.8:.1f}" width="{2 * r:.1f}" height="{1.6 * r:.1f}" fill="{c}" fill-opacity="{op}"/>' for x, y, c, r in wins)


def rain(w, h, rng, n, length, op, color="#9fb4c8", slant=0.18, width=1.2):
    out = []
    for _ in range(n):
        x, y = rng.uniform(-0.1 * w, w), rng.uniform(-0.1 * h, h)
        l = length * rng.uniform(0.5, 1.3)
        out.append(f'<line x1="{x:.1f}" y1="{y:.1f}" x2="{x + l * slant:.1f}" y2="{y + l:.1f}" stroke="{color}" stroke-opacity="{op * rng.uniform(0.4, 1):.2f}" stroke-width="{width}"/>')
    return "".join(out)


def stains(w, h, rng, n, top=0.0):
    """Drips and water streaks running down a wall."""
    out = []
    for _ in range(n):
        x = rng.uniform(0, w)
        y = rng.uniform(top * h, h * 0.9)
        sw = rng.uniform(2, 14) * w / 1024
        sl = rng.uniform(0.08, 0.5) * h
        dark = rng.random() < 0.75
        out.append(f'<rect x="{x:.1f}" y="{y:.1f}" width="{sw:.1f}" height="{sl:.1f}" fill="{"#000" if dark else "#6b5a48"}" fill-opacity="{rng.uniform(0.08, 0.28):.2f}"/>')
    return "".join(out)


# ------------------------------------------------------------ compositing

def raster(markup, w, h):
    import cairosvg, numpy as np
    from PIL import Image
    png = cairosvg.svg2png(bytestring=markup.encode("utf-8"), output_width=w, output_height=h)
    return np.asarray(Image.open(io.BytesIO(png)).convert("RGBA")).astype(np.float32) / 255.0


def blur(rgb, radius):
    from scipy.ndimage import gaussian_filter
    return gaussian_filter(rgb, sigma=(radius, radius, 0), mode="constant")


def smooth_noise(w, h, cell, rng):
    import numpy as np
    from PIL import Image
    small = rng.random((h // cell + 3, w // cell + 3)).astype(np.float32)
    im = Image.fromarray(small, mode="F").resize((w + 2 * cell, h + 2 * cell), Image.BICUBIC)
    return np.asarray(im)[cell:cell + h, cell:cell + w]


def compose(base, neon, w, h, vb, seed, bloom=((0.006, 0.9), (0.02, 0.65), (0.06, 0.4)), grime=0.5, grain=0.035,
            mask=None, vignette=0.35, fringe=0, glitch=0):
    """Base layer, then the neon layer bloomed over it, grime, grain."""
    import numpy as np
    from PIL import Image
    rng = np.random.default_rng(seed)
    b = raster(svg(w, h, base, vb), w, h)
    n = raster(svg(w, h, neon, vb), w, h)
    b_rgb, b_a = b[..., :3] * b[..., 3:], b[..., 3]
    if grime:
        g = 0.6 * smooth_noise(w, h, max(8, w // 14), rng) + 0.4 * smooth_noise(w, h, max(3, w // 60), rng)
        b_rgb = b_rgb * (1.0 - grime + grime * 2.0 * g[..., None])
    n_rgb, n_a = n[..., :3] * n[..., 3:], n[..., 3]
    rgb = b_rgb * (1 - n_a[..., None]) + n_rgb
    a = n_a + b_a * (1 - n_a)
    for r, k in bloom:
        glow = blur(n_rgb, max(0.8, r * w)) * k
        rgb = rgb + glow
        a = np.maximum(a, np.clip(glow.max(axis=2), 0, 1))
    if vignette:
        ys, xs = np.mgrid[0:h, 0:w].astype(np.float32)
        d = np.sqrt(((xs - w / 2) / (w / 2)) ** 2 + ((ys - h / 2) / (h / 2)) ** 2)
        rgb = rgb * (1 - vignette * np.clip(d - 0.55, 0, 1)[..., None])
    if fringe:
        rgb[..., 0] = np.roll(rgb[..., 0], fringe, axis=1)
        rgb[..., 2] = np.roll(rgb[..., 2], -fringe, axis=1)
    for _ in range(glitch):
        y0 = int(rng.integers(0, h - 2))
        hh = int(rng.integers(2, max(3, h // 40)))
        rgb[y0:y0 + hh] = np.roll(rgb[y0:y0 + hh], int(rng.integers(-w // 40, w // 40)), axis=1)
    if grain:
        rgb = rgb + rng.normal(0, grain, (h, w, 1)).astype(np.float32) * a[..., None]
    rgb = np.clip(rgb, 0, None)
    rgb = rgb / (1 + 0.15 * np.maximum(rgb - 0.85, 0))  # soften what blows out
    if mask is not None:
        m = raster(svg(w, h, mask, vb), w, h)[..., 3]
        rgb, a = rgb * m[..., None], a * m
    out = np.zeros((h, w, 4), np.float32)
    out[..., :3] = np.clip(rgb / np.maximum(a, 1e-4)[..., None], 0, 1)
    out[..., 3] = np.clip(a, 0, 1)
    return Image.fromarray((out * 255 + 0.5).astype(np.uint8), "RGBA")


def tile_mask(m, r):
    return f'<rect x="{m}" y="{m}" width="{1024 - 2 * m}" height="{1024 - 2 * m}" rx="{r}" fill="#fff"/>'


# --------------------------------------------- candidate a: the blade sign

# The invented glyphs, in a unit box: polylines a tube bender could make.
GLYPH_HC = [  # an H inside an open C: HEIFUNG CITY
    [(0.92, 0.28), (0.92, 0.1), (0.1, 0.1), (0.1, 0.9), (0.92, 0.9), (0.92, 0.72)],
    [(0.34, 0.27), (0.34, 0.73)], [(0.68, 0.27), (0.68, 0.73)], [(0.34, 0.5), (0.68, 0.5)],
]
GLYPH_SEP = [  # "://" as a sign tile
    [(0.2, 0.3), (0.2, 0.32)], [(0.2, 0.68), (0.2, 0.7)],
    [(0.42, 0.88), (0.62, 0.12)], [(0.68, 0.88), (0.88, 0.12)],
]
GLYPH_TOWERS = [  # three towers on a street line
    [(0.08, 0.9), (0.92, 0.9)],
    [(0.18, 0.9), (0.18, 0.5), (0.36, 0.5), (0.36, 0.9)],
    [(0.42, 0.9), (0.42, 0.14), (0.58, 0.14), (0.58, 0.9)], [(0.5, 0.14), (0.5, 0.04)],
    [(0.64, 0.9), (0.64, 0.36), (0.82, 0.36), (0.82, 0.9)],
]
RED, CYAN, AMBER, ROSE = "#ff2a4d", "#2ee6ff", "#ffae2e", "#ff4fa0"


def wall_base(w, h, rng, color="#140f10"):
    seams = "".join(f'<line x1="0" y1="{y}" x2="{w}" y2="{y}" stroke="#000" stroke-opacity="0.45" stroke-width="{w / 400:.1f}"/>'
                    for y in range(int(h * 0.13), h, int(h * 0.26)))
    seams += "".join(f'<line x1="{x}" y1="0" x2="{x}" y2="{h}" stroke="#000" stroke-opacity="0.35" stroke-width="{w / 500:.1f}"/>'
                     for x in range(int(w * 0.21), w, int(w * 0.37)))
    return f'<rect width="{w}" height="{h}" fill="{color}"/>' + seams + stains(w, h, rng, 60)


def sign_box(x, y, w, h, s):
    """The sign's sheet-metal box with its edge and bolts."""
    bolts = "".join(f'<circle cx="{bx:.1f}" cy="{by:.1f}" r="{4 * s:.1f}" fill="#4a4550"/>'
                    for bx in (x + 12 * s, x + w - 12 * s) for by in (y + 12 * s, y + h - 12 * s))
    return (f'<rect x="{x + 8 * s:.1f}" y="{y + 10 * s:.1f}" width="{w:.1f}" height="{h:.1f}" fill="#000" fill-opacity="0.55"/>'
            f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" fill="#0b090d" stroke="#2b2630" stroke-width="{5 * s:.1f}"/>'
            + bolts)


def a_icon():
    rng = random.Random(41)
    base = wall_base(1024, 1024, rng)
    # brackets from the wall and the box
    base += '<rect x="96" y="150" width="70" height="24" fill="#26222a"/><rect x="96" y="850" width="70" height="24" fill="#26222a"/>'
    base += sign_box(150, 70, 724, 884, 1.6)
    base += stains(1024, 1024, rng, 40, top=0.1)  # grime on the box too
    hc, sep = scaled(GLYPH_HC, 232, 130, 560), scaled(GLYPH_SEP, 393, 700, 220)
    # the border with a dead stretch, as on every real sign
    border = [[(838, 520), (838, 106), (186, 106), (186, 918), (838, 918), (838, 700)]]
    base += dead_tube(hc, 46) + dead_tube(sep, 34) + dead_tube(border + [[(838, 700), (838, 520)]], 14)
    neon = tube(hc, RED, 46) + tube(sep, CYAN, 34, "#e8fdff") + tube(border, AMBER, 14, "#fff1d0")
    base += rain(1024, 1024, rng, 70, 60, 0.12)
    return base, neon


def a_small():
    hc = scaled(GLYPH_HC, 150, 150, 724)
    border = [[(70, 70), (954, 70), (954, 954), (70, 954), (70, 70)]]
    base = '<rect width="1024" height="1024" fill="#120d0f"/>' + dead_tube(hc, 120)
    neon = tube(hc, RED, 120) + tube(border, AMBER, 36, "#fff1d0")
    return base, neon


def a_logo():
    rng = random.Random(42)
    W, H = 1600, 560
    base = wall_base(W, H, rng, "#120d0f")
    # a cable run along the wall
    base += poly([(0, 40), (300, 70), (700, 52), (1100, 82), (1600, 58)], "#050405", 6)
    sx, sy, sw, sh = 214, 44, 176, 472
    base += f'<rect x="{sx - 40}" y="{sy + 40}" width="40" height="14" fill="#26222a"/><rect x="{sx - 40}" y="{sy + sh - 54}" width="40" height="14" fill="#26222a"/>'
    base += sign_box(sx, sy, sw, sh, 1.0)
    g1, g2, g3 = scaled(GLYPH_HC, sx + 23, sy + 22, 130), scaled(GLYPH_SEP, sx + 33, sy + 176, 110), scaled(GLYPH_TOWERS, sx + 23, sy + 312, 130)
    base += dead_tube(g1, 13) + dead_tube(g2, 11) + dead_tube(g3, 10)
    neon = tube(g1, RED, 13) + tube(g2, CYAN, 11, "#e8fdff") + tube(g3, AMBER, 10, "#fff1d0")
    # the wordmark: two lines of bent tube
    cx = 1000
    top = tube_word("HEIFUNG", cx, 286, 168, gap=0.3)
    sep, city = tube_word("://", cx - 214, 448, 104, gap=0.32), tube_word("CITY", cx + 112, 448, 104, gap=0.32)
    base += dead_tube(top, 15) + dead_tube(sep + city, 11)
    neon += tube(top, ROSE, 15, "#ffe6f0") + tube(sep, CYAN, 11, "#e8fdff") + tube(city, AMBER, 11, "#fff1d0")
    base += rain(W, H, rng, 120, 50, 0.1)
    return base, neon


# ------------------------------------------------ candidate b: the terminal

PHOS, MAG, WARM = "#3cf2ff", "#ff2a6d", "#ffb347"


def scanlines(w, h, step, op):
    return "".join(f'<rect x="0" y="{y}" width="{w}" height="{step / 2:.1f}" fill="#000" fill-opacity="{op}"/>' for y in range(0, h, step))


def junk_rows(x, y0, y1, size, rng, cols, op):
    """Dim log rows: invented node names and hex, unreadable at a glance."""
    words = ["NODE", "LINK", "GRID", "SECTOR", "ACK", "RTE", "SIG", "HOLD", "SYNC", "ZONE", "PWR", "TX", "RX"]
    out = []
    y = y0
    while y < y1:
        row = " ".join(rng.choice([rng.choice(words), f"{rng.randrange(16**4):04X}", f"{rng.randrange(100):02d}", "--", "OK"]) for _ in range(cols))
        out.append(f'<text x="{x}" y="{y:.1f}" style="{MONO};font-size:{size}px" fill="{PHOS}" fill-opacity="{op}">{row}</text>')
        y += size * 1.35
    return "".join(out)


def b_icon():
    rng = random.Random(51)
    base = '<rect width="1024" height="1024" fill="#05090b"/>'
    base += junk_rows(70, 120, 1000, 30, rng, 6, 0.10)
    base += '<rect x="70" y="84" width="884" height="96" rx="22" fill="#071116" stroke="#1d4652" stroke-width="5"/>'
    base += f'<text x="112" y="150" style="{MONO};font-size:46px" fill="#4f8796">heifung</text>'
    base += scanlines(1024, 1024, 8, 0.35)
    size = 600
    x = 512 - 3 * size * MONO_ADV / 2
    neon = f'<text x="{x:.1f}" y="770" style="{MONO};font-size:{size}px;letter-spacing:-30px" fill="{PHOS}">://</text>'
    neon += f'<rect x="300" y="826" width="210" height="44" fill="{WARM}"/>'
    return base, neon


def b_small():
    base = '<rect width="1024" height="1024" fill="#05090b"/>' + scanlines(1024, 1024, 16, 0.3)
    size = 880
    x = 512 - 3 * size * MONO_ADV / 2 + 30
    neon = f'<text x="{x:.1f}" y="830" style="{MONO};font-size:{size}px;letter-spacing:-60px" fill="{PHOS}">://</text>'
    neon += f'<rect x="40" y="40" width="944" height="944" rx="150" fill="none" stroke="{MAG}" stroke-width="34"/>'
    return base, neon


def b_logo():
    rng = random.Random(52)
    W, H = 1600, 560
    base = f'<rect width="{W}" height="{H}" fill="#04080a"/>'
    base += junk_rows(150, 70, 540, 22, rng, 14, 0.09)
    size = 128
    adv = size * MONO_ADV
    text_w = 14 * adv
    x0 = W / 2 - (text_w + adv * 0.7) / 2
    bx0, bx1, by0, by1 = x0 - 56, x0 + text_w + adv * 0.7 + 50, 196, 356
    base += f'<rect x="{bx0:.1f}" y="{by0}" width="{bx1 - bx0:.1f}" height="{by1 - by0}" rx="26" fill="#061015" fill-opacity="0.92"/>'
    base += f'<text x="{bx0 + 8:.1f}" y="{by0 - 22}" style="{MONO};font-size:24px" fill="#4f8796">&gt; open heifung://city --night</text>'
    base += f'<text x="{bx1 - 8:.1f}" y="{by1 + 46}" text-anchor="end" style="{MONO};font-size:24px" fill="#4f8796">[ 200 OK ]  SECTOR 07  23:47</text>'
    base += scanlines(W, H, 6, 0.38)
    neon = f'<rect x="{bx0:.1f}" y="{by0}" width="{bx1 - bx0:.1f}" height="{by1 - by0}" rx="26" fill="none" stroke="{PHOS}" stroke-opacity="0.75" stroke-width="3"/>'
    y = 322
    for part, col, off in (("HEIFUNG", "#e9fcff", 0), ("://", MAG, 7), ("CITY", PHOS, 10)):
        neon += f'<text x="{x0 + off * adv:.1f}" y="{y}" style="{MONO};font-size:{size}px" fill="{col}">{part}</text>'
    neon += f'<rect x="{x0 + 14 * adv + 12:.1f}" y="{y - size * 0.68:.1f}" width="{adv * 0.62:.1f}" height="{size * 0.8:.1f}" fill="{WARM}"/>'
    # a progress rule under the bar
    neon += "".join(f'<rect x="{bx0 + 6 + i * 26:.1f}" y="{by1 + 22}" width="18" height="6" fill="{PHOS}" fill-opacity="{0.8 if i < 17 else 0.18}"/>' for i in range(24))
    return base, neon


# ------------------------------------------------- candidate c: the skyline

def c_sky(w, h, horizon):
    return (f'<defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="#04040a"/><stop offset="0.5" stop-color="#0d0a1c"/>'
            f'<stop offset="{horizon / h * 0.97:.3f}" stop-color="#3b1236"/><stop offset="{horizon / h:.3f}" stop-color="#5a1a3c"/>'
            f'<stop offset="1" stop-color="#05040a"/></linearGradient>'
            f'<radialGradient id="smog" cx="0.5" cy="1" r="0.75"><stop offset="0" stop-color="#ff5a7a" stop-opacity="0.45"/>'
            f'<stop offset="0.5" stop-color="#7a1a5a" stop-opacity="0.18"/><stop offset="1" stop-color="#000" stop-opacity="0"/></radialGradient></defs>'
            f'<rect width="{w}" height="{h}" fill="url(#sky)"/>'
            f'<rect x="0" y="{horizon - h * 0.6:.1f}" width="{w}" height="{h * 0.6:.1f}" fill="url(#smog)"/>')


def h_blade(x, y, bw, bh, s, tw=0.13):
    """The vertical sign on the tall tower: an H."""
    box = f'<rect x="{x:.1f}" y="{y:.1f}" width="{bw:.1f}" height="{bh:.1f}" fill="#08070b" stroke="#2b2630" stroke-width="{3 * s:.1f}"/>'
    m = bw * 0.24
    lines = [[(x + m, y + bh * 0.12), (x + m, y + bh * 0.88)], [(x + bw - m, y + bh * 0.12), (x + bw - m, y + bh * 0.88)], [(x + m, y + bh * 0.5), (x + bw - m, y + bh * 0.5)]]
    return box + dead_tube(lines, bw * tw), tube(lines, ROSE, bw * tw, "#ffe6f0")


def reflection(w, top, h, rng, cols, n):
    out = []
    for _ in range(n):
        x = rng.uniform(0, w)
        out.append(f'<rect x="{x:.1f}" y="{top + rng.uniform(4, 30):.1f}" width="{rng.uniform(1.5, 5):.1f}" height="{rng.uniform(0.2, 0.8) * (h - top):.1f}" fill="{rng.choice(cols)}" fill-opacity="{rng.uniform(0.08, 0.3):.2f}"/>')
    return "".join(out)


def c_icon():
    rng = random.Random(61)
    horizon = 780
    base = c_sky(1024, 1024, horizon)
    t_back, w_back = towers(-20, 1040, horizon, random.Random(5), 1.2)
    t_front, w_front = towers(-40, 1060, horizon + 6, random.Random(9), 1.6, tall_mid=False)
    base += f'<g fill="#120c1c">{t_back}</g><g fill="#050409">{t_front}</g>'
    # the tall tower and its sign
    base += '<rect x="420" y="150" width="184" height="640" fill="#040307"/><rect x="452" y="96" width="120" height="60" fill="#040307"/><rect x="509" y="20" width="6" height="80" fill="#040307"/>'
    sb, sn = h_blade(470, 210, 84, 300, 1.4)
    base += sb
    base += f'<rect x="0" y="{horizon}" width="1024" height="{1024 - horizon}" fill="#06050b"/>'
    base += reflection(1024, horizon, 1024, rng, [ROSE, "#ffc96b", CYAN], 70)
    base += rain(1024, 1024, rng, 160, 70, 0.16)
    neon = windows_svg(w_back, 0.5) + windows_svg(w_front, 0.85) + sn
    neon += f'<rect x="508" y="14" width="8" height="8" fill="{RED}"/>'
    neon += poly([(0, horizon), (1024, horizon)], "#ff4f7a", 3, 0.6)
    return base, neon


def c_small():
    horizon = 800
    base = c_sky(1024, 1024, horizon)
    base += '<g fill="#050409"><rect x="60" y="560" width="150" height="260"/><rect x="230" y="430" width="140" height="390"/><rect x="654" y="380" width="150" height="440"/><rect x="820" y="520" width="150" height="300"/></g>'
    base += '<rect x="330" y="110" width="364" height="720" fill="#040307"/><rect x="507" y="20" width="10" height="100" fill="#040307"/>'
    sb, sn = h_blade(352, 150, 320, 640, 4, tw=0.2)
    base += sb + f'<rect x="0" y="{horizon}" width="1024" height="224" fill="#06050b"/>'
    neon = sn + poly([(0, horizon), (1024, horizon)], "#ff4f7a", 12, 0.7)
    return base, neon


def c_logo():
    rng = random.Random(62)
    W, H = 1600, 560
    horizon = 330
    base = c_sky(W, H, horizon)
    t_back, w_back = towers(-20, 1620, horizon, random.Random(15), 0.8)
    t_front, w_front = towers(-30, 1630, horizon + 4, random.Random(19), 0.95, tall_mid=False)
    base += f'<g fill="#120c1c">{t_back}</g><g fill="#050409">{t_front}</g>'
    base += '<rect x="752" y="20" width="96" height="320" fill="#040307"/><rect x="798" y="0" width="4" height="24" fill="#040307"/>'
    sb, sn = h_blade(776, 54, 48, 170, 1)
    base += sb
    # rooftop billboards
    bills = [(150, 190, 120, 44, CYAN), (1290, 170, 140, 50, AMBER), (470, 214, 90, 36, RED)]
    neon = windows_svg(w_back, 0.45) + windows_svg(w_front, 0.8) + sn
    for x, y, bw, bh, col in bills:
        base += f'<rect x="{x}" y="{y}" width="{bw}" height="{bh}" fill="#07060a"/><rect x="{x + bw * 0.2}" y="{y + bh}" width="4" height="{horizon - y - bh}" fill="#07060a"/><rect x="{x + bw * 0.8}" y="{y + bh}" width="4" height="{horizon - y - bh}" fill="#07060a"/>'
        neon += f'<rect x="{x + 4}" y="{y + 4}" width="{bw - 8}" height="{bh - 8}" fill="none" stroke="{col}" stroke-width="3"/>'
        neon += "".join(f'<rect x="{x + 12 + i * (bw - 24) / 5:.1f}" y="{y + bh / 2 - 3:.1f}" width="{(bw - 24) / 7:.1f}" height="6" fill="{col}" fill-opacity="0.8"/>' for i in range(5))
    neon += poly([(0, horizon), (W, horizon)], "#ff4f7a", 2.5, 0.6)
    base += f'<rect x="0" y="{horizon}" width="{W}" height="{H - horizon}" fill="#06050b"/>'
    base += reflection(W, horizon, H, rng, [ROSE, "#ffc96b", CYAN], 110)
    base += rain(W, H, rng, 260, 46, 0.13)
    # the wordmark, filled, with a tube edge; a dark band so it reads over the reflections
    base += ('<defs><linearGradient id="band" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#05040a" stop-opacity="0"/>'
             '<stop offset="0.3" stop-color="#05040a" stop-opacity="0.85"/><stop offset="1" stop-color="#05040a" stop-opacity="0.9"/></linearGradient></defs>')
    base += f'<rect x="0" y="{horizon + 30}" width="{W}" height="{H - horizon - 30}" fill="url(#band)"/>'
    # (one text per colour, placed by measured widths: cairosvg misplaces
    # centred tspans)
    size, sp = 122, 12
    parts = [("HEIFUNG", "#f5ecff"), ("://", CYAN), ("CITY", "#f5ecff")]
    widths = [text_width(t, size) + sp * len(t) for t, _ in parts]
    x = 800 - (sum(widths) - sp) / 2
    for (t, col), w in zip(parts, widths):
        neon += f'<text x="{x:.1f}" y="490" style="{BAHN};font-size:{size}px;letter-spacing:{sp}px" fill="{col}">{t}</text>'
        x += w
    x0 = 800 - (sum(widths) - sp) / 2
    neon += poly([(x0, 516), (x0 + sum(widths) - sp, 516)], ROSE, 4)
    return base, neon


CANDIDATES = {
    "a": dict(icon=a_icon, small=a_small, logo=a_logo, icon_mask=(24, 150), small_mask=(0, 120), fringe=0, glitch=0),
    "b": dict(icon=b_icon, small=b_small, logo=b_logo, icon_mask=(24, 190), small_mask=(0, 150), fringe=3, glitch=5),
    "c": dict(icon=c_icon, small=c_small, logo=c_logo, icon_mask=(24, 190), small_mask=(0, 150), fringe=0, glitch=0),
}


# ------------------------------------------------------------- the files

def render(key, out):
    import numpy as np
    from PIL import Image
    c = CANDIDATES[key]
    os.makedirs(out, exist_ok=True)
    ib, ine = c["icon"]()
    mask = tile_mask(*c["icon_mask"])
    big = compose(ib, ine, 1024, 1024, "0 0 1024 1024", 1, mask=mask, fringe=c["fringe"], glitch=c["glitch"])
    sb, sn = c["small"]()
    small = compose(sb, sn, 256, 256, "0 0 1024 1024", 2, bloom=((0.012, 0.8), (0.04, 0.5)), grime=0.25, grain=0.0,
                    mask=tile_mask(*c["small_mask"]), vignette=0.2)
    lb, ln = c["logo"]()
    logo = compose(lb, ln, 1600, 560, "0 0 1600 560", 3, vignette=0.45, fringe=min(c["fringe"], 2), glitch=c["glitch"])
    # The SVGs: base and neon together, before bloom and grain.
    open(os.path.join(out, "icon.svg"), "w", encoding="utf-8").write(svg(1024, 1024, f'<clipPath id="tile">{mask}</clipPath><g clip-path="url(#tile)">{ib}{ine}</g>'))
    open(os.path.join(out, "logo.svg"), "w", encoding="utf-8").write(svg(1600, 560, lb + ln))
    big.save(os.path.join(out, "icon_1024.png"))
    logo.save(os.path.join(out, "logo.png"))
    i256, i128 = big.resize((256, 256), Image.LANCZOS), big.resize((128, 128), Image.LANCZOS)
    i256.save(os.path.join(out, "icon_256.png"))
    i128.save(os.path.join(out, "icon_128.png"))
    # 64 and below come from the simplified small drawing, so they read.
    smalls = {s: small.resize((s, s), Image.LANCZOS) for s in (64, 48, 32, 16)}
    smalls[64].save(os.path.join(out, "icon_64.png"))
    smalls[32].save(os.path.join(out, "icon_32.png"))
    i256.save(os.path.join(out, "icon.ico"), sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)],
              append_images=[i128] + [smalls[s] for s in (64, 48, 32, 16)])
    # Raw RGBA for the game (no image decoder needed there): the window
    # icon, and the title logo with its edges faded out so it sits on any backdrop.
    open(os.path.join(out, "icon_256.rgba"), "wb").write(i256.tobytes())
    arr = np.array(logo.resize((800, 280), Image.LANCZOS)).astype(np.float32)
    ys, xs = np.mgrid[0:280, 0:800]
    fx = np.clip(np.minimum(xs, 799 - xs) / 90.0, 0, 1)
    fy = np.clip(np.minimum(ys, 279 - ys) / 40.0, 0, 1)
    arr[..., 3] *= (fx * fy) ** 0.8
    open(os.path.join(out, "logo_800x280.rgba"), "wb").write(arr.astype(np.uint8).tobytes())
    print(f"{key}: ok")


def wire(key):
    src = os.path.join(HERE, "candidates", key)
    for f in FILES:
        shutil.copyfile(os.path.join(src, f), os.path.join(HERE, f))
    print(f"live logo and icon: candidate {key}")


def sheet(path, live):
    """Each candidate: the title logo as the game shows it, the 256 icon, the
    32 icon at 1x and blown up 4x, and 32/16/24 on a light taskbar."""
    import numpy as np
    from PIL import Image, ImageDraw
    rows = sorted(CANDIDATES)
    W, RH = 1500, 330
    img = Image.new("RGBA", (W, RH * len(rows) + 20), (18, 18, 24, 255))
    d = ImageDraw.Draw(img)
    for i, k in enumerate(rows):
        src = os.path.join(HERE, "candidates", k)
        y = 20 + i * RH
        raw = np.frombuffer(open(os.path.join(src, "logo_800x280.rgba"), "rb").read(), np.uint8).reshape(280, 800, 4)
        title_bg = Image.new("RGBA", (800, 280), (10, 9, 14, 255))
        title_bg.alpha_composite(Image.fromarray(raw.copy(), "RGBA"))
        img.alpha_composite(title_bg, (20, y))
        img.alpha_composite(Image.open(os.path.join(src, "icon_256.png")).convert("RGBA"), (840, y + 12))
        i32 = Image.open(os.path.join(src, "icon_32.png")).convert("RGBA")
        img.alpha_composite(i32, (1120, y + 12))
        img.alpha_composite(i32.resize((128, 128), Image.NEAREST), (1170, y + 12))
        light = Image.new("RGBA", (180, 60), (225, 225, 230, 255))
        light.alpha_composite(i32, (14, 14))
        light.alpha_composite(Image.open(os.path.join(src, "icon_64.png")).convert("RGBA").resize((16, 16), Image.LANCZOS), (70, 22))
        light.alpha_composite(i32.resize((24, 24), Image.LANCZOS), (110, 18))
        img.alpha_composite(light, (1120, y + 160))
        d.text((24, y + 290), f"candidate {k}" + (" (live)" if k == live else ""), fill=(230, 230, 240, 255))
        d.text((844, y + 290), "icon 256", fill=(160, 160, 170, 255))
        d.text((1120, y + 290), "icon 32 (1x, 4x); 32/16/24 on light", fill=(160, 160, 170, 255))
    img.convert("RGB").save(path)
    print(f"sheet: {path}")


if __name__ == "__main__":
    args = sys.argv[1:]
    pick = args[args.index("--pick") + 1] if "--pick" in args else None
    sheet_path = args[args.index("--sheet") + 1] if "--sheet" in args else None
    if pick:
        wire(pick)
    else:
        for k in CANDIDATES:
            render(k, os.path.join(HERE, "candidates", k))
        wire(PICK)
    if sheet_path:
        sheet(sheet_path, pick or PICK)
