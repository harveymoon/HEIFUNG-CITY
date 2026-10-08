"""Before | after pairs as one JPEG each: python pair.py outdir name:before.png:after.png ..."""
import sys, os
from PIL import Image, ImageDraw

def load(f):
    im = Image.open(f).convert("RGB")
    w, h = im.size
    if h > 1000:  # a game screenshot: drop the top bar and the tool panel
        im = im.crop((0, int(h * 0.05), w, int(h * 0.86)))
    return im

out = sys.argv[1]
for spec in sys.argv[2:]:
    name, b, a = spec.split(":", 2) if spec.count(":") == 2 else spec.rsplit(":", 2)
    ib, ia = load(b), load(a)
    W = 700
    ib = ib.resize((W, ib.height * W // ib.width)); ia = ia.resize((W, ia.height * W // ia.width))
    H = max(ib.height, ia.height)
    S = Image.new("RGB", (W * 2 + 6, H + 24), (20, 20, 20))
    S.paste(ib, (0, 24)); S.paste(ia, (W + 6, 24))
    d = ImageDraw.Draw(S)
    d.text((6, 5), "before (main)", fill=(255, 200, 120)); d.text((W + 12, 5), "after (fix/road-polish)", fill=(140, 230, 140))
    S.save(os.path.join(out, name + ".jpg"), quality=72)
    print(name)
