<p align="center">
  <img src="docs/images/banner.svg" width="100%" alt="Custom Start: a smooth, themeable command bar that replaces the Windows 11 Start menu">
</p>

<p align="center">
  <a href="https://github.com/mysisnotreal/CustomStartMenu/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/mysisnotreal/CustomStartMenu?style=flat-square&label=release&color=0A7CFF"></a>
  <a href="https://github.com/mysisnotreal/CustomStartMenu/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/mysisnotreal/CustomStartMenu/total?style=flat-square&label=downloads&color=8C3CFA"></a>
  <a href="https://github.com/mysisnotreal/CustomStartMenu/stargazers"><img alt="Stars" src="https://img.shields.io/github/stars/mysisnotreal/CustomStartMenu?style=flat-square&color=FF5AA0"></a>
  <img alt="Windows 11" src="https://img.shields.io/badge/Windows-11-00BEC8?style=flat-square&logo=windows11&logoColor=white">
  <img alt="C++ and Rust" src="https://img.shields.io/badge/C%2B%2B%20%2F%20Rust-native-3F6FB5?style=flat-square">
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/github/license/mysisnotreal/CustomStartMenu?style=flat-square&color=5EE6A8"></a>
</p>

<p align="center">
  <a href="https://github.com/mysisnotreal/CustomStartMenu/releases/latest/download/CSMInstaller.exe">
    <img alt="Download for Windows" src="https://img.shields.io/badge/Download%20for%20Windows-CSMInstaller.exe-0A7CFF?style=for-the-badge&logo=windows11&logoColor=white">
  </a>
</p>

<p align="center">
  <a href="#-features">Features</a> &nbsp;·&nbsp;
  <a href="#-themes">Themes</a> &nbsp;·&nbsp;
  <a href="#-setup">Setup</a> &nbsp;·&nbsp;
  <a href="#%EF%B8%8F-shortcuts">Shortcuts</a> &nbsp;·&nbsp;
  <a href="#-analytics">Analytics</a>
</p>

<br>

<p align="center">
  <img src="docs/images/demo.png" width="880" alt="Custom Start opening, searching for apps and previewing themes">
</p>

<br>

## ✨ Features

<table>
  <tr>
    <td width="50%" valign="top">
      <img src="docs/images/search.png" alt="Searching apps">
      <h3>Instant app search</h3>
      Press <kbd>Win</kbd> and start typing. Every installed app, including Store apps, shows up as you type, with its icon, matched letters in bold, and an inline completion you can accept with <kbd>Tab</kbd>.
    </td>
    <td width="50%" valign="top">
      <img src="docs/images/commands.png" alt="Commands">
      <h3>Commands with <code>&gt;</code></h3>
      Type <code>&gt;</code> to see your games, manage pinned apps, switch theme or font, turn the results list on or off, start with Windows, or quit. No settings window needed.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="docs/images/themes.png" alt="Theme picker">
      <h3>Live theme picker</h3>
      <code>&gt; theme</code> opens a grid of live previews. Arrow keys cross-fade the whole bar into each theme, and <kbd>Enter</kbd> applies it.
    </td>
    <td width="50%" valign="top">
      <img src="docs/images/bar.png" alt="The command bar">
      <h3>Smooth all the way</h3>
      The bar springs open, the caret glides, freshly typed letters fade in, and a frosted, edge-faded backdrop blurs whatever is behind it. Rendered with Direct2D at your display's refresh rate.
    </td>
  </tr>
</table>

- **Replaces the Start menu.** <kbd>Win</kbd>, <kbd>Ctrl</kbd>+<kbd>Esc</kbd> and the Start button all open Custom Start, while shortcuts like <kbd>Win</kbd>+<kbd>E</kbd> and <kbd>Win</kbd>+<kbd>R</kbd> keep working.
- **Your pinned apps, above the bar.** The apps pinned to your Windows Start menu come along as tiles, with names under the icons. Drag them to reorder, right-click to hide or unpin, and type `> pins` to bring hidden ones back.
- **Right-click menu.** Run as administrator, Open file location, Pin to Start and Pin to taskbar, on any app or pinned tile.
- **All your games in one place.** `> games` finds the games on every drive (Steam, Epic, GOG, Ubisoft, Xbox, Riot, EA and more) and starts them. Type after it to filter.
- **More than apps.** Type a path, a command or a URL and it runs or opens. Program names work too, so `cmd` finds Command Prompt. Anything else becomes a web search.
- **Your font.** `> font` opens a picker with 477 free fonts (the whole SF Pro family, plus [open-fonts](https://github.com/kiwi0fruit/open-fonts) sorted into Sans Serif, Serif, Monospace, Math, Symbols & Emoji and fonts for other languages). Each card shows what the font looks like; nothing is downloaded until you click its Download button, and then every bit of text uses it. `> font NAME` searches, and `> fontfolder` opens the folder (`%LOCALAPPDATA%\Programs\CustomStartMenu\Fonts`) so you can add your own `.otf` / `.ttf` files.
- **Stays out of your games.** Hardware acceleration (on by default) draws the bar on your graphics card. It opens on top of fullscreen games and <kbd>Esc</kbd> hands the keyboard straight back. While a game is in front, Custom Start stops watching the mouse and holds update prompts until you're done.
- **Two native backends.** Pick **C++** or **Rust** when you install. Both look and behave the same.
- **Speaks your language.** English, Español, Français, Deutsch, Italiano, Português, Русский, 日本語, 中文 and 한국어.
- **Stays up to date.** New versions show up on their own, with a clear list of what changed.

## 🎨 Themes

Eight built-in themes: **Dark**, **Light**, **Translucent**, **Glass**, **Midnight**, **Sunset**, **Mint** and **OLED**.

<table>
  <tr>
    <td width="50%"><img src="docs/images/theme-dark.png" alt="Dark theme"><p align="center"><b>Dark</b></p></td>
    <td width="50%"><img src="docs/images/theme-light.png" alt="Light theme"><p align="center"><b>Light</b></p></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/theme-midnight.png" alt="Midnight theme"><p align="center"><b>Midnight</b></p></td>
    <td width="50%"><img src="docs/images/theme-sunset.png" alt="Sunset theme"><p align="center"><b>Sunset</b></p></td>
  </tr>
</table>

## 🚀 Setup

Download **[CSMInstaller.exe](https://github.com/mysisnotreal/CustomStartMenu/releases/latest/download/CSMInstaller.exe)** and run it. No admin rights needed.

<table>
  <tr>
    <td width="50%" valign="top"><img src="docs/images/setup-hello.png" alt="Hello screen"><p align="center"><b>1.</b> A hand-drawn hello in your PC's language</p></td>
    <td width="50%" valign="top"><img src="docs/images/setup-language.png" alt="Language selection"><p align="center"><b>2.</b> Pick your language</p></td>
  </tr>
  <tr>
    <td width="50%" valign="top"><img src="docs/images/setup-backend.png" alt="Backend selection"><p align="center"><b>3.</b> Choose the C++ or Rust backend</p></td>
    <td width="50%" valign="top"><img src="docs/images/setup-performance.png" alt="Hardware acceleration"><p align="center"><b>4.</b> Hardware acceleration, on by default</p></td>
  </tr>
  <tr>
    <td valign="top" colspan="2" align="center"><img src="docs/images/setup-update.png" alt="Update prompt" width="50%"><p align="center"><b>Later:</b> updates arrive with a changelog</p></td>
  </tr>
</table>

Uninstall any time from **Settings → Apps**.

## ⌨️ Shortcuts

| Key | Action |
| --- | --- |
| <kbd>Win</kbd> · <kbd>Ctrl</kbd>+<kbd>Esc</kbd> · Start button | Open or close Custom Start |
| <kbd>Enter</kbd> | Open the selected app |
| <kbd>Shift</kbd>+<kbd>Enter</kbd> | Run exactly what you typed |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Enter</kbd> | Run as administrator |
| <kbd>Tab</kbd> · <kbd>→</kbd> | Accept the completion |
| <kbd>↑</kbd> <kbd>↓</kbd> · <kbd>PgUp</kbd> <kbd>PgDn</kbd> | Move through results |
| Right-click · <kbd>Menu</kbd> key | App menu (administrator, file location, pin) |
| Drag a pinned tile | Reorder pinned apps |
| <kbd>Esc</kbd> | Close |
| `> games` · `> pins` · `> theme` · `> font` · `> fontfolder` · `> expanded` · `> startup` · `> quit` | Commands |

## 📊 Analytics

<table>
  <tr>
    <td><b>Downloads</b></td>
    <td>
      <img alt="Total downloads" src="https://img.shields.io/github/downloads/mysisnotreal/CustomStartMenu/total?style=flat-square&label=all%20time&color=8C3CFA">
      <img alt="Latest release downloads" src="https://img.shields.io/github/downloads/mysisnotreal/CustomStartMenu/latest/total?style=flat-square&label=latest%20release&color=8C3CFA">
    </td>
  </tr>
  <tr>
    <td><b>By file</b></td>
    <td>
      <img alt="Installer downloads" src="https://img.shields.io/github/downloads/mysisnotreal/CustomStartMenu/latest/CSMInstaller.exe?style=flat-square&label=installer&color=0A7CFF">
      <img alt="C++ backend downloads" src="https://img.shields.io/github/downloads/mysisnotreal/CustomStartMenu/latest/CSM-cpp.exe?style=flat-square&label=C%2B%2B%20backend&color=3F6FB5">
      <img alt="Rust backend downloads" src="https://img.shields.io/github/downloads/mysisnotreal/CustomStartMenu/latest/CSM-rust.exe?style=flat-square&label=Rust%20backend&color=B7410E">
    </td>
  </tr>
  <tr>
    <td><b>Community</b></td>
    <td>
      <img alt="Stars" src="https://img.shields.io/github/stars/mysisnotreal/CustomStartMenu?style=flat-square&color=FF5AA0">
      <img alt="Forks" src="https://img.shields.io/github/forks/mysisnotreal/CustomStartMenu?style=flat-square&color=FF5AA0">
      <img alt="Watchers" src="https://img.shields.io/github/watchers/mysisnotreal/CustomStartMenu?style=flat-square&color=FF5AA0">
      <img alt="Page views" src="https://hits.sh/github.com/mysisnotreal/CustomStartMenu.svg?style=flat-square&label=views&color=FF5AA0">
    </td>
  </tr>
  <tr>
    <td><b>Activity</b></td>
    <td>
      <img alt="Last commit" src="https://img.shields.io/github/last-commit/mysisnotreal/CustomStartMenu?style=flat-square&color=00BEC8">
      <img alt="Commit activity" src="https://img.shields.io/github/commit-activity/m/mysisnotreal/CustomStartMenu?style=flat-square&color=00BEC8">
      <img alt="Open issues" src="https://img.shields.io/github/issues/mysisnotreal/CustomStartMenu?style=flat-square&color=00BEC8">
    </td>
  </tr>
  <tr>
    <td><b>Code</b></td>
    <td>
      <img alt="Top language" src="https://img.shields.io/github/languages/top/mysisnotreal/CustomStartMenu?style=flat-square&color=5EE6A8">
      <img alt="Code size" src="https://img.shields.io/github/languages/code-size/mysisnotreal/CustomStartMenu?style=flat-square&color=5EE6A8">
      <img alt="Repo size" src="https://img.shields.io/github/repo-size/mysisnotreal/CustomStartMenu?style=flat-square&color=5EE6A8">
    </td>
  </tr>
</table>

<p align="center">
  <a href="https://star-history.com/#mysisnotreal/CustomStartMenu&Date">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=mysisnotreal/CustomStartMenu&type=Date&theme=dark">
      <img alt="Star history" src="https://api.star-history.com/svg?repos=mysisnotreal/CustomStartMenu&type=Date" width="720">
    </picture>
  </a>
</p>

## 📄 License

[MIT](LICENSE). The hand-drawn "hello" in Setup comes from [Apple OOBE hello](https://codepen.io/steefmaster/pen/MWvdyGb) on CodePen (MIT).

<p align="center"><sub>Made for Windows 11 · If you like it, leave a ⭐</sub></p>
