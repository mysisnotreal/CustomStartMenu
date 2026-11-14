"""Draws fonts/previews/<id>.png for every font in fonts/catalog.tsv.

    python tools/fonts/render_previews.py <folder for the downloaded fonts>

Downloads each font once into that folder (skipped when already there), builds
fontpreview.exe with the Visual C++ tools, and runs it over the whole catalog.
"""
import os
import subprocess
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))


def main():
    cache = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "build", "font-cache"))
    os.makedirs(cache, exist_ok=True)
    out_dir = os.path.join(ROOT, "fonts", "previews")
    os.makedirs(out_dir, exist_ok=True)

    lines = []
    with open(os.path.join(ROOT, "fonts", "catalog.tsv"), encoding="utf-8") as f:
        for row in f:
            cat, name, fid, url, size, script, flags = row.rstrip("\n").split("\t")
            local = os.path.join(cache, fid + os.path.splitext(url)[1])
            if not os.path.exists(local) or os.path.getsize(local) != int(size):
                print("download", url)
                urllib.request.urlretrieve(url, local)
            lines.append("\t".join([local, os.path.join(out_dir, fid + ".png"), script, flags]))
    listing = os.path.join(cache, "list.tsv")
    with open(listing, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")

    exe = os.path.join(ROOT, "build", "fontpreview.exe")
    os.makedirs(os.path.dirname(exe), exist_ok=True)
    vswhere = os.path.expandvars(r"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe")
    vs = subprocess.run([vswhere, "-latest", "-property", "installationPath"], capture_output=True, text=True).stdout.strip()
    vcvars = os.path.join(vs, "VC", "Auxiliary", "Build", "vcvars64.bat")
    src = os.path.join(HERE, "fontpreview.cpp")
    build = f'call "{vcvars}" >nul && cl /nologo /utf-8 /std:c++17 /O2 /EHsc /DUNICODE /D_UNICODE "{src}" /Fo"{os.path.dirname(exe)}\\\\" /Fe"{exe}"'
    subprocess.run(build, shell=True, check=True)
    subprocess.run([exe, listing], check=True)


if __name__ == "__main__":
    main()
