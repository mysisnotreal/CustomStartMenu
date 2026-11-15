"""Builds res/emoji.txt (the "> emoji" picker's list) from Unicode's emoji-test.txt.

    python tools/gen_emoji.py path/to/emoji-test.txt

Keeps fully-qualified emoji without skin-tone variants. Output lines:
    @Group name
    <emoji>\t<name>
"""
import sys
import os

src = sys.argv[1]
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'res', 'emoji.txt')
skin = {0x1F3FB, 0x1F3FC, 0x1F3FD, 0x1F3FE, 0x1F3FF}
lines = []
group = None
for raw in open(src, encoding='utf-8'):
    raw = raw.rstrip('\n')
    if raw.startswith('# group:'):
        group = raw.split(':', 1)[1].strip()
        if group != 'Component':
            lines.append('@' + group)
        continue
    if not raw or raw.startswith('#') or group == 'Component':
        continue
    codes, rest = raw.split(';', 1)
    status, comment = rest.split('#', 1)
    if status.strip() != 'fully-qualified':
        continue
    cps = [int(c, 16) for c in codes.split()]
    if any(c in skin for c in cps):
        continue
    # comment: " 😀 E1.0 grinning face"
    parts = comment.strip().split(' ', 2)
    name = parts[2] if len(parts) > 2 else ''
    lines.append(''.join(chr(c) for c in cps) + '\t' + name)
with open(out, 'w', encoding='utf-8', newline='\n') as f:
    f.write('\n'.join(lines) + '\n')
print(len([l for l in lines if not l.startswith('@')]), 'emoji ->', os.path.normpath(out))
