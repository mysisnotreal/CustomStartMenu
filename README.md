# Custom Start (CSM)

A tray app that replaces the Windows 11 Start menu with a minimal, animated command bar.

- Press the **Windows key**, **Ctrl+Esc**, or click the **Start button**. Windows shortcuts like Win+E and Win+R keep working.
- Type to find any installed app. Enter opens the selected app, Shift+Enter runs the raw text, and Ctrl+Shift+Enter runs it as administrator. Text that isn't an app opens as a URL or a web search.
- Type `>` to see the commands:
  - `> theme` opens the theme picker.
  - `> expanded` turns the list of matching apps under the bar on or off.
  - `> startup` turns starting with Windows on or off.
  - `> quit` closes Custom Start.
- There are 8 themes: Dark, Light, Translucent, Glass, Midnight, Sunset, Mint and OLED.

## Install

Download **CSMInstaller.exe** from the [latest release](https://github.com/mysisnotreal/CustomStartMenu/releases/latest) and run it. Setup then:

1. Greets you with an animated "hello" in your PC's language.
2. Asks for your language.
3. Asks which backend to use: **C++** (recommended) or **Rust**. Both have the same features.
4. Downloads the build you chose from this repository's GitHub releases. It installs per user to `%LOCALAPPDATA%\Programs\CustomStartMenu`, so no administrator rights are needed.

Custom Start checks for updates every 6 hours. When a newer version is on GitHub, setup opens and shows **Update to V*x.y.z*?** with the changes: additions in green, removals in red.

You can uninstall from *Settings → Apps*, or by running setup again.

## Build from source

You need Visual Studio 2022 with *Desktop development with C++*. The Rust backend also needs a stable Rust toolchain (MSVC). Then run:

```bat
build.bat
```

This writes `dist\CSM-cpp.exe`, `dist\CSM-rust.exe` and `dist\CSMInstaller.exe`.

| Folder | What it is |
| --- | --- |
| `cpp/` | C++ backend: Win32, Direct2D, DirectWrite |
| `rust/` | Rust backend: a feature-for-feature port using the `windows` crate |
| `installer/` | CSMInstaller: setup, updater and uninstaller |
| `assets/` | Icon |
| `manifest.json` | What the installer reads: latest version, release asset names and the changelog |

## Publishing an update

1. Bump the version in `manifest.json` and add a `releases` entry with the `added` and `removed` lists.
2. Build, then create the GitHub release `v<version>` with the three executables attached:

   ```bat
   gh release create v1.1.0 dist\CSM-cpp.exe dist\CSM-rust.exe dist\CSMInstaller.exe
   ```

3. Push `manifest.json`. Installed copies pick up the update on their next check.

## License

MIT. The installer's "hello" lettering path comes from the CodePen [Apple OOBE hello](https://codepen.io/steefmaster/pen/MWvdyGb) (MIT).
