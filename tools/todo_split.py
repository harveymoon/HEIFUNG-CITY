#!/usr/bin/env python3
"""Split todo.md into open work (todo.md) and finished items (an archive file).

Re-runnable on a newer todo.md (round 13 cleanup, r13-docs). Usage, from the repo root:

    python tools/todo_split.py                 # split todo.md in place, write the archive
    python tools/todo_split.py --dry-run       # print counts only, write nothing
    python tools/todo_split.py --check ORIG    # verify ORIG against todo.md + the archive

Rules (see docs/README.md):
- An item is a line at column 0 ("- [x] ...", "[x] - ...", "- [ ] ...", "- [→] ...") plus every
  deeper-indented line under it (sub-items, "→ DONE" continuation lines).
- Open: an unchecked "[ ]" or "[~]" checkbox anywhere. Kept in todo.md with its description.
- Partial: checked, but it (or a sub-line) still has an open marker ("Left", "LEFT", "Not done",
  "still open", "PARTLY", "PART DONE") whose text is not resolved by a later "→ DONE"/"→ FIXED".
  The full item goes to the archive; todo.md keeps a short form: the request, the branch, a
  pointer to the archive and the open part (and any open sub-lines verbatim).
- Done: checked ("[x]", "[x ]", "[→]") with no open marker: moved to the archive verbatim.
- "## Agents running (update ...)" keeps the entries of the last three rounds (and anything
  newer); older entries go to the archive.
- Sections with nothing kept go to the archive whole (heading + notes). Headings agents rely on
  are always kept. Headings that themselves carry an open marker are kept.
- If the archive already exists, newly archived lines are appended (lines already in it are not
  repeated), so the split can be run again after other agents tick more items.
"""
import argparse
import re
import sys
from pathlib import Path

ARCHIVE_DEFAULT = "docs/archive/todo_done_2026-11-21.md"
POINTER = "full text in docs/archive/todo_done_2026-11-21.md"

CHECKBOX = re.compile(r"^\s*(?:[-*] |\d+\. )?\[(x|X|x |~|→| )\](?:\s|$)|^\s*\[(x|X)\] -")
OPEN_BOX = re.compile(r"\[ \]|\[~\]")
MARKER = re.compile(r"\bLeft\b(?! (?:turn|lane|arrow|arm|side|half|of|to|at|hand|from the right))|\bLEFT\b|"
                    r"\bNot done\b|\bnot done\b|\bstill open\b|\bPARTLY\b|\bPART DONE\b")
RESOLVED = re.compile(r"→\s*(?:DONE|FIXED)|\bDONE \[|^\W*(?:none|nothing)\b", re.I)

ALWAYS_KEEP_HEADINGS = ("# OPEN WORK", "## Follow-ups noted in agent reports",
                        "## Agents running (update", "## Questions for Harvey")
ARCHIVE_WHOLE_HEADINGS = ("### Agent waves", "## Where we paused", "## Agents running (playtest round",
                          "## DONE 2026-10-11", "### E. Housekeeping",
                          "## Next steps (Harvey 2026-10-08)")
AGENTS_RUNNING = "## Agents running (update"

# Docs moved to docs/archive/ in the round 13 cleanup: lines kept in todo.md get the new path
# (the archive keeps the text verbatim).
PATH_REWRITES = {
    f"docs/playtest/{n}.md": f"docs/archive/playtest/{n}.md"
    for n in ("todo_2026-10-10", "todo_2026-10-11", "todo_2026-10-11-evening", "flags_2026-10-09",
              "flags_2026-10-11", "flags_2026-10-11-evening", "transit_ridership_2026-10-10")
}
PATH_REWRITES.update({
    "docs/REVIEW.md": "docs/archive/reviews/REVIEW.md",
    "docs/REVIEW2.md": "docs/archive/reviews/REVIEW2.md",
    "docs/IDEAS.md": "docs/archive/IDEAS_2026-10.md",
    "docs/qa/road_artifacts_2026-10-07": "docs/archive/qa/road_artifacts_2026-10-07",
})
for _n in ("nd_app", "nd_procgen", "nd_sim_core", "nd_sim_world"):
    PATH_REWRITES[f"docs/review/{_n}.md"] = f"docs/archive/reviews/{_n}.md"


def rewrite(line):
    for a, b in PATH_REWRITES.items():
        line = line.replace(a, b)
    return line

# Items whose "Left:" part was finished later or is tracked by another open item (checked by
# hand 2026-11-21, r13-docs). A line containing one of these strings counts as done; its open
# sub-lines still count on their own. Add to this list instead of editing todo.md by hand.
SUPERSEDED = (
    "tow trucks break traffic rules only near the job",      # done: feat/street-followups
    "look-dev pass, grounded neon",                          # rest in "Look-dev leftovers"
    "Social layer: encounters and conversations",            # LLM dialogue done: feat/committee
    "High quality: near-camera geometry washes out",         # rest in "Look-dev leftovers"
    "Interchange leftovers: hand-drawn ramps",               # parclo in the [~] Interchanges item; pad gaps w1/render
    "Megacorp leftovers:",                                   # HQ crown w1/render; deposits in "Investors"
    "Econ leftovers:",                                       # rest in "Investors"
    "Buildings grow under raised paths and roads built",     # rest in "Layered city"
    "Odd building on a huge waterfront plot",                # rest in "Odd lots in general"
    "Big-city performance 20k–50k people → PARTLY",         # rest in "Performance plan"
    "Harvey's city gridlocked (away fans",                   # tow queue: round1/r2-traffic
    "→ [r4-brick] Brick & Steel done",                       # rest in the r4-patchwork line
    "→ [r4-oldstone] Old Stone done",
    "→ [r4-solarpunk] Solarpunk Green done",
    "More: aquarium, botanical garden",                      # "pick the best few": done
    "flag: windows z-fighting on Aurora Block",              # tail is mostly hidden faces; facades now via gallery checks
    "flag: crashed motorcycle in a one-way tunnel",          # tow trucks reworked: street-followups, round1/r2-traffic
    "flag: kerbs turn to dirt at corners",                   # rest closed by fix/review on the same line
    "Sim on its own thread (Harvey 2026-11-01",              # rest in "Performance plan" (multi-core sim)
)


def indent(line):
    return len(line) - len(line.lstrip(" "))


def is_heading(line):
    return line.startswith("#")


def marker_open(line):
    """True if the line carries an open marker not resolved later on the same line."""
    if any(x in line for x in SUPERSEDED):
        return False
    hits = list(MARKER.finditer(line))
    if not hits:
        return False
    tail = line[hits[-1].end():]
    return not RESOLVED.search(tail)


class Node:
    def __init__(self, idx, line):
        self.idx, self.line, self.children = idx, line, []

    def walk(self):
        yield self
        for c in self.children:
            yield from c.walk()

    @property
    def checkbox(self):
        m = CHECKBOX.match(self.line)
        if not m:
            return None
        return (m.group(1) or m.group(2)).strip() or " "

    @property
    def checked(self):
        s = self.line.lstrip()
        return self.checkbox in ("x", "X", "→") or s.startswith("→") or s.startswith("- →") \
            or re.match(r"^(?:- )?DONE \[", s) is not None

    def any_open(self):
        return any(n.checkbox in (" ", "~") or (n.checkbox is None and OPEN_BOX.search(n.line[:12]))
                   or marker_open(n.line) for n in self.walk())


def parse(lines):
    """-> list of sections: (heading_idx or None, [Node...]) plus blank handling."""
    sections, cur = [], [None, []]
    stack = []
    for i, line in enumerate(lines):
        if is_heading(line):
            sections.append(cur)
            cur, stack = [i, []], []
            continue
        if not line.strip():
            continue
        node = Node(i, line)
        ind = indent(line)
        while stack and indent(stack[-1].line) >= ind:
            stack.pop()
        if stack:
            stack[-1].children.append(node)
        else:
            cur[1].append(node)
        stack.append(node)
    sections.append(cur)
    return sections


def shorten(line):
    """Short form of a checked line with an open part: request, branch, pointer, open part."""
    if len(line) <= 400:
        return line
    m = CHECKBOX.match(line)
    skip = m.end() if m else 0
    body = line[skip:].lstrip()
    if body.startswith("→") or body.startswith("- →"):
        # continuation line ("  → DONE [branch]: ..."): keep its indent and bullet
        k = line.index("→", skip)
        head, rest = line[:k], line[k + 1:]
    else:
        k = line.find("→", skip)
        if k < 0:
            return line
        head, rest = line[:k].rstrip() + " ", line[k + 1:]
    branch = re.match(r"\s*(?:PART(?:LY| DONE)|DONE|FIXED)?\s*(\[[^\]]+\])?", rest)
    br = (" " + branch.group(1)) if branch and branch.group(1) else ""
    hits = [h for h in MARKER.finditer(line) if h.start() > len(head)]
    left = ""
    if hits:
        open_hits = [h for h in hits if marker_open(line[h.start():])]
        start = (open_hits[0] if open_hits else hits[-1]).start()
        left = " " + line[start:].strip()
    return f"{head}→ DONE{br} ({POINTER}).{left}"


def decide(node, parent_open, keep, archive):
    """Fill keep/archive dicts idx -> text. parent_open: parent is an open item."""
    box = node.checkbox
    subtree_open = node.any_open()
    if box in (" ", "~") or (box is None and not node.checked and (parent_open or node.idx is None)):
        keep[node.idx] = node.line
        for c in node.children:
            decide(c, True, keep, archive)
        return
    if box is None and not node.checked:
        # plain line (note, description) at top level or under a checked parent
        if parent_open is None:  # top-level note
            keep[node.idx] = node.line
            for c in node.children:
                decide(c, True, keep, archive)
        elif subtree_open:
            keep[node.idx] = node.line
            archive[node.idx] = node.line
            for c in node.children:
                decide(c, False, keep, archive)
        else:
            for n in node.walk():
                archive[n.idx] = n.line
        return
    # checked
    if not subtree_open:
        for n in node.walk():
            archive[n.idx] = n.line
        return
    archive[node.idx] = node.line
    keep[node.idx] = shorten(node.line) if marker_open(node.line) or node.children else node.line
    if len(keep[node.idx]) > 400 and not marker_open(node.line):
        m = CHECKBOX.match(node.line)
        k = node.line.find("→", m.end() if m else 0)
        head = node.line[:k].rstrip() if k > 0 else node.line[:200]
        keep[node.idx] = f"{head} → DONE ({POINTER}); open parts below"
    for c in node.children:
        decide(c, False, keep, archive)


def agents_running(nodes, keep, archive):
    rounds = []
    for n in nodes:
        m = re.search(r"Rounds? (\d+)", n.line)
        rounds.append(int(m.group(1)) if m else None)
    known = [r for r in rounds if r is not None]
    floor = (max(known) - 2) if known else 0
    cut = len(nodes)
    for k, r in enumerate(rounds):
        if r is not None and r < floor:
            cut = k
            break
    for k, n in enumerate(nodes):
        for x in n.walk():
            (keep if k < cut else archive)[x.idx] = x.line


def split(lines):
    sections = parse(lines)
    keep, archive = {}, {}
    sec_info = []
    for hidx, nodes in sections:
        heading = lines[hidx] if hidx is not None else ""
        if heading.startswith(AGENTS_RUNNING):
            agents_running(nodes, keep, archive)
        elif any(heading.startswith(h) for h in ARCHIVE_WHOLE_HEADINGS) and \
                not any(n.any_open() and n.checkbox in (" ", "~") for n in nodes):
            for n in nodes:
                for x in n.walk():
                    archive[x.idx] = x.line
        else:
            for n in nodes:
                decide(n, None, keep, archive)
        sec_info.append((hidx, nodes))

    # A bold group label ("**Zoning and parcels**") with no kept item before the next label goes.
    for hidx, nodes in sec_info:
        for k, n in enumerate(nodes):
            if n.idx in keep and re.match(r"^\*\*[^*]+\*\*\s*$", n.line):
                nxt = nodes[k + 1] if k + 1 < len(nodes) else None
                if nxt is None or nxt.idx not in keep or re.match(r"^\*\*[^*]+\*\*\s*$", nxt.line):
                    archive.setdefault(n.idx, n.line)
                    del keep[n.idx]

    # A section with no kept item (only notes) goes to the archive whole.
    def is_item(n):
        return n.checkbox is not None or n.checked or n.line.startswith("- ")
    kept_heads = set()
    for hidx, nodes in sec_info:
        heading = lines[hidx] if hidx is not None else ""
        items_kept = any(x.idx in keep and is_item(x) for n in nodes for x in n.walk())
        always = hidx is None or any(heading.startswith(h) for h in ALWAYS_KEEP_HEADINGS) \
            or marker_open(heading)
        if not items_kept and not always:
            for n in nodes:
                for x in n.walk():
                    if x.idx in keep:
                        archive.setdefault(x.idx, x.line)
                        del keep[x.idx]
        if items_kept or always:
            if hidx is not None:
                kept_heads.add(hidx)
    # Parent headings ("#" over "##" over "###") stay if any child section stayed.
    level = lambda h: len(lines[h]) - len(lines[h].lstrip("#"))
    heads = [h for h, _ in sec_info if h is not None]
    for k, h in enumerate(heads):
        if h in kept_heads:
            continue
        for h2 in heads[k + 1:]:
            if level(h2) <= level(h):
                break
            if h2 in kept_heads:
                kept_heads.add(h)
                break

    def emit(chosen, headset):
        out, prev_blank = [], True
        # chosen: idx -> text; emit in order, headings from headset, blank lines preserved once
        for i, line in enumerate(lines):
            if i in headset:
                if not prev_blank:
                    out.append("")
                out.append(line)
                prev_blank = False
            elif i in chosen:
                out.append(chosen[i])
                prev_blank = False
            elif not line.strip() and out and not prev_blank:
                # keep a blank only if the next non-blank original line is emitted
                j = i + 1
                while j < len(lines) and not lines[j].strip():
                    j += 1
                if j < len(lines) and (j in chosen or j in headset):
                    out.append("")
                    prev_blank = True
        while out and not out[-1].strip():
            out.pop()
        return out

    arch_heads = set()
    for hidx, nodes in sec_info:
        if hidx is not None and any(x.idx in archive for n in nodes for x in n.walk()):
            arch_heads.add(hidx)
        if hidx is not None and hidx not in kept_heads:
            arch_heads.add(hidx)
    # archive headings need their parents too
    for k, h in enumerate(heads):
        if h in arch_heads:
            continue
        for h2 in heads[k + 1:]:
            if level(h2) <= level(h):
                break
            if h2 in arch_heads:
                arch_heads.add(h)
                break
    keep = {i: rewrite(t) for i, t in keep.items()}
    return emit(keep, kept_heads), emit(archive, arch_heads)


def open_lines(lines):
    return [l.strip() for l in lines if CHECKBOX.match(l) and OPEN_BOX.search(l[:20])]


def marker_lines(lines):
    return [l.strip() for l in lines if MARKER.search(l)]


def check(orig, todo, arch):
    tset, aset = set(l.strip() for l in todo), set(l.strip() for l in arch)
    both = tset | aset
    bad = 0
    for l in orig:
        if l.strip() and l.strip() not in both and rewrite(l).strip() not in tset:
            print("MISSING:", l[:160]); bad += 1
    for l in open_lines(orig):
        if rewrite(l) not in tset:
            print("OPEN ITEM NOT IN todo.md:", l[:160]); bad += 1
    for l in marker_lines(orig):
        if l not in both and rewrite(l) not in tset:
            print("LEFT LINE LOST:", l[:160]); bad += 1
    print(f"lines: orig {len(orig)}, todo {len(todo)}, archive {len(arch)}")
    print(f"open '[ ]'/'[~]' items: orig {len(open_lines(orig))}, todo {len(open_lines(todo))}")
    print(f"open-marker lines (Left/PARTLY/...): orig {len(marker_lines(orig))}, "
          f"in todo {sum(1 for l in marker_lines(orig) if rewrite(l) in tset)}, "
          f"archive-only {sum(1 for l in marker_lines(orig) if l not in tset and l in aset)}")
    print("OK" if not bad else f"{bad} PROBLEMS")
    return bad == 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--todo", default="todo.md")
    ap.add_argument("--archive", default=ARCHIVE_DEFAULT)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--check", metavar="ORIG")
    a = ap.parse_args()
    rd = lambda p: Path(p).read_text(encoding="utf-8").split("\n")
    if a.check:
        sys.exit(0 if check(rd(a.check), rd(a.todo), rd(a.archive)) else 1)
    orig = rd(a.todo)
    todo, arch = split(orig)
    ap_ = Path(a.archive)
    header = ["# todo.md archive: finished items (moved 2026-11-21, round 13 cleanup)", "",
              "Verbatim text of finished todo.md items, under their original headings. "
              "Open work stays in `todo.md`. Made by `tools/todo_split.py`.", ""]
    if ap_.exists():
        old = rd(ap_)
        have = set(l.strip() for l in old if l.strip())
        new = [l for l in arch if l.strip() and (l.startswith("#") or l.strip() not in have)]
        new = [l for k, l in enumerate(new)
               if not (l.startswith("#") and (k + 1 >= len(new) or new[k + 1].startswith("#")))]
        arch_out = old + ([""] + [f"## Archived again {Path(a.todo).name} (re-run)"] + new if new else [])
    else:
        arch_out = header + arch
    print(f"todo.md {len(orig)} -> {len(todo)} lines; archive {len(arch_out)} lines")
    if a.dry_run:
        check(orig, todo, arch_out)
        return
    ap_.parent.mkdir(parents=True, exist_ok=True)
    ap_.write_text("\n".join(arch_out) + "\n", encoding="utf-8")
    Path(a.todo).write_text("\n".join(todo) + "\n", encoding="utf-8")
    check(orig, todo, arch_out)


if __name__ == "__main__":
    main()
