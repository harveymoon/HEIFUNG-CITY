"""Make `legacy_town.ron`, the old-format save the old-save tests load
(`world::legacy_save_text`): drop every struct field, by its path (enum
variants included), that a real old save does not have, inside the structs
that save shows, and strip the indentation.

    python trim_legacy.py saves/flags/flag_patch.ron NEW.ron legacy_town.ron

OLD: a real save from before the land market, mega-corps and the league
(Harvey's `saves/flags/flag_patch.ron`, gitignored). NEW: a small town saved
by the current game (a flat 900 m map, a cross of streets, five zones, two
days with the rule-based mind).
"""
import re, sys

FIELD = re.compile(r'^(\s*)([a-z_][a-z0-9_]*): ?(.*)$')
VARIANT = re.compile(r'^\s*(?:\([^()]*\):\s*)?([A-Z][A-Za-z0-9_]*)\(')


def variant(text):
    m = VARIANT.match(text)
    return m.group(1) if m else ''

STR = re.compile(r'"(?:[^"\\]|\\.)*"')


def balance(text):
    t = STR.sub('""', text)
    return sum(t.count(c) for c in '([{') - sum(t.count(c) for c in ')]}')


def paths(lines):
    out = set()
    stack = []  # (indent, key)
    for l in lines:
        ind = len(l) - len(l.lstrip(' '))
        while stack and stack[-1][0] >= ind:
            stack.pop()
        m = FIELD.match(l)
        if m:
            key = m.group(2)
            p = tuple(k for _, k in stack) + (key,)
            out.add(p)
            if balance(m.group(3)) > 0:
                stack.append((ind, key + ':' + variant(m.group(3))))
        elif balance(l) > 0 and variant(l):
            stack.append((ind, '#' + variant(l)))
    return out


old = open(sys.argv[1], encoding='utf-8').read().split('\n')
new = open(sys.argv[2], encoding='utf-8').read().split('\n')
keep = paths(old)
# Containers the old save shows: only fields inside these are judged (a
# variant or struct the old save never holds keeps all its fields).
seen = {p[:-1] for p in keep}
out = []
stack = []
i = 0
dropped = set()
while i < len(new):
    l = new[i]
    ind = len(l) - len(l.lstrip(' '))
    while stack and stack[-1][0] >= ind:
        stack.pop()
    m = FIELD.match(l)
    if m:
        key = m.group(2)
        p = tuple(k for _, k in stack) + (key,)
        b = balance(m.group(3))
        if p not in keep and p[:-1] in seen:
            dropped.add(p)
            # skip the field and its block
            i += 1
            while b > 0 and i < len(new):
                b += balance(new[i])
                i += 1
            continue
        if b > 0:
            stack.append((ind, key + ':' + variant(m.group(3))))
    elif balance(l) > 0 and variant(l):
        stack.append((ind, '#' + variant(l)))
    out.append(l)
    i += 1
# Compact: indentation is only for reading.
text = '\n'.join(x.strip() for x in out)
open(sys.argv[3], 'w', encoding='utf-8', newline='\n').write(text)
tops = sorted({p[0] for p in dropped if len(p) == 1})
print('dropped top-level:', tops)
print('dropped nested:', len([p for p in dropped if len(p) > 1]))
