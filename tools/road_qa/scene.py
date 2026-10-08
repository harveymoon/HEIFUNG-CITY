"""Lays the road QA test scene (junctions of every shape, ramps, an overpass,
a tunnel, a path and a level crossing) through the remote API on a flat new
game: python scene.py (port in nd.py). Same scene as the street_scene example."""
import sys, math; sys.path.insert(0,'.')
from nd import *
def road(pts, f=1, b=1, **kw):
    r = cmd(cmd="add_road", points=pts, lanes_fwd=f, lanes_bwd=b, implement=True, **kw)
    if not r.get("ok") or not r.get("implemented", True):
        print("FAIL", pts, r.get("error"), r.get("implement_refused"))
    return r
cmd(cmd="economy", treasury=1e9)
r = cmd(cmd="cheat", money=1e9); 
# S1 crossroads avenue x street
road([[-750,-550],[-450,-550]], 2, 2)
road([[-600,-700],[-600,-400]])
# S2 T
road([[-400,-550],[-200,-550]])
road([[-300,-550],[-300,-700]])
# S3 5-arm at (-50,-550)
c=(-50,-550)
for k in range(5):
    a=math.radians(90+72*k)
    road([[c[0],c[1]],[c[0]+120*math.cos(a), c[1]+120*math.sin(a)]])
# S4 shallow Y at (200,-550)
road([[120,-550],[350,-550]])
road([[200,-550],[200+130*math.cos(math.radians(25)), -550+130*math.sin(math.radians(25))]])
# S5 roundabout at (500,-550)
road([[400,-550],[600,-550]]); road([[500,-650],[500,-450]])
print(cmd(cmd="add_roundabout", center=[500,-550], radius=25, lanes=1, implement=True).get("ok"))
# S6 curved road
road([[-750,-300],[-650,-250],[-600,-150],[-650,-50]], curved=True)
# S7 overpass: street over street
road([[-450,-300],[-450,100]])
road([[-560,-100,0],[-500,-100,8],[-400,-100,8],[-340,-100,0]])
# S8 tunnel under a street
road([[-150,-250],[-150,50]])
road([[-300,-100,0],[-230,-100,-8],[-70,-100,-8],[0,-100,0]])
# S9 path crossing street, rail crossing
road([[100,-250],[100,50]])
road([[30,-150],[170,-150]], kind="path")
road([[50,-50],[150,-60]], kind="rail")
# S10 lane change avenue -> street
road([[250,-250],[350,-250]], 2, 2); road([[350,-250],[480,-250]])
# S11 highway off and on ramp (westbound at y=411)
road([[300,411],[220,385],[150,330],[150,250]], 1, 0)
road([[-50,250],[-50,330],[-120,385],[-200,411]], 1, 0)
road([[150,250],[-50,250]])
# S12 highway over a street: street crossing highway (underpass) 
road([[-400,200],[-400,600]])
# S13 shallow-angle road meeting
road([[600,-250],[800,-250]]); road([[620,-250],[800,-210]])
print(cmd(cmd="status").get("message"))
