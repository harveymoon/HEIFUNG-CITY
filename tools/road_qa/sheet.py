"""Contact sheet of screenshots, two per row: python sheet.py out.png in.png..."""
import sys
from PIL import Image
out = sys.argv[1]; files = sys.argv[2:]
ims=[]
for f in files:
    im = Image.open(f).convert("RGB")
    w,h = im.size
    im = im.crop((0, int(h*0.05), w, int(h*0.86)))
    im = im.resize((im.width*55//100, im.height*55//100))
    ims.append(im)
cols = 2
rows = (len(ims)+1)//2
W = ims[0].width; H = ims[0].height
S = Image.new("RGB", (W*cols, H*rows), (0,0,0))
for i,im in enumerate(ims):
    S.paste(im, ((i%cols)*W, (i//cols)*H))
S.save(out)
print(out, S.size)
