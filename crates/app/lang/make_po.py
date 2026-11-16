"""Rebuild lang/<code>/LC_MESSAGES/swiftget.po from the @tr("...") texts in ui/**/*.slint.

Each language comes from lang/ui/<code>.json
(English text -> translation, written the way the .slint files write it).
"""
import json
import re
from pathlib import Path

here = Path(__file__).resolve().parent
ui_dir = here.parent / "ui"

texts = []
for path in sorted(ui_dir.rglob("*.slint")):
    src = path.read_text(encoding="utf-8")
    for m in re.finditer(r'@tr\("((?:[^"\\]|\\.)*)"', src):
        if m.group(1) not in texts:
            texts.append(m.group(1))
for k in ["Undo", "Redo", "Cut", "Copy", "Paste", "Select All"]:
    if k not in texts:
        texts.append(k)

tables = {
    path.stem: json.loads(path.read_text(encoding="utf-8"))
    for path in sorted((here / "ui").glob("*.json"))
}

for code, table in tables.items():
    out = [
        'msgid ""',
        'msgstr ""',
        '"Content-Type: text/plain; charset=UTF-8\\n"',
        f'"Language: {code}\\n"',
        '"Plural-Forms: nplurals=2; plural=(n != 1);\\n"',
        "",
    ]
    for t in texts:
        out += [f'msgid "{t}"', f'msgstr "{table.get(t, t)}"', ""]
    po_dir = here / code / "LC_MESSAGES"
    po_dir.mkdir(parents=True, exist_ok=True)
    with open(po_dir / "swiftget.po", "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(out))
    missing = [t for t in texts if t not in table]
    print(f"{code}: {len(texts)} texts, {len(missing)} without translation")
    for t in missing:
        print("  missing:", t)
