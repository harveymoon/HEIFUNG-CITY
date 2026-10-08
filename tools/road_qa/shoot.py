"""Orbit screenshots (close and top-down) of every spot in spots.py (or the
module named by $SPOTS): python shoot.py <prefix> [spot ...]."""
import sys; sys.path.insert(0,'.')
from nd import *
import importlib, os; SPOTS = importlib.import_module(os.environ.get("SPOTS","spots")).SPOTS
tag = sys.argv[1]  # b or a
only = sys.argv[2:] 
for name,(x,y) in SPOTS.items():
    if only and name not in only: continue
    cam(x,y,45,0.85,0.6); time.sleep(2.2); shot(f"screenshots/qa/{tag}_{name}_close.png")
    cam(x,y,110,1.2,0.0); time.sleep(2.2); shot(f"screenshots/qa/{tag}_{name}_top.png")
print("done")
