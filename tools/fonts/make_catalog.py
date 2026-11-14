"""Builds fonts/catalog.tsv, the list of fonts shown by "> font".

Every .otf / .ttf in the two source repositories becomes one entry with a clean name
("SF-Pro-Display-BlackItalic.otf" -> "SF Pro Display Black Italic") and a category.
Fonts are only downloaded by the app when someone clicks Download; the card pictures
come from fonts/previews (see render_previews.py).

    python tools/fonts/make_catalog.py

Columns: category, name, id, url, size in bytes, sample script, flags (c = color font).
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOURCES = [
    # (repository, commit) - pinned so the download links never move
    ("sahibjotsaggu/San-Francisco-Pro-Fonts", "8bfea09aa6f1139479f80358b2e1e5c6dc991a58"),
    ("kiwi0fruit/open-fonts", "deb211dd1e08becf3b9300b02365cd79415c64d4"),
]

CATEGORY_ORDER = [
    "SF Pro", "Sans Serif", "Serif", "Monospace", "Math", "Symbols & Emoji", "Arabic", "Hebrew",
    "Indian Scripts", "Thai", "Armenian & Georgian", "Chinese, Japanese & Korean",
]
STYLE_ORDER = ["Hairline", "Thin", "Ultra Light", "Extra Light", "Light", "Normal", "Regular", "Book", "Medium",
               "Semi Bold", "Bold", "Extra Bold", "Heavy", "Black"]

# file name stems whose words can't be found by splitting on case
SPECIAL = [
    (r"^EmbedSerifR-", "Embed Serif Regular"), (r"^EmbedSerifB-", "Embed Serif Bold"),
    (r"^EmbedSerifIt-", "Embed Serif Italic"), (r"^EmbedSerifBIt-", "Embed Serif Bold Italic"),
    (r"^EmbedSerifSb-", "Embed Serif Semi Bold"), (r"^EmbedSerifSbIt-", "Embed Serif Semi Bold Italic"),
    (r"^MJ_Cal-", "MathJax Caligraphic "), (r"^MJ_Mat-", "MathJax Math "), (r"^MJ-", "MathJax Main "),
    (r"^hintedSymbola$", "Symbola Hinted"),
]
FIXES = [("Deja Vu", "DejaVu"), ("Math Jax", "MathJax"), ("Semibold", "Semi Bold"), ("Extralight", "Extra Light"),
         ("Ultralight", "Ultra Light"), ("Regular Italic", "Italic")]


def clean_name(filename):
    """The regex clean-up: drop the extension, split on - and _, split joined words
    (camelCase and acronyms), and spell out style abbreviations."""
    stem = re.sub(r"\.(otf|ttf)$", "", filename, flags=re.I)
    for pat, rep in SPECIAL:
        if re.search(pat, stem):
            if rep.endswith(" "):
                stem = re.sub(pat, "", stem)
                return tidy(rep + split_words(stem))
            return rep
    return tidy(split_words(stem))


def split_words(s):
    s = re.sub(r"(?<=[a-z])It$", "Italic", s)            # SemiboldIt -> SemiboldItalic
    s = re.sub(r"(?<=-)It$", "Italic", s)                # -It -> -Italic
    s = re.sub(r"SemiBold|Semibold", "Semibold", s)
    s = re.sub(r"[-_]+", " ", s)
    s = re.sub(r"(?<=[a-z])(?=[A-Z])", " ", s)          # camelCase
    s = re.sub(r"(?<=[A-Z])(?=[A-Z][a-z])", " ", s)     # IBMPlex -> IBM Plex
    s = re.sub(r"(?<=[a-z])(?=\d+(\s|$))", " ", s)       # SourceSans3 -> Source Sans 3
    return s


def tidy(s):
    s = re.sub(r"\s+", " ", s).strip()
    for a, b in FIXES:
        s = s.replace(a, b)
    return s


def category_of(name, path):
    n = name
    if "San-Francisco" in path:
        return "SF Pro"
    if "/open-fonts-cjk/" in path:
        return "Chinese, Japanese & Korean"
    if re.search(r"Math", n):
        return "Math"
    if re.search(r"Symbola|Twemoji", n):
        return "Symbols & Emoji"
    if re.search(r"Arabic|Naskh|Nastaliq|Amiri|Kawkab", n):
        return "Arabic"
    if re.search(r"Hebrew|David Libre", n):
        return "Hebrew"
    if re.search(r"Bengali|Devanagari|Tamil|Halant|Catamaran", n):
        return "Indian Scripts"
    if re.search(r"Thai", n):
        return "Thai"
    if re.search(r"Armenian|Georgian", n):
        return "Armenian & Georgian"
    if re.search(r"Mono|Code|Consol|Inconsolata|Cousine", n):
        return "Monospace"
    if re.search(r"Serif|Libertinus|Vollkorn|STIX|XITS", n) and "Sans" not in n:
        return "Serif"
    return "Sans Serif"


def script_of(name, cat):
    for pat, script in [(r"Arabic|Naskh|Amiri|Kawkab", "arab"), (r"Nastaliq", "urdu"), (r"Hebrew|David", "hebr"),
                        (r"Bengali", "beng"), (r"Devanagari|Halant", "deva"), (r"Tamil|Catamaran", "taml"),
                        (r"Thai", "thai"), (r"Armenian", "armn"), (r"Georgian", "geor"), (r"Twemoji", "emoji"),
                        (r"Symbola", "symb"), (r"Math", "math")]:
        if re.search(pat, name):
            return script
    return "cjk" if cat.startswith("Chinese") else "latn"


def style_key(name):
    italic = bool(re.search(r"Italic|Oblique", name))
    weight = 6
    for i, w in enumerate(STYLE_ORDER):
        if re.search(r"\b" + w + r"\b", name):
            weight = i
    family = name
    for w in STYLE_ORDER + ["Italic", "Oblique", "Hinted"]:
        family = re.sub(r"\b" + w + r"\b", "", family)
    return (re.sub(r"\s+", " ", family).strip().lower(), italic, weight)


def tree(repo, sha):
    out = subprocess.run(["gh", "api", f"repos/{repo}/git/trees/{sha}?recursive=1"], capture_output=True, check=True)
    return json.loads(out.stdout)["tree"]


def path_rank(path):
    # duplicates: prefer the main folder, then desktop (oft) builds over browser-only ones
    if path.startswith("open-fonts/"):
        return 0
    if "/browser_only/" in path:
        return 3
    return 1 if path.startswith("Fonts/") else 2


def main():
    entries = {}
    for repo, sha in SOURCES:
        for item in tree(repo, sha):
            path = item["path"]
            if item["type"] != "blob" or not re.search(r"\.(otf|ttf)$", path, re.I):
                continue
            base = path.rsplit("/", 1)[-1]
            name = clean_name(base)
            key = re.sub(r"\bRegular\b", "", name).replace(" ", "").lower()
            cand = (path_rank(path), name, base, path, repo, sha, item["size"])
            if key not in entries or cand[0] < entries[key][0]:
                entries[key] = cand

    rows = []
    used_ids = set()
    for rank, name, base, path, repo, sha, size in entries.values():
        cat = category_of(name, repo + "/" + path)
        fid = re.sub(r"[^a-z0-9]+", "-", re.sub(r"\.(otf|ttf)$", "", base, flags=re.I).lower()).strip("-")
        while fid in used_ids:
            fid += "-2"
        used_ids.add(fid)
        url = f"https://raw.githubusercontent.com/{repo}/{sha}/{path}"
        script = script_of(name, cat)
        flags = "c" if script == "emoji" else ""
        rows.append((CATEGORY_ORDER.index(cat), style_key(name), cat, name, fid, url, size, script, flags))

    rows.sort(key=lambda r: (r[0], r[1]))
    out = os.path.join(ROOT, "fonts", "catalog.tsv")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        for r in rows:
            f.write("\t".join(str(x) for x in r[2:]) + "\n")
    counts = {}
    for r in rows:
        counts[r[2]] = counts.get(r[2], 0) + 1
    for c in CATEGORY_ORDER:
        print(f"{counts.get(c, 0):4}  {c}")
    print(f"{len(rows):4}  fonts -> {out}")


if __name__ == "__main__":
    sys.exit(main())
