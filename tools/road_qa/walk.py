"""Walk-view screenshots, day and night, at the scene's junctions:
python walk.py <prefix>."""
import sys; sys.path.insert(0,'.')
from nd import *
tag=sys.argv[1]
W = {
 "w01_cross": (-635,-545, 1.5708), "w03_5arm": (-50,-585, 0.0), "w04_Y25": (150,-546, 1.5708),
 "w05_roundabout": (400,-546, 1.5708), "w11_offramp": (330,407, -1.5708), "w12_underpass": (-396,300, 0.0),
 "w09_rail": (95,-20, 3.1416), "w07_overpass": (-446,-180, 0.0),
}
cmd(cmd="graphics", quality="medium")
for hour, sfx in [(13, "day"), (22.5, "night")]:
    cmd(cmd="lighting", hour=hour)
    for name, (x, y, yaw) in W.items():
        cmd(cmd="camera", mode="walk", look_at=[x, y], yaw=yaw, pitch=-0.08)
        time.sleep(3.0)
        shot(f"screenshots/qa/{tag}_{name}_{sfx}.png")
cmd(cmd="camera", mode="orbit")
cmd(cmd="lighting", hour=13)
print("done")
