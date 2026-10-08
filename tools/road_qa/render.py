"""Top-down render of the street triangles dumped by the `street_scene`
example (nd_procgen), one PNG per spot: python render.py dump.json outdir [prefix]"""
import json, sys, os
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.collections import PolyCollection

TINT = {"Paint": (1.0, 1.0, 1.0), "Tactile": (0.9, 0.75, 0.1), "Glow": (1.0, 0.9, 0.5)}

def main():
    data = json.load(open(sys.argv[1]))
    out = sys.argv[2]
    prefix = sys.argv[3] if len(sys.argv) > 3 else ""
    os.makedirs(out, exist_ok=True)
    for spot in data:
        tris = spot["tris"]
        # Painter's order by height (mean y), then flat surfaces before props.
        tris.sort(key=lambda t: (t[2] + t[5] + t[8]) / 3.0)
        polys, cols = [], []
        for t in tris:
            s = t[0]
            pts = [(t[1], -t[3]), (t[4], -t[6]), (t[7], -t[9])]
            c = [min(1.0, max(0.0, v)) for v in t[10:13]]
            if s in TINT:
                c = [min(1.0, 0.25 + 0.75 * a * b) for a, b in zip(c, TINT[s])]
            if s.startswith("Asphalt") or s == "WornAsphalt":
                c = [v * 1.6 for v in c]
            polys.append(pts)
            cols.append([min(1.0, v) for v in c])
        fig = plt.figure(figsize=(9, 9), dpi=100)
        ax = fig.add_axes([0, 0, 1, 1])
        ax.set_facecolor((0.55, 0.47, 0.36))
        ax.add_collection(PolyCollection(polys, facecolors=cols, edgecolors="none", antialiased=False))
        x, y = spot["at"]; r = spot["r"]
        ax.set_xlim(x - r, x + r); ax.set_ylim(-y - r, -y + r)
        ax.set_aspect("equal"); ax.set_xticks([]); ax.set_yticks([])
        ax.text(x - r + 1, -y + r - 3, f"{spot['name']} ({x:.0f}, {y:.0f})", color="w", fontsize=12)
        fig.savefig(os.path.join(out, f"{prefix}{spot['name']}.png"))
        plt.close(fig)
        print("rendered", spot["name"], len(tris))

main()
