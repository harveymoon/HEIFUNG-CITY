# Moving the camera: mouse, laptop, keyboard, remote desktop [laptop-nav]

Everything essential works with a plain left click, a touchpad, the keyboard alone, or over a remote
desktop (RDP, Chrome Remote Desktop, Parsec, VNC). The code is `crates/nd_app/src/nav.rs` (input
gathering and the mappings, as pure functions with tests in `nav_tests.rs`), `camera.rs`
(`control_camera` applies them) and `ui/nav_pad.rs` (the on-screen pad and the Settings section).

## Controls

### Orbit view (planning)

| With | Pan | Turn and tilt | Zoom |
| --- | --- | --- | --- |
| Mouse | middle-drag, Shift+left-drag, left-drag in the Inspect tool | right-drag | wheel |
| Touchpad / laptop | two-finger scroll, left-drag (Inspect), Shift+left-drag | Alt+left-drag, rotate gesture (macOS) | pinch, Ctrl+two-finger scroll |
| Keyboard | W A S D or arrows | Q / E turn; Home / End or Shift+W / Shift+S tilt | `.` in, `,` out; also `[` `]` and Page Up / Page Down outside the Road tool |
| On-screen pad | arrows | the curved arrows turn, the corner buttons tilt | + and − |

- **Inspect tool**: a left-drag on the map pans; a click that does not move (6 px) still selects, on the
  button's release. Other tools keep the left button for themselves (Alt+drag and Shift+drag still
  move the camera there). Settings can turn the Inspect drag off.
- **The pad** sits at the bottom left of the map: pan arrows, turn (top corners), tilt (bottom corners:
  steeper / towards the horizon), zoom + and −, and a compass whose red needle points north; click it
  to face north. Click for a step, hold to keep moving. By default (Auto) it shows until a mouse wheel is
  used in that run; Settings can make it Always or Off.
- The Road tool keeps `[` `]` (and Page Up / Down while drawing) for road levels: `.` and `,` zoom then.

### Walk mode (Tab)

WASD or arrows walk, Shift runs, Q / E turn, R / F or Home / End look up and down, the pad works too.
Mouselook uses the raw mouse; with **Walk mode: drag to look** (Settings), or by itself when the raw
mouse motion is unusable (remote desktop, see below), the pointer stays free and a left- or right-drag
turns the head.

### Drive mode (Shift+Tab, or Drive in the top bar) [r5-drive]

At the wheel of a car on the street nearest the view; the game runs at 1× while driving (its speed
comes back after). Keyboard only is enough:

| Key | Does |
| --- | --- |
| W / ↑ | accelerate |
| S / ↓ | brake; once stopped, reverse |
| A D / ← → | steer |
| Space | handbrake (not pause while driving) |
| H | horn (not heat vision while driving) |
| Q / E | look left / right (eases back behind the car) |
| R / F, Home / End | look up / down |
| C (hold) | look behind |
| `.` `,` (or wheel, [ ], Page Up/Down) | camera closer / further |
| V | chase view / bonnet view |
| L | lane assist (steers along the lane while you do not) |
| Esc or Tab | back to Orbit over the car |

A right-drag or Alt+left-drag looks round, from the cursor's movement (never the raw mouse alone),
so a remote desktop works the same. The on-screen pad's arrows are the pedals and the wheel, its
turn / tilt buttons the camera, + / − the zoom, and a row of HANDBRAKE and HORN buttons shows while
driving. The HUD (bottom right) shows the speed, the limit, the street, the heading and a
heading-up map of the streets round the car.

### Following something

Right-drag or Alt+left-drag looks round, Q / E turn, Home / End tilt, wheel / pinch / `.` `,` zoom.

## Settings (Settings window, NAVIGATION; kept in `settings.ron`)

- **Two-finger scroll**: Auto (a mouse wheel zooms, a touchpad scroll pans), Zoom (every scroll zooms;
  the old behaviour), Pan (every scroll pans). Pinch and Ctrl+scroll always zoom.
- **On-screen controls**: Auto / Always / Off.
- Speeds: touchpad pan, zoom (wheel, pinch, Ctrl+scroll), drag turn. Inverts: zoom, scroll pan, drag tilt.
- Left-drag pans the map (Inspect tool); Walk mode: drag to look.
- A status line says what the last scroll was taken for and whether raw mouse motion is in use.

## Why the old controls failed on a laptop and over a remote desktop

- The camera read Bevy's `AccumulatedMouseMotion`, the raw device motion. Over RDP, VNC, Chrome Remote
  Desktop, in a VM or with a pen, Windows reports the pointer as an *absolute* device; winit 0.30's
  raw input handler checks `has_flag(usFlags, MOUSE_MOVE_RELATIVE)`, and that flag is 0, so the test is
  always true and the absolute coordinates (0..65535) arrive as "deltas". A right-drag either did
  nothing or flung the camera to its limits. Drags now follow the window's cursor position
  (`CursorMoved`), which every remote tool moves correctly; raw motion is used only when the cursor did
  not move in a frame (stuck at a screen edge) and only while it looks like a mouse's (no frame over
  1500 counts; two such frames and it is not trusted again that run).
- Windows precision touchpads send a two-finger scroll as many small *fractional* wheel steps (and
  horizontal ones), and a pinch as Ctrl + those steps; there are no pinch or rotate gestures on Windows
  (winit sends those on macOS and iOS only, with pixel scroll deltas). Every wheel event is sorted: Ctrl
  held: zoom; pixels, a fraction or a sideways step: a touchpad scroll (the gesture keeps that kind
  until 0.3 s pass without scrolling); whole notches: a mouse wheel.
- Panning needed the middle button or Shift+drag (the latter also through raw motion).

## Testing it without a touchpad

The remote API's `input` command plays synthetic window events (cursor, buttons, wheel in lines or
pixels, pinch, rotate, raw motion, keys) into the game through the same path real input takes, and
`nav` presses the pad's buttons and reads back the state; see `docs/REMOTE.md` (Navigation input).
`nav_tests.rs` does the same in a headless app: a normal mouse (right/middle drag with raw motion,
wheel notches), an RDP pointer (absolute raw motion), a Windows touchpad (fractional steps, Ctrl+scroll
pinch), a macOS touchpad (pixels, pinch, rotate), keys, the pad, and Walk mode without mouselook.

## Not covered

- Touch screens: Windows turns a one-finger drag into a left-drag (so it pans in the Inspect tool),
  but two-finger touch pinch is not read.
- Whether a Windows touchpad pinch reaches the game as Ctrl+wheel depends on the driver (precision
  touchpads do); if it does not, Ctrl+two-finger scroll, `.` / `,` and the pad's + / − zoom.
- A high-resolution mouse wheel that sends fractional steps counts as a touchpad in Auto (it pans):
  set Two-finger scroll to Zoom.
