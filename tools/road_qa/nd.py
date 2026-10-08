"""Tiny client for a Neon District test instance's remote API (never 4817,
Harvey's game): set PORT to the instance's ND_REMOTE_PORT. Screenshot paths
are relative to the game's working directory."""
import json, sys, time, urllib.request, os
PORT = 4895
def cmd(**kw):
    body = json.dumps(kw).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}/cmd", data=body, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=300) as r:
            return json.loads(r.read())
    except urllib.error.HTTPError as e:
        return json.loads(e.read())

def shot(path, wait=1.5):
    """Screenshot into screenshots/<path>; waits for the file."""
    r = cmd(cmd="screenshot", path=path)
    p = r.get("path")
    t0 = time.time()
    while p and not os.path.exists(p) and time.time() - t0 < 10:
        time.sleep(0.2)
    time.sleep(0.3)
    return p

if __name__ == "__main__":
    a = json.loads(sys.argv[1])
    print(json.dumps(cmd(**a), indent=1)[:int(sys.argv[2]) if len(sys.argv) > 2 else 6000])

def cam(x, y, dist, pitch=0.8, yaw=0.7, mode="orbit"):
    return cmd(cmd="camera", look_at=[x, y], distance=dist, pitch=pitch, yaw=yaw, mode=mode)
