// Custom Start (CSM), Rust backend.
//
// A tray application that replaces the Windows Start menu with a minimal, animated
// command bar. Press the Windows key (or click Start) to open it.
//
// This is a feature-for-feature port of the C++ backend (cpp/main.cpp): the same
// rendering (Direct2D + DirectWrite into a per-pixel-alpha layered window presented
// once per vsync), the same themes, panels, settings and registry keys, so the two
// backends are interchangeable.

#![windows_subsystem = "windows"]
#![allow(non_snake_case, static_mut_refs, clippy::too_many_arguments)]

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::mem::{size_of, ManuallyDrop};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering::*};
use std::sync::{Condvar, Mutex, RwLock};

use windows::core::{w, Interface, HSTRING, PCWSTR, PWSTR, VARIANT};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Performance::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

mod calc;
mod caret;
mod custom;
mod emoji;
mod fonts;
mod games;
mod grid;
mod panels;
mod pins;
mod register;
mod tools;
mod widgets;
use games::{refresh_games, GAMES, GAMES_LOADING};
use pins::*;

// ---------------------------------------------------------------------------
// Layout (device independent pixels)
// ---------------------------------------------------------------------------
const BAR_H: f32 = 56.0;
const MARGIN: f32 = 52.0; // room around the container for the shadow / animation
const LOGO_X: f32 = 20.0;
const LOGO_SIZE: f32 = 18.0;
const TEXT_X: f32 = 54.0;
const RIGHT_PAD: f32 = 18.0;
const FONT_SIZE: f32 = 16.0;
const HINT_SIZE: f32 = 13.0;
const HINT_MAX_W: f32 = 240.0;
const CARET_W: f32 = 2.0;
const CARET_H: f32 = 20.0;
const SHADOW_SIGMA: f32 = 15.0;
const SHADOW_OFFSET_Y: f32 = 8.0;

const ROW_H: f32 = 42.0;
const PANEL_PAD: f32 = 6.0;
const MAX_ROWS: usize = 8;
const GRID_COLS: usize = 4;
const GRID_PAD: f32 = 12.0;
const GRID_HEADER_H: f32 = 28.0;
const CARD_GAP: f32 = 10.0;
const CARD_PREVIEW_H: f32 = 70.0;
const CARD_NAME_H: f32 = 28.0;
const MAX_PANEL_H: f32 = PANEL_PAD * 2.0 + ROW_H * MAX_ROWS as f32;

// Pinned apps: separate tiles above the bar
const PIN_COLS: i32 = 7;
const PIN_MAX_ROWS: i32 = 3;
const TILE_GAP: f32 = 10.0;
const TILE_H: f32 = 80.0;
const TILE_RADIUS: f32 = 12.0;
const TILE_ICON: f32 = 32.0;
const PINS_GAP: f32 = 14.0; // between the tiles and the bar
const PINS_MAX_H: f32 = PIN_MAX_ROWS as f32 * TILE_H + (PIN_MAX_ROWS - 1) as f32 * TILE_GAP + PINS_GAP;

// Right-click menu
const MENU_W: f32 = 236.0;
const MENU_ROW_H: f32 = 36.0;
const MENU_PAD: f32 = 5.0;

// The window always has room for the tallest pin area above the bar, so the bar (and the
// blurred backdrop behind it) never moves when pins are added or hidden. Drawing happens in
// "bar space", where the bar's top-left corner is (MARGIN, MARGIN) and the pins sit at y < MARGIN.
// "> layout" changes these (the size takes effect the next time the bar opens)
static mut LAY_BAR_W: f32 = 620.0;
static mut LAY_RADIUS: f32 = 14.0;
static mut LAY_VERTICAL_POS: f32 = 0.40; // bar center, as a fraction of work-area height
fn bar_w() -> f32 {
    unsafe { LAY_BAR_W }
}
fn radius() -> f32 {
    unsafe { LAY_RADIUS }
}
fn vertical_pos() -> f32 {
    unsafe { LAY_VERTICAL_POS }
}
fn tile_w() -> f32 {
    (bar_w() - (PIN_COLS - 1) as f32 * TILE_GAP) / PIN_COLS as f32
}
fn win_w() -> f32 {
    bar_w() + 2.0 * MARGIN
}
const WIN_MAX_H: f32 = PINS_MAX_H + BAR_H + MAX_PANEL_H + 2.0 * MARGIN;

// ---------------------------------------------------------------------------
// Themes
// ---------------------------------------------------------------------------
#[derive(Clone, Copy)]
struct Theme {
    name: &'static str,
    tint_top: D2D1_COLOR_F,
    tint_bottom: D2D1_COLOR_F,
    border_top: D2D1_COLOR_F,
    border_bottom: D2D1_COLOR_F,
    text: D2D1_COLOR_F,
    dim: D2D1_COLOR_F,
    ghost: D2D1_COLOR_F,
    accent: D2D1_COLOR_F,
    highlight: D2D1_COLOR_F,
    divider: D2D1_COLOR_F,
    shadow: f32,
}

const fn c(rgb: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 255) as f32 / 255.0,
        g: ((rgb >> 8) & 255) as f32 / 255.0,
        b: (rgb & 255) as f32 / 255.0,
        a,
    }
}

const fn theme(
    name: &'static str,
    tt: D2D1_COLOR_F,
    tb: D2D1_COLOR_F,
    bt: D2D1_COLOR_F,
    bb: D2D1_COLOR_F,
    text: D2D1_COLOR_F,
    dim: D2D1_COLOR_F,
    ghost: D2D1_COLOR_F,
    accent: D2D1_COLOR_F,
    highlight: D2D1_COLOR_F,
    divider: D2D1_COLOR_F,
    shadow: f32,
) -> Theme {
    Theme {
        name,
        tint_top: tt,
        tint_bottom: tb,
        border_top: bt,
        border_bottom: bb,
        text,
        dim,
        ghost,
        accent,
        highlight,
        divider,
        shadow,
    }
}

static mut THEMES: [Theme; 9] = [
    theme("Dark", c(0x2F2D2B, 0.95), c(0x292725, 0.97), c(0xFFFFFF, 0.13), c(0xFFFFFF, 0.04), c(0xEDEBE8, 1.0),
        c(0x8E8983, 1.0), c(0x726D67, 1.0), c(0x60CDFF, 1.0), c(0xFFFFFF, 0.07), c(0xFFFFFF, 0.07), 0.60),
    theme("Light", c(0xFAFAFA, 0.93), c(0xF1F1F1, 0.95), c(0xFFFFFF, 0.80), c(0x000000, 0.07), c(0x1A1A1A, 1.0),
        c(0x6E6E6E, 1.0), c(0xA3A3A3, 1.0), c(0x005FB8, 1.0), c(0x000000, 0.055), c(0x000000, 0.07), 0.30),
    theme("Translucent", c(0x1E1E1E, 0.38), c(0x181818, 0.50), c(0xFFFFFF, 0.22), c(0xFFFFFF, 0.06), c(0xFFFFFF, 1.0),
        c(0xD2D2D2, 1.0), c(0xA8A8A8, 1.0), c(0x60CDFF, 1.0), c(0xFFFFFF, 0.11), c(0xFFFFFF, 0.12), 0.45),
    theme("Glass", c(0xFFFFFF, 0.38), c(0xF4F4F4, 0.48), c(0xFFFFFF, 0.60), c(0xFFFFFF, 0.18), c(0x101010, 1.0),
        c(0x3C3C3C, 1.0), c(0x6A6A6A, 1.0), c(0x005FB8, 1.0), c(0x000000, 0.07), c(0x000000, 0.09), 0.28),
    theme("Midnight", c(0x171E31, 0.96), c(0x10172A, 0.97), c(0x9DB4FF, 0.16), c(0x9DB4FF, 0.04), c(0xE6EAF5, 1.0),
        c(0x7D879E, 1.0), c(0x5D667C, 1.0), c(0x7AA2FF, 1.0), c(0x9DB4FF, 0.09), c(0x9DB4FF, 0.08), 0.65),
    theme("Sunset", c(0x3B2422, 0.96), c(0x2C1B24, 0.97), c(0xFFC9A8, 0.16), c(0xFFC9A8, 0.04), c(0xFBEDE6, 1.0),
        c(0xB08E86, 1.0), c(0x8A6C66, 1.0), c(0xFF9E64, 1.0), c(0xFFC9A8, 0.09), c(0xFFC9A8, 0.08), 0.60),
    theme("Mint", c(0x1D2A25, 0.96), c(0x16211D, 0.97), c(0xB4FFDD, 0.14), c(0xB4FFDD, 0.04), c(0xE6F2EC, 1.0),
        c(0x7FA394, 1.0), c(0x5F7E72, 1.0), c(0x5EE6A8, 1.0), c(0xB4FFDD, 0.08), c(0xB4FFDD, 0.07), 0.60),
    theme("OLED", c(0x000000, 0.985), c(0x000000, 0.99), c(0xFFFFFF, 0.12), c(0xFFFFFF, 0.05), c(0xFFFFFF, 1.0),
        c(0x8A8A8A, 1.0), c(0x5E5E5E, 1.0), c(0x60CDFF, 1.0), c(0xFFFFFF, 0.08), c(0xFFFFFF, 0.09), 0.70),
    // "> theme new" fills this one in
    theme("Custom", c(0x2F2D2B, 0.74), c(0x292725, 0.82), c(0xFFFFFF, 0.13), c(0xFFFFFF, 0.04), c(0xEDEBE8, 1.0),
        c(0x8E8983, 1.0), c(0x726D67, 1.0), c(0x60CDFF, 1.0), c(0xFFFFFF, 0.07), c(0xFFFFFF, 0.07), 0.62),
];
const THEME_COUNT: usize = 9;
fn themes() -> &'static mut [Theme; THEME_COUNT] {
    unsafe { &mut *std::ptr::addr_of_mut!(THEMES) }
}

fn lerp_c(a: D2D1_COLOR_F, b: D2D1_COLOR_F, t: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}
fn lerp_theme(a: &Theme, b: &Theme, t: f32) -> Theme {
    Theme {
        name: b.name,
        tint_top: lerp_c(a.tint_top, b.tint_top, t),
        tint_bottom: lerp_c(a.tint_bottom, b.tint_bottom, t),
        border_top: lerp_c(a.border_top, b.border_top, t),
        border_bottom: lerp_c(a.border_bottom, b.border_bottom, t),
        text: lerp_c(a.text, b.text, t),
        dim: lerp_c(a.dim, b.dim, t),
        ghost: lerp_c(a.ghost, b.ghost, t),
        accent: lerp_c(a.accent, b.accent, t),
        highlight: lerp_c(a.highlight, b.highlight, t),
        divider: lerp_c(a.divider, b.divider, t),
        shadow: a.shadow + (b.shadow - a.shadow) * t,
    }
}
fn with_a(mut c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    c.a *= a;
    c
}

// ---------------------------------------------------------------------------
// Commands ("> ...")
// ---------------------------------------------------------------------------
struct CmdDef {
    key: &'static str,
    title: &'static str,
    glyph: &'static str, // Segoe Fluent Icons / MDL2
}
const CMDS: [CmdDef; 9] = [
    CmdDef { key: "games", title: "Games", glyph: "\u{E7FC}" },
    CmdDef { key: "pins", title: "Pinned apps", glyph: "\u{E718}" },
    CmdDef { key: "theme", title: "Theme", glyph: "\u{E790}" },
    CmdDef { key: "caret", title: "Caret", glyph: "\u{E70F}" },
    CmdDef { key: "font", title: "Font", glyph: "\u{E8D2}" },
    CmdDef { key: "fontfolder", title: "Open font folder", glyph: "\u{E838}" },
    CmdDef { key: "expanded", title: "Expanded results", glyph: "\u{E8FD}" },
    CmdDef { key: "startup", title: "Start with Windows", glyph: "\u{E7E8}" },
    CmdDef { key: "quit", title: "Quit Custom Start", glyph: "\u{E711}" },
];
const CMD_GAMES: usize = 0;
const CMD_PINS: usize = 1;
const CMD_THEME: usize = 2;
const CMD_CARET: usize = 3;
const CMD_FONT: usize = 4;
const CMD_FONTFOLDER: usize = 5;
const CMD_EXPANDED: usize = 6;
const CMD_STARTUP: usize = 7;
const CMD_QUIT: usize = 8;

const CLASS_NAME: PCWSTR = w!("CustomStartMenu.CommandBar");
// Placeholder in the language chosen in Setup (HKCU\Software\CustomStartMenu\Language).
const PLACEHOLDERS: [(&str, &str); 10] = [
    ("en", "Search apps, or type > for commands"),
    ("es", "Busca apps o escribe > para comandos"),
    ("fr", "Rechercher des applis, ou tapez > pour les commandes"),
    ("de", "Apps suchen oder > für Befehle eingeben"),
    ("it", "Cerca app o digita > per i comandi"),
    ("pt", "Pesquise apps ou digite > para comandos"),
    ("ru", "Поиск приложений или > для команд"),
    ("ja", "アプリを検索、または > でコマンド"),
    ("zh", "搜索应用，或输入 > 使用命令"),
    ("ko", "앱 검색 또는 > 를 입력해 명령 사용"),
];
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_NAME: PCWSTR = w!("CustomStartMenu");
const SETTINGS_KEY: PCWSTR = w!("Software\\CustomStartMenu");
const INJECT_MAGIC: usize = 0x43534D31; // marks our own injected keys
const BACKDROP_SCALE: i32 = 4; // backdrop is blurred at 1/4 resolution

const WM_APP_TOGGLE: u32 = WM_APP + 1;
const WM_APP_TRAY: u32 = WM_APP + 2;
const WM_APP_APPS: u32 = WM_APP + 3;
const WM_APP_HIDE: u32 = WM_APP + 4;
const WM_APP_QUIT: u32 = WM_APP + 5; // sent by CSMInstaller before replacing the exe
const WM_APP_GAMES: u32 = WM_APP + 6;
const WM_APP_PINS: u32 = WM_APP + 7;
const WM_APP_FONT: u32 = WM_APP + 8; // a font finished downloading
const WM_APP_PANEL: u32 = WM_APP + 9; // a panel's data changed (wp 1: currency rates, 2: a switch failed)
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_UPDATE_CHECK: usize = 1;
const TIMER_TICK: usize = 2; // a running timer

const ID_OPEN: usize = 1;
const ID_AUTOSTART: usize = 2;
const ID_INTERCEPT: usize = 3;
const ID_EXPANDED: usize = 4;
const ID_EXIT: usize = 5;
const ID_THEME0: usize = 100;

// Results, Games and Pins are all lists of apps (State::results).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    None,
    Results,
    Commands,
    Themes,
    Games,
    Pins,
    Fonts,
    Caret,
    Panel,
}
fn is_app_list(m: Mode) -> bool {
    matches!(m, Mode::Results | Mode::Games | Mode::Pins)
}

// Right-click menu actions
const MA_ADMIN: usize = 0;
const MA_LOCATION: usize = 1;
const MA_PIN_START: usize = 2;
const MA_UNPIN_START: usize = 3;
const MA_PIN_TASKBAR: usize = 4;
const MA_HIDE: usize = 5;

// ---------------------------------------------------------------------------
// UTF-16 string helpers (text is kept as UTF-16 so indices match DirectWrite)
// ---------------------------------------------------------------------------
type WStr = Vec<u16>;

fn ws(s: &str) -> WStr {
    s.encode_utf16().collect()
}
fn wz(s: &str) -> WStr {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
fn is_space(c: u16) -> bool {
    char::from_u32(c as u32).map(|ch| ch.is_whitespace()).unwrap_or(false)
}
fn lower(s: &[u16]) -> WStr {
    let mut v = s.to_vec();
    if !v.is_empty() {
        unsafe { CharLowerBuffW(&mut v) };
    }
    v
}
fn trim(s: &[u16]) -> WStr {
    let mut a = 0;
    let mut b = s.len();
    while a < b && is_space(s[a]) {
        a += 1;
    }
    while b > a && is_space(s[b - 1]) {
        b -= 1;
    }
    s[a..b].to_vec()
}
fn find(h: &[u16], n: &[u16]) -> Option<usize> {
    if n.is_empty() {
        return Some(0);
    }
    if n.len() > h.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| &h[i..i + n.len()] == n)
}
fn starts_with(s: &[u16], p: &[u16]) -> bool {
    s.len() >= p.len() && &s[..p.len()] == p
}
fn initials(lower: &[u16]) -> WStr {
    let mut r = Vec::new();
    let mut start = true;
    for &c in lower {
        let sep = is_space(c) || c == b'-' as u16 || c == b'_' as u16 || c == b'.' as u16 || c == b'(' as u16;
        if sep {
            start = true;
            continue;
        }
        if start {
            r.push(c);
        }
        start = false;
    }
    r
}
fn is_high(c: u16) -> bool {
    (0xD800..=0xDBFF).contains(&c)
}
fn is_low(c: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&c)
}

// ---------------------------------------------------------------------------
// Math
// ---------------------------------------------------------------------------
fn clamp01(v: f64) -> f32 {
    v.clamp(0.0, 1.0) as f32
}
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}
fn ease_out_quart(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(4)
}
fn ease_out_back(t: f64) -> f64 {
    let c1 = 1.45;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}
fn damp(cur: f32, target: f32, speed: f32, dt: f32) -> f32 {
    cur + (target - cur) * (1.0 - (-speed * dt).exp())
}

fn m_identity() -> Matrix3x2 {
    Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 }
}
fn m_translate(x: f32, y: f32) -> Matrix3x2 {
    Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: x, M32: y }
}
fn m_scale(sx: f32, sy: f32, cx: f32, cy: f32) -> Matrix3x2 {
    Matrix3x2 { M11: sx, M12: 0.0, M21: 0.0, M22: sy, M31: cx - sx * cx, M32: cy - sy * cy }
}
fn m_mul(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}
fn m_invert(m: &Matrix3x2) -> Option<Matrix3x2> {
    let det = m.M11 * m.M22 - m.M12 * m.M21;
    if det.abs() < 1e-12 {
        return None;
    }
    let d = 1.0 / det;
    Some(Matrix3x2 {
        M11: m.M22 * d,
        M12: -m.M12 * d,
        M21: -m.M21 * d,
        M22: m.M11 * d,
        M31: (m.M21 * m.M32 - m.M22 * m.M31) * d,
        M32: (m.M12 * m.M31 - m.M11 * m.M32) * d,
    })
}

fn rect(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: l, top: t, right: r, bottom: b }
}
fn rrect(r: D2D_RECT_F, rx: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: r, radiusX: rx, radiusY: rx }
}
fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}
fn pixel_format() -> D2D1_PIXEL_FORMAT {
    D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------
static mut QPF: f64 = 1.0;
fn now() -> f64 {
    let mut t = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut t);
        t as f64 / QPF
    }
}

// ---------------------------------------------------------------------------
// Shared (cross-thread) state
// ---------------------------------------------------------------------------
// target is one of: an AppsFolder parsing name (launched via shell:AppsFolder\...), an absolute
// file path (exe, .lnk, .url) or a URL (steam://...). icon, when set, is a file to take the icon
// from instead of the target. alias is the program's file name ("cmd" for Command Prompt).
#[derive(Clone, Default)]
struct App {
    name: WStr,
    lower: WStr,
    initials: WStr,
    target: WStr,
    alias: WStr,
    icon: WStr,
    detail: WStr,
}
fn is_url_target(t: &[u16]) -> bool {
    find(t, &ws("://")).is_some()
}
fn is_path_target(t: &[u16]) -> bool {
    (t.len() > 2 && t[1] == b':' as u16 && t[2] == b'\\' as u16) || starts_with(t, &ws("\\\\"))
}
fn is_packaged_target(t: &[u16]) -> bool {
    !is_url_target(t) && !is_path_target(t) && t.contains(&(b'!' as u16)) && !t.contains(&(b'\\' as u16))
}
fn icon_key_of(a: &App) -> &WStr {
    if a.icon.is_empty() {
        &a.target
    } else {
        &a.icon
    }
}

// Gaussian-blurred rounded rectangle, drawn as a nine-slice so it fits any size.
struct Shadow {
    bmp: Option<ID2D1Bitmap>,
    dpi: f32,
    sigma: f32,
    radius: f32,
    offset_y: f32,
    c: i32,
    n: i32,
    pad: i32,
}
impl Shadow {
    const fn new(sigma: f32, radius: f32, offset_y: f32) -> Self {
        Shadow { bmp: None, dpi: 0.0, sigma, radius, offset_y, c: 0, n: 0, pad: 0 }
    }
}

static HWND_G: AtomicIsize = AtomicIsize::new(0);
fn hwnd() -> HWND {
    HWND(HWND_G.load(Relaxed) as *mut c_void)
}
static QUIT_EVENT: AtomicIsize = AtomicIsize::new(0);

static APPS: RwLock<Vec<App>> = RwLock::new(Vec::new());
static APPS_LOADING: AtomicBool = AtomicBool::new(false);
static START_RECTS: RwLock<Vec<RECT>> = RwLock::new(Vec::new());

struct IconData {
    ready: bool,
    px: Vec<u32>,
    w: i32,
    h: i32,
}
struct IconCache {
    queue: VecDeque<String>,
    map: HashMap<String, IconData>,
    quit: bool,
}
static ICONS: Mutex<Option<IconCache>> = Mutex::new(None);
static ICON_CV: Condvar = Condvar::new();

// Keyboard / mouse hook state. The hooks run on their own high-priority thread so that input
// never waits on the UI thread, and only read these atomics. The mouse hook is removed
// entirely while a game or other fullscreen app is in front (unless the bar is open).
const WM_HOOK_SYNC: u32 = WM_APP + 20;
static HOOK_TID: AtomicU32 = AtomicU32::new(0);
static BAR_OPEN: AtomicBool = AtomicBool::new(false);
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static BOX_L: AtomicI32 = AtomicI32::new(0);
static BOX_T: AtomicI32 = AtomicI32::new(0);
static BOX_R: AtomicI32 = AtomicI32::new(0);
static BOX_B: AtomicI32 = AtomicI32::new(0);
static INTERCEPT: AtomicBool = AtomicBool::new(true);
static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static WIN_EMITTED: AtomicBool = AtomicBool::new(false);
static SWALLOW_ESC_UP: AtomicBool = AtomicBool::new(false);
static SWALLOW_CLICK_UP: AtomicBool = AtomicBool::new(false);
static WIN_VK: AtomicU32 = AtomicU32::new(0);
static WIN_FLAGS: AtomicU32 = AtomicU32::new(0);
static WIN_LAST_TIME: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// UI-thread state
// ---------------------------------------------------------------------------
struct State {
    d2d: ID2D1Factory1,
    dw: IDWriteFactory,
    fmt_text: IDWriteTextFormat,
    fmt_hint: IDWriteTextFormat,
    fmt_row: IDWriteTextFormat,
    fmt_row_right: IDWriteTextFormat,
    fmt_glyph: IDWriteTextFormat,
    fmt_header: IDWriteTextFormat,
    fmt_card: IDWriteTextFormat,
    fmt_tile: IDWriteTextFormat,
    fmt_menu: IDWriteTextFormat,
    fmt_glyph_sm: IDWriteTextFormat,
    fmt_big: IDWriteTextFormat,
    fmt_big_c: IDWriteTextFormat,
    fmt_mid: IDWriteTextFormat,
    fmt_small: IDWriteTextFormat,
    fmt_small_c: IDWriteTextFormat,
    fmt_small_r: IDWriteTextFormat,
    fmt_hint_l: IDWriteTextFormat,
    fmt_emoji: IDWriteTextFormat,
    placeholder_layout: Option<IDWriteTextLayout>,

    // Rendering backends. Hardware acceleration (the default, chosen in Setup) draws on the
    // GPU into a DirectComposition swap chain, so nothing is copied back to the CPU. Without
    // it, Direct2D renders in software into a layered window and never touches the GPU.
    hw: bool,
    rt: Option<ID2D1RenderTarget>, // the active target
    dcrt: Option<ID2D1DCRenderTarget>,
    d3d: Option<ID3D11Device>,
    d2d_dev: Option<ID2D1Device>,
    ctx: Option<ID2D1DeviceContext>,
    swap: Option<IDXGISwapChain1>,
    target: Option<ID2D1Bitmap1>,
    dcomp: Option<IDCompositionDevice>,
    dc_target: Option<IDCompositionTarget>,
    dc_visual: Option<IDCompositionVisual>,
    win_h: i32,
    solid: Option<ID2D1SolidColorBrush>,
    logo_brush: Option<ID2D1LinearGradientBrush>,
    bar_shadow: Shadow,
    tile_shadow: Shadow,

    bd_px: Vec<u32>,
    bd_w: i32,
    bd_h: i32,
    bd_dirty: bool,
    bd_bmp: Option<ID2D1Bitmap>,
    bd_brush: Option<ID2D1BitmapBrush>,

    mem_dc: HDC,
    dib: HBITMAP,
    old_bmp: HGDIOBJ,
    px_w: i32,
    px_h: i32,
    dpi: f32,
    win_pos: POINT,

    theme_idx: usize,
    expanded: bool,
    autostart: bool,

    preview_idx: usize,
    th: Theme,
    th_from: Theme,
    th_start: f64,

    visible: bool,
    anim: f64,
    anim_from: f64,
    anim_to: f64,
    anim_start: f64,
    anim_dur: f64,
    show_time: f64,
    last_frame: f64,
    last_hide_time: f64,

    text: WStr,
    birth: Vec<f64>,
    caret: usize,
    anchor: usize,
    last_input: f64,
    caret_x: f32,
    trail_x: f32,
    scroll: f32,
    scroll_target: f32,
    placeholder_a: f32,
    caret_init: bool,

    mode: Mode,
    draw_mode: Mode,
    sel: i32,
    panel_h: f32,
    content_a: f32,
    sel_y: f32,
    sel_y_init: bool,
    card_sel: [f32; THEME_COUNT],
    hits: Vec<(D2D_RECT_F, i32)>,
    last_mouse: POINT,

    apps_loaded_at: f64,
    results: Vec<App>,
    cmd_list: Vec<usize>,
    has_match: bool,
    match_app: App,
    ghost: WStr,
    hint_name: WStr,
    hint_time: f64,

    icon_px: i32,
    tile_px: i32, // icon size on the pinned tiles
    icon_bmps: HashMap<String, (ID2D1Bitmap, f64)>,

    // pinned apps (tiles above the bar, in this order)
    pins: Vec<Pin>,
    pin_row_off: i32, // first pin row shown (mouse wheel pages through more)
    press_pin: i32,   // pin under the left button
    dragging: bool,
    press_pt: D2D_POINT_2F,
    drag_pt: D2D_POINT_2F,
    grab: D2D_POINT_2F,
    hover_pin: i32,
    pin_hits: Vec<(D2D_RECT_F, i32)>,
    tracking_leave: bool,

    games_at: f64,

    // list scrolling (games can be longer than the panel)
    list_off: f32,
    sel_row: f32,
    list_top: i32,

    // right-click menu
    menu: Vec<(usize, &'static str, &'static str)>, // action, glyph, text
    menu_open: bool,
    menu_app: App,
    menu_pin: i32, // pin index when opened on a tile
    menu_pos: D2D_POINT_2F,
    menu_time: f64,
    menu_sel: i32,
    menu_hl: f32,
    menu_hl_init: bool,

    // focus to give back when the bar was opened over a game and is closed with Esc / Win
    prev_fg: HWND,

    fonts: fonts::FontUi, // "> font"
    cfx: caret::CaretUi,  // "> caret"
    sb: grid::ScrollBar,  // the panel's scroll bar
}

static mut ST: *mut State = null_mut();
#[inline]
fn st() -> &'static mut State {
    unsafe { &mut *ST }
}
fn rt() -> &'static ID2D1RenderTarget {
    st().rt.as_ref().unwrap()
}
fn solid() -> &'static ID2D1SolidColorBrush {
    st().solid.as_ref().unwrap()
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------
fn read_dword(name: &str, def: u32) -> u32 {
    let mut v = 0u32;
    let mut sz = 4u32;
    let n = wz(name);
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            SETTINGS_KEY,
            PCWSTR(n.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut c_void),
            Some(&mut sz),
        )
    };
    if r == ERROR_SUCCESS {
        v
    } else {
        def
    }
}
fn write_dword(name: &str, v: u32) {
    unsafe {
        let mut k = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, SETTINGS_KEY, 0, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None)
            == ERROR_SUCCESS
        {
            let n = wz(name);
            let _ = RegSetValueExW(k, PCWSTR(n.as_ptr()), 0, REG_DWORD, Some(&v.to_le_bytes()));
            let _ = RegCloseKey(k);
        }
    }
}
fn write_string(name: &str, v: &str) {
    unsafe {
        let mut k = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, SETTINGS_KEY, 0, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None)
            == ERROR_SUCCESS
        {
            let n = wz(name);
            let data: Vec<u8> = wz(v).iter().flat_map(|c| c.to_le_bytes()).collect();
            let _ = RegSetValueExW(k, PCWSTR(n.as_ptr()), 0, REG_SZ, Some(&data));
            let _ = RegCloseKey(k);
        }
    }
}
fn read_string(key: PCWSTR, name: PCWSTR) -> Option<String> {
    let mut buf = [0u16; 512];
    let mut sz = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut sz))
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}
fn is_auto_start() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME, RRF_RT_REG_SZ, None, None, None) == ERROR_SUCCESS }
}
fn set_auto_start(on: bool) {
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_SET_VALUE, &mut k) != ERROR_SUCCESS {
            return;
        }
        if on {
            let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            let v = wz(&format!("\"{}\"", exe));
            let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
            let _ = RegSetValueExW(k, RUN_NAME, 0, REG_SZ, Some(bytes));
        } else {
            let _ = RegDeleteValueW(k, RUN_NAME);
        }
        let _ = RegCloseKey(k);
    }
    st().autostart = is_auto_start();
}

fn set_preview(idx: usize) {
    let s = st();
    let idx = idx.min(THEME_COUNT - 1);
    if idx == s.preview_idx {
        return;
    }
    s.th_from = s.th;
    s.preview_idx = idx;
    s.th_start = now();
}
fn apply_theme(idx: usize) {
    let s = st();
    s.theme_idx = idx.min(THEME_COUNT - 1);
    write_dword("Theme", s.theme_idx as u32);
    if s.visible {
        set_preview(s.theme_idx);
    } else {
        s.preview_idx = s.theme_idx;
        s.th = themes()[s.theme_idx];
        s.th_from = s.th;
    }
}

// ---------------------------------------------------------------------------
// App index (shell:AppsFolder covers both Win32 and Store apps)
// ---------------------------------------------------------------------------
fn pwstr_to_vec(p: PWSTR) -> WStr {
    if p.is_null() {
        return Vec::new();
    }
    unsafe { p.as_wide().to_vec() }
}

fn load_apps() -> Vec<App> {
    let mut apps = Vec::new();
    unsafe {
        let folder: IShellItem = match SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, HANDLE::default()) {
            Ok(f) => f,
            Err(_) => return apps,
        };
        let en: IEnumShellItems = match folder.BindToHandler(None::<&IBindCtx>, &BHID_EnumItems) {
            Ok(e) => e,
            Err(_) => return apps,
        };
        loop {
            let mut items: [Option<IShellItem>; 1] = [None];
            if en.Next(&mut items, None).is_err() {
                break;
            }
            let Some(it) = items[0].take() else { break };
            let name = it.GetDisplayName(SIGDN_NORMALDISPLAY);
            let parse = it.GetDisplayName(SIGDN_PARENTRELATIVEPARSING);
            if let (Ok(n), Ok(p)) = (&name, &parse) {
                let nv = pwstr_to_vec(*n);
                if !nv.is_empty() {
                    let lw = lower(&nv);
                    let target = pwstr_to_vec(*p);
                    // "{System32}\cmd.exe" -> "cmd", so typing the program's name finds it
                    let lt = lower(&target);
                    let mut alias = Vec::new();
                    if lt.len() > 4 && lt.ends_with(&ws(".exe")) {
                        let sl = lt.iter().rposition(|&c| c == b'\\' as u16 || c == b'/' as u16).map(|i| i + 1).unwrap_or(0);
                        alias = lt[sl..lt.len() - 4].to_vec();
                        if alias == lw {
                            alias.clear();
                        }
                    }
                    apps.push(App { initials: initials(&lw), lower: lw, name: nv, target, alias, ..Default::default() });
                }
            }
            if let Ok(n) = name {
                CoTaskMemFree(Some(n.0 as *const c_void));
            }
            if let Ok(p) = parse {
                CoTaskMemFree(Some(p.0 as *const c_void));
            }
        }
    }
    apps.sort_by(|a, b| a.lower.cmp(&b.lower));
    apps.dedup_by(|a, b| a.lower == b.lower);
    apps
}

fn refresh_apps(force: bool) {
    let t = now();
    if !force && t - st().apps_loaded_at < 30.0 {
        return;
    }
    if APPS_LOADING.compare_exchange(false, true, SeqCst, SeqCst).is_err() {
        return;
    }
    st().apps_loaded_at = t;
    std::thread::spawn(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let apps = load_apps();
        *APPS.write().unwrap() = apps;
        APPS_LOADING.store(false, SeqCst);
        unsafe {
            let h = hwnd();
            if !h.is_invalid() {
                let _ = PostMessageW(h, WM_APP_APPS, WPARAM(0), LPARAM(0));
            }
            CoUninitialize();
        }
    });
}

fn score(a: &App, q: &[u16]) -> i32 {
    let base;
    let alias_hit = !a.alias.is_empty() && a.alias.as_slice() == q;
    let alias_prefix = !a.alias.is_empty() && q.len() >= 2 && starts_with(&a.alias, q);
    match find(&a.lower, q) {
        _ if alias_hit => base = 3600, // "cmd" -> Command Prompt
        Some(0) => base = 3000,
        _ if alias_prefix => base = 2500,
        Some(p) => {
            let c = a.lower[p - 1];
            base = if is_space(c) || c == b'-' as u16 || c == b'(' as u16 || c == b'.' as u16 || c == b'_' as u16 {
                2000
            } else {
                1000
            };
        }
        None => {
            if q.len() >= 2 && starts_with(&a.initials, q) {
                base = 1500;
            } else {
                return 0;
            }
        }
    }
    let mut s = base - a.lower.len() as i32;
    if find(&a.lower, &ws("uninstall")).is_some()
        || find(&a.lower, &ws("readme")).is_some()
        || find(&a.lower, &ws("documentation")).is_some()
    {
        s -= 800;
    }
    s.max(1)
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------
fn load_icon_pixels(target: &str, size: i32) -> Option<(Vec<u32>, i32, i32)> {
    let tw = ws(target);
    if is_url_target(&tw) {
        return None;
    }
    let path = if is_path_target(&tw) { target.to_string() } else { format!("shell:AppsFolder\\{}", target) };
    let lp = path.to_lowercase();
    let image = lp.ends_with(".jpg") || lp.ends_with(".png");
    unsafe {
        let f: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None::<&IBindCtx>).ok()?;
        let hb = f.GetImage(SIZE { cx: size, cy: size }, if image { SIIGBF_RESIZETOFIT } else { SIIGBF_ICONONLY }).ok()?;
        let mut bm = BITMAP::default();
        GetObjectW(HGDIOBJ(hb.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void));
        let w = bm.bmWidth;
        let h = bm.bmHeight.abs();
        if w <= 0 || h <= 0 {
            let _ = DeleteObject(HGDIOBJ(hb.0));
            return None;
        }
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;
        let mut px = vec![0u32; (w * h) as usize];
        let dc = GetDC(HWND::default());
        let lines = GetDIBits(dc, hb, 0, h as u32, Some(px.as_mut_ptr() as *mut c_void), &mut bi, DIB_RGB_COLORS);
        ReleaseDC(HWND::default(), dc);
        let _ = DeleteObject(HGDIOBJ(hb.0));
        if lines == 0 {
            return None;
        }
        let mut any_alpha = false;
        let mut straight = false;
        for &p in &px {
            let (a, r, g, b) = (p >> 24, (p >> 16) & 255, (p >> 8) & 255, p & 255);
            if a != 0 {
                any_alpha = true;
            }
            if r > a || g > a || b > a {
                straight = true;
            }
        }
        for p in px.iter_mut() {
            if !any_alpha {
                *p |= 0xFF000000;
                continue;
            }
            if straight {
                let (a, r, g, b) = (*p >> 24, (*p >> 16) & 255, (*p >> 8) & 255, *p & 255);
                *p = (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) | (b * a / 255);
            }
        }
        Some((px, w, h))
    }
}

fn icon_worker() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    loop {
        let key = {
            let mut g = ICONS.lock().unwrap();
            loop {
                let cache = g.as_mut().unwrap();
                if cache.quit {
                    unsafe { CoUninitialize() };
                    return;
                }
                if let Some(k) = cache.queue.pop_front() {
                    break k;
                }
                g = ICON_CV.wait(g).unwrap();
            }
        };
        let bar = key.rfind('|').unwrap_or(key.len());
        let target = &key[..bar];
        let size: i32 = key[bar + 1..].parse().unwrap_or(32);
        let res = load_icon_pixels(target, size);
        let mut g = ICONS.lock().unwrap();
        let e = g.as_mut().unwrap().map.entry(key.clone()).or_insert(IconData { ready: false, px: Vec::new(), w: 0, h: 0 });
        match res {
            Some((px, w, h)) => {
                e.px = px;
                e.w = w;
                e.h = h;
                e.ready = true;
            }
            None => e.ready = false,
        }
    }
}

// Returns the icon bitmap if loaded (and when it first appeared), else queues a load.
fn get_icon(target: &[u16], px: i32) -> Option<(ID2D1Bitmap, f64)> {
    let s = st();
    let key = format!("{}|{}", String::from_utf16_lossy(target), if px > 0 { px } else { s.icon_px });
    if let Some(b) = s.icon_bmps.get(&key) {
        return Some(b.clone());
    }
    let mut g = ICONS.lock().unwrap();
    let cache = g.as_mut().unwrap();
    match cache.map.get(&key) {
        None => {
            cache.map.insert(key.clone(), IconData { ready: false, px: Vec::new(), w: 0, h: 0 });
            cache.queue.push_back(key);
            ICON_CV.notify_one();
            None
        }
        Some(e) if !e.ready => None,
        Some(e) => {
            let rt = s.rt.as_ref()?;
            let bp = D2D1_BITMAP_PROPERTIES { pixelFormat: pixel_format(), dpiX: 0.0, dpiY: 0.0 };
            let bmp = unsafe {
                rt.CreateBitmap(
                    D2D_SIZE_U { width: e.w as u32, height: e.h as u32 },
                    Some(e.px.as_ptr() as *const c_void),
                    (e.w * 4) as u32,
                    &bp,
                )
            }
            .ok()?;
            let v = (bmp, now());
            s.icon_bmps.insert(key, v.clone());
            Some(v)
        }
    }
}

// ---------------------------------------------------------------------------
// State: what the panel shows for the current text
// ---------------------------------------------------------------------------
fn command_detail(c: usize) -> String {
    let s = st();
    match c {
        CMD_PINS => {
            let hidden = s.pins.iter().filter(|p| p.hidden).count();
            if hidden > 0 {
                format!("{} hidden", hidden)
            } else {
                s.pins.len().to_string()
            }
        }
        CMD_FONT => fonts::display_name(&fonts::font_file()),
        CMD_CARET => caret::summary(),
        _ => command_detail_str(c).to_string(),
    }
}
fn command_detail_str(c: usize) -> &'static str {
    let s = st();
    match c {
        CMD_THEME => themes()[s.theme_idx].name,
        CMD_EXPANDED => {
            if s.expanded {
                "On"
            } else {
                "Off"
            }
        }
        CMD_STARTUP => {
            if s.autostart {
                "On"
            } else {
                "Off"
            }
        }
        _ => "",
    }
}

fn recompute_ghost() {
    let s = st();
    s.ghost.clear();
    let lt = lower(&s.text);
    if s.mode == Mode::Commands {
        if s.sel >= 0 && (s.sel as usize) < s.cmd_list.len() {
            let gt = lt.iter().position(|&c| c == b'>' as u16).unwrap_or(0);
            let q = lower(&trim(&s.text[gt + 1..]));
            let key = ws(&panels::cmd_key(s.cmd_list[s.sel as usize]));
            if !q.is_empty() && starts_with(&key, &q) && !lt.is_empty() && !is_space(*lt.last().unwrap()) {
                s.ghost = key[q.len()..].to_vec();
            }
        }
        return;
    }
    let a: Option<&App> = if s.mode == Mode::Results && s.sel >= 0 && (s.sel as usize) < s.results.len() {
        Some(&s.results[s.sel as usize])
    } else if s.mode == Mode::None && s.has_match {
        Some(&s.match_app)
    } else {
        None
    };
    if let Some(a) = a {
        if !lt.is_empty() && starts_with(&a.lower, &lt) && lt.len() < a.name.len() {
            s.ghost = a.name[s.text.len()..].to_vec();
        }
    }
}

fn update_state() {
    let s = st();
    let old = s.mode;
    s.results.clear();
    s.cmd_list.clear();
    s.has_match = false;
    let pst = panels::ps();
    pst.inline_copy.clear();
    pst.inline_url.clear();
    let mut new_panel = -1;
    let mut inline_hint = String::new();

    let tl = lower(&trim(&s.text));
    if !tl.is_empty() && tl[0] == b'>' as u16 {
        let q = trim(&tl[1..]);
        let qs = String::from_utf16_lossy(&q);
        let qo = String::from_utf16_lossy(&trim(&trim(&s.text)[1..]));
        let word = |w: &str| q == ws(w) || starts_with(&q, &ws(&format!("{} ", w)));
        let pm = panels::panel_match(&qs, &qo);
        let um = if pm.is_none() { tools::user_cmd_match(&qs, &qo) } else { None };
        if pm.is_some() && qs.starts_with("theme ") {
            // "> theme new" beats the theme picker
            let (i, a) = pm.clone().unwrap();
            s.mode = Mode::Panel;
            new_panel = i as i32;
            pst.arg = a;
        } else if q == ws("theme") || q == ws("themes") || starts_with(&q, &ws("theme ")) {
            s.mode = Mode::Themes;
        } else if word("font") || word("fonts") {
            s.mode = Mode::Fonts;
            let skip = if word("fonts") { 5 } else { 4 };
            fonts::build_list(&String::from_utf16_lossy(&trim(&q[skip.min(q.len())..])));
        } else if word("caret") {
            s.mode = Mode::Caret;
            caret::build_list(&String::from_utf16_lossy(&trim(&q[5.min(q.len())..])));
        } else if word("games") || word("game") {
            s.mode = Mode::Games;
            let skip = if word("games") { 5 } else { 4 };
            let f = trim(&q[find(&q, &ws("game")).unwrap_or(0) + skip..]);
            refresh_games();
            let games = GAMES.read().unwrap();
            if f.is_empty() {
                s.results = games.clone();
            } else {
                let mut scored: Vec<(i32, usize)> =
                    games.iter().enumerate().filter_map(|(i, a)| { let sc = score(a, &f); (sc > 0).then_some((sc, i)) }).collect();
                scored.sort_by(|a, b| b.0.cmp(&a.0));
                s.results = scored.iter().map(|&(_, i)| games[i].clone()).collect();
            }
        } else if word("pins") || word("pinned") {
            s.mode = Mode::Pins;
            for p in &s.pins {
                let mut a = p.app.clone();
                a.detail = ws(if p.hidden { "Hidden" } else { "Shown" });
                s.results.push(a);
            }
        } else if let Some((i, a)) = pm {
            s.mode = Mode::Panel;
            new_panel = i as i32;
            pst.arg = a;
        } else if let Some((i, a)) = um {
            s.mode = Mode::Panel;
            new_panel = pst.run_panel;
            pst.user_cmd = i as i32;
            pst.arg = a;
        } else {
            s.mode = Mode::Commands;
            panels::cmd_list_fill(&qs);
        }
    } else if !tl.is_empty() {
        let apps = APPS.read().unwrap();
        let mut scored: Vec<(i32, usize)> =
            apps.iter().enumerate().filter_map(|(i, a)| { let sc = score(a, &tl); (sc > 0).then_some((sc, i)) }).collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        for &(_, i) in scored.iter().take(MAX_ROWS) {
            s.results.push(apps[i].clone());
        }
        drop(apps);
        s.has_match = !s.results.is_empty();
        if s.has_match {
            s.match_app = s.results[0].clone();
        }
        s.mode = if s.expanded && s.has_match { Mode::Results } else { Mode::None };
        // math, conversions and web shortcuts answer on the right of the bar
        inline_hint = calc::inline_answer(&String::from_utf16_lossy(&s.text), s.has_match && starts_with(&s.match_app.lower, &tl));
        if !inline_hint.is_empty() {
            s.mode = Mode::None;
        }
    } else {
        s.mode = Mode::None;
    }

    let panel_changed = s.mode == Mode::Panel && old == Mode::Panel && new_panel != pst.panel;
    if old == Mode::Panel && (s.mode != Mode::Panel || panel_changed) {
        panels::leave();
    }
    if s.mode == Mode::Panel {
        pst.panel = new_panel;
    }
    if s.mode != old || panel_changed {
        if old == Mode::Themes {
            set_preview(s.theme_idx); // leaving without applying reverts the preview
        }
        s.sel = if s.mode == Mode::Themes { s.theme_idx as i32 } else { 0 };
        if s.mode == Mode::Fonts {
            fonts::enter();
        }
        if s.mode == Mode::Caret {
            caret::enter();
        }
        if s.mode != Mode::None {
            s.content_a = if old == Mode::None { 0.0 } else { s.content_a.min(0.35) };
            s.draw_mode = s.mode;
            s.sel_y_init = false;
        }
        s.list_top = 0;
        s.list_off = 0.0;
    } else if is_app_list(s.mode) || s.mode == Mode::Commands || s.mode == Mode::Panel {
        s.sel = 0;
        s.list_top = 0;
    } else if s.mode == Mode::Fonts {
        s.sel = 0;
        fonts::scroll_to(0.0, true);
    } else if s.mode == Mode::Caret {
        caret::enter();
    }
    if s.mode == Mode::Panel {
        panels::fill(true);
    }
    recompute_ghost();

    let mut hint = ws(&inline_hint);
    if hint.is_empty() && s.mode == Mode::None && s.has_match && !starts_with(&s.match_app.lower, &lower(&s.text)) {
        hint = s.match_app.name.clone();
    }
    if hint != s.hint_name {
        s.hint_name = hint;
        s.hint_time = now();
    }
}

fn item_count() -> i32 {
    let s = st();
    match s.mode {
        Mode::Results | Mode::Games | Mode::Pins => s.results.len() as i32,
        Mode::Commands => s.cmd_list.len() as i32,
        Mode::Themes => THEME_COUNT as i32,
        Mode::Fonts => fonts::count(),
        Mode::Caret => caret::count(),
        Mode::Panel => panels::item_count(),
        Mode::None => 0,
    }
}

fn theme_grid_height() -> f32 {
    let rows = THEME_COUNT.div_ceil(GRID_COLS) as f32;
    MAX_PANEL_H.min(GRID_PAD + GRID_HEADER_H + rows * (CARD_PREVIEW_H + CARD_NAME_H) + (rows - 1.0) * 4.0 + GRID_PAD - 4.0)
}

fn panel_target_h() -> f32 {
    let s = st();
    match s.mode {
        Mode::Results => PANEL_PAD * 2.0 + ROW_H * s.results.len() as f32,
        // an empty list still shows one line ("Looking for games...")
        Mode::Games | Mode::Pins => PANEL_PAD * 2.0 + ROW_H * s.results.len().clamp(1, MAX_ROWS) as f32,
        Mode::Commands => {
            if s.cmd_list.is_empty() {
                0.0
            } else {
                PANEL_PAD * 2.0 + ROW_H * s.cmd_list.len().min(MAX_ROWS) as f32
            }
        }
        Mode::Themes => theme_grid_height(),
        Mode::Fonts => fonts::panel_h(),
        Mode::Caret => caret::panel_h(),
        Mode::Panel => panels::height(),
        Mode::None => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Start button location (UI Automation, polled on a worker thread)
// ---------------------------------------------------------------------------
unsafe extern "system" fn enum_taskbars(h: HWND, lp: LPARAM) -> BOOL {
    let mut cls = [0u16; 64];
    let n = GetClassNameW(h, &mut cls) as usize;
    let name = String::from_utf16_lossy(&cls[..n]);
    if name == "Shell_TrayWnd" || name == "Shell_SecondaryTrayWnd" {
        (*(lp.0 as *mut Vec<isize>)).push(h.0 as isize);
    }
    TRUE
}

// A game or other fullscreen app is in front (exclusive or borderless fullscreen).
fn foreground_is_fullscreen() -> bool {
    unsafe {
        if let Ok(qs) = SHQueryUserNotificationState() {
            if qs == QUNS_RUNNING_D3D_FULL_SCREEN || qs == QUNS_BUSY || qs == QUNS_PRESENTATION_MODE {
                return true;
            }
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == hwnd() || fg == GetShellWindow() || fg == GetDesktopWindow() {
            return false;
        }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls) as usize;
        let name = String::from_utf16_lossy(&cls[..n]);
        if matches!(name.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return false;
        }
        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 == WS_CAPTION.0 {
            return false; // a normal maximized window
        }
        let mut r = RECT::default();
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetWindowRect(fg, &mut r).is_err() || !GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

// Background watcher: notices games / fullscreen apps once a second, and (only when none is
// running) refreshes where the Start button is.
fn start_button_thread() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: Option<IUIAutomation> = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok();
        let cond = uia.as_ref().and_then(|u| {
            let v = VARIANT::from(windows::core::BSTR::from("StartButton"));
            u.CreatePropertyCondition(UIA_AutomationIdPropertyId, &v).ok()
        });
        let mut since_query = 1000;
        loop {
            let fs = foreground_is_fullscreen();
            if fs != FULLSCREEN.load(Relaxed) {
                FULLSCREEN.store(fs, Relaxed);
                post_hook_sync();
                if !fs {
                    since_query = 1000; // the taskbar may have moved meanwhile
                }
            }
            since_query += 1;
            if let (false, Some(uia), Some(cond), true) = (fs, &uia, &cond, since_query >= 3) {
                since_query = 0;
                let mut bars: Vec<isize> = Vec::new();
                let _ = EnumWindows(Some(enum_taskbars), LPARAM(&mut bars as *mut _ as isize));
                let mut rects = Vec::new();
                for h in bars {
                    if let Ok(root) = uia.ElementFromHandle(HWND(h as *mut c_void)) {
                        if let Ok(el) = root.FindFirst(TreeScope_Descendants, cond) {
                            if let Ok(r) = el.CurrentBoundingRectangle() {
                                if r.right > r.left && r.bottom > r.top {
                                    rects.push(r);
                                }
                            }
                        }
                    }
                }
                *START_RECTS.write().unwrap() = rects;
            }
            let ev = HANDLE(QUIT_EVENT.load(Relaxed) as *mut c_void);
            if WaitForSingleObject(ev, 1000) != WAIT_TIMEOUT {
                break;
            }
        }
        drop(cond);
        drop(uia);
        CoUninitialize();
    }
}

fn pt_in(r: &RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

fn point_on_start_button(p: POINT) -> bool {
    START_RECTS.read().map(|v| v.iter().any(|r| pt_in(r, p))).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Low level hooks
// ---------------------------------------------------------------------------
unsafe extern "system" fn kbd_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && INTERCEPT.load(Relaxed) {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        if k.dwExtraInfo != INJECT_MAGIC {
            let m = wp.0 as u32;
            let down = m == WM_KEYDOWN || m == WM_SYSKEYDOWN;
            let vk = k.vkCode;
            if vk == VK_LWIN.0 as u32 || vk == VK_RWIN.0 as u32 {
                if down {
                    // New press (not auto-repeat)? Also recovers if a release was never seen
                    // (e.g. Win+L switches to the secure desktop before the key comes up).
                    if !WIN_DOWN.load(Relaxed) || k.time.wrapping_sub(WIN_LAST_TIME.load(Relaxed)) > 600 {
                        WIN_DOWN.store(true, Relaxed);
                        WIN_EMITTED.store(false, Relaxed);
                        WIN_VK.store(vk, Relaxed);
                        WIN_FLAGS.store(k.flags.0, Relaxed);
                    }
                    WIN_LAST_TIME.store(k.time, Relaxed);
                    // Hold the Win key back; the shell never sees a lone Win press, so the
                    // Start menu never opens. It is replayed if a shortcut key follows.
                    if !WIN_EMITTED.load(Relaxed) {
                        return LRESULT(1);
                    }
                } else if WIN_DOWN.load(Relaxed) {
                    let emitted = WIN_EMITTED.load(Relaxed);
                    WIN_DOWN.store(false, Relaxed);
                    if !emitted {
                        let _ = PostMessageW(hwnd(), WM_APP_TOGGLE, WPARAM(0), LPARAM(0));
                        return LRESULT(1);
                    }
                }
            } else if down && WIN_DOWN.load(Relaxed) && !WIN_EMITTED.load(Relaxed) {
                // Win + <key> shortcut (Win+E, Win+R, Win+Shift+S...): replay Win-down, then this key.
                let ext = |f: u32| if f & LLKHF_EXTENDED.0 != 0 { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) };
                let inputs = [
                    INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(WIN_VK.load(Relaxed) as u16),
                                wScan: 0,
                                dwFlags: ext(WIN_FLAGS.load(Relaxed)),
                                time: 0,
                                dwExtraInfo: INJECT_MAGIC,
                            },
                        },
                    },
                    INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(vk as u16),
                                wScan: k.scanCode as u16,
                                dwFlags: ext(k.flags.0),
                                time: 0,
                                dwExtraInfo: INJECT_MAGIC,
                            },
                        },
                    },
                ];
                WIN_EMITTED.store(true, Relaxed);
                if SendInput(&inputs, size_of::<INPUT>() as i32) == 2 {
                    return LRESULT(1);
                }
            } else if down {
                if vk == VK_ESCAPE.0 as u32
                    && GetAsyncKeyState(VK_CONTROL.0 as i32) < 0
                    && GetAsyncKeyState(VK_MENU.0 as i32) >= 0
                    && GetAsyncKeyState(VK_SHIFT.0 as i32) >= 0
                {
                    SWALLOW_ESC_UP.store(true, Relaxed); // Ctrl+Esc also opens Start
                    let _ = PostMessageW(hwnd(), WM_APP_TOGGLE, WPARAM(0), LPARAM(0));
                    return LRESULT(1);
                }
            } else if vk == VK_ESCAPE.0 as u32 && SWALLOW_ESC_UP.load(Relaxed) {
                SWALLOW_ESC_UP.store(false, Relaxed);
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wp, lp)
}

// Called on the UI thread whenever the bar moves or resizes.
fn menu_h() -> f32 {
    MENU_PAD * 2.0 + MENU_ROW_H * st().menu.len() as f32
}

fn publish_box() {
    let s = st();
    let sc = s.dpi / 96.0;
    // bar space -> window: y + PINS_MAX_H
    let (mut l, mut t, mut r, mut b) = (MARGIN, MARGIN - pins_area_h(), MARGIN + bar_w(), MARGIN + BAR_H + s.panel_h);
    if s.menu_open {
        l = l.min(s.menu_pos.x);
        r = r.max(s.menu_pos.x + MENU_W);
        t = t.min(s.menu_pos.y);
        b = b.max(s.menu_pos.y + menu_h());
    }
    BOX_L.store(s.win_pos.x + (l * sc) as i32, Relaxed);
    BOX_T.store(s.win_pos.y + ((t + PINS_MAX_H) * sc) as i32, Relaxed);
    BOX_R.store(s.win_pos.x + (r * sc) as i32, Relaxed);
    BOX_B.store(s.win_pos.y + ((b + PINS_MAX_H) * sc) as i32, Relaxed);
}

fn post_hook_sync() {
    let tid = HOOK_TID.load(Relaxed);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_HOOK_SYNC, WPARAM(0), LPARAM(0));
        }
    }
}

fn set_bar_open(open: bool) {
    if BAR_OPEN.swap(open, Relaxed) != open {
        post_hook_sync();
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // Moves are the hot path: let them straight through.
    if code == HC_ACTION as i32 && wp.0 as u32 != WM_MOUSEMOVE {
        let m = &*(lp.0 as *const MSLLHOOKSTRUCT);
        if m.flags & 1 == 0 {
            // not injected
            let msg = wp.0 as u32;
            if INTERCEPT.load(Relaxed) && msg == WM_LBUTTONDOWN && point_on_start_button(m.pt) {
                SWALLOW_CLICK_UP.store(true, Relaxed);
                let _ = PostMessageW(hwnd(), WM_APP_TOGGLE, WPARAM(0), LPARAM(0));
                return LRESULT(1);
            }
            if msg == WM_LBUTTONUP && SWALLOW_CLICK_UP.load(Relaxed) {
                SWALLOW_CLICK_UP.store(false, Relaxed);
                return LRESULT(1);
            }
            // Clicking anywhere outside the bar closes it (even if focus could not be taken).
            if (msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN) && BAR_OPEN.load(Relaxed) {
                let r = RECT { left: BOX_L.load(Relaxed), top: BOX_T.load(Relaxed), right: BOX_R.load(Relaxed), bottom: BOX_B.load(Relaxed) };
                if !pt_in(&r, m.pt) {
                    let _ = PostMessageW(hwnd(), WM_APP_HIDE, WPARAM(0), LPARAM(0));
                }
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wp, lp)
}

// Owns both low-level hooks. Runs at time-critical priority and does almost nothing else, so
// a busy CPU (a game, a compile) never makes the mouse or keyboard wait on Custom Start.
fn sync_mouse_hook(ms: &mut Option<HHOOK>, inst: HINSTANCE) {
    let want = !FULLSCREEN.load(Relaxed) || BAR_OPEN.load(Relaxed);
    unsafe {
        if want && ms.is_none() {
            *ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), inst, 0).ok();
        } else if !want {
            if let Some(h) = ms.take() {
                let _ = UnhookWindowsHookEx(h);
                SWALLOW_CLICK_UP.store(false, Relaxed);
            }
        }
    }
}

fn hook_thread(ready: std::sync::mpsc::Sender<()>) {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_NOREMOVE); // create the message queue
        HOOK_TID.store(GetCurrentThreadId(), Relaxed);
        let inst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).map(|m| HINSTANCE(m.0)).unwrap_or_default();
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kbd_proc), inst, 0).ok();
        let mut ms: Option<HHOOK> = None;
        sync_mouse_hook(&mut ms, inst);
        let _ = ready.send(());
        while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
            if msg.message == WM_HOOK_SYNC {
                sync_mouse_hook(&mut ms, inst);
            }
        }
        if let Some(h) = ms {
            let _ = UnhookWindowsHookEx(h);
        }
        if let Some(h) = kb {
            let _ = UnhookWindowsHookEx(h);
        }
    }
}

// ---------------------------------------------------------------------------
// Graphics resources
// ---------------------------------------------------------------------------
fn font_exists(dw: &IDWriteFactory, family: &str) -> bool {
    unsafe {
        let mut coll: Option<IDWriteFontCollection> = None;
        if dw.GetSystemFontCollection(&mut coll, false).is_err() {
            return false;
        }
        let Some(coll) = coll else { return false };
        let mut idx = 0u32;
        let mut exists = BOOL(0);
        let _ = coll.FindFamilyName(&HSTRING::from(family), &mut idx, &mut exists);
        exists.as_bool()
    }
}

// Fonts live in "<install folder>\Fonts". The one in use is chosen with "> font" (saved as
// "Font"); before that existed, the first font in the folder was used, so that still applies
// while nothing has been chosen.
fn load_custom_font(dw: &IDWriteFactory) -> Option<(IDWriteFontCollection, String)> {
    let dir = fonts::fonts_dir();
    *fonts::FONT_FILE.lock().unwrap() = String::new();
    let file = match read_string(SETTINGS_KEY, w!("Font")) {
        Some(chosen) => {
            if chosen.is_empty() || !std::path::Path::new(&format!("{}\\{}", dir, chosen)).is_file() {
                return None;
            }
            chosen
        }
        None => {
            let mut files: Vec<String> = std::fs::read_dir(&dir)
                .ok()?
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| {
                    let l = n.to_lowercase();
                    l.ends_with(".otf") || l.ends_with(".ttf")
                })
                .collect();
            files.sort();
            files.into_iter().next()?
        }
    };
    let res = unsafe {
        (|| {
            let f5: IDWriteFactory5 = dw.cast().ok()?;
            let b = f5.CreateFontSetBuilder().ok()?;
            let ff = f5.CreateFontFileReference(&HSTRING::from(format!("{}\\{}", dir, file)), None).ok()?;
            b.AddFontFile(&ff).ok()?;
            let set = b.CreateFontSet().ok()?;
            let coll = f5.CreateFontCollectionFromFontSet(&set).ok()?;
            if coll.GetFontFamilyCount() == 0 {
                return None;
            }
            let names = coll.GetFontFamily(0).ok()?.GetFamilyNames().ok()?;
            let (mut idx, mut exists) = (0u32, BOOL(0));
            let _ = names.FindLocaleName(w!("en-us"), &mut idx, &mut exists);
            if !exists.as_bool() {
                idx = 0;
            }
            let len = names.GetStringLength(idx).ok()? as usize;
            let mut buf = vec![0u16; len + 1];
            names.GetString(idx, &mut buf).ok()?;
            let name = String::from_utf16_lossy(&buf[..len]);
            if name.is_empty() {
                return None;
            }
            Some((coll.cast().ok()?, name))
        })()
    };
    if res.is_some() {
        *fonts::FONT_FILE.lock().unwrap() = file;
    }
    res
}

// Set once at startup, before any text format is made (UI thread only).
static mut CUSTOM_FONT: Option<(IDWriteFontCollection, String)> = None;

fn make_format(dw: &IDWriteFactory, fam: &str, size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT, ellipsis: bool) -> IDWriteTextFormat {
    let coll = unsafe { CUSTOM_FONT.as_ref() }.filter(|(_, n)| n == fam).map(|(c, _)| c.clone());
    unsafe {
        let f = dw
            .CreateTextFormat(
                &HSTRING::from(fam),
                coll.as_ref(),
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )
            .expect("CreateTextFormat");
        let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        let _ = f.SetTextAlignment(align);
        if ellipsis {
            let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
            if let Ok(sign) = dw.CreateEllipsisTrimmingSign(&f) {
                let _ = f.SetTrimming(&trim, &sign);
            }
        }
        f
    }
}

struct Formats {
    text: IDWriteTextFormat,
    hint: IDWriteTextFormat,
    row: IDWriteTextFormat,
    row_right: IDWriteTextFormat,
    glyph: IDWriteTextFormat,
    glyph_sm: IDWriteTextFormat,
    header: IDWriteTextFormat,
    card: IDWriteTextFormat,
    tile: IDWriteTextFormat,
    menu: IDWriteTextFormat,
    big: IDWriteTextFormat,
    big_c: IDWriteTextFormat,
    mid: IDWriteTextFormat,
    small: IDWriteTextFormat,
    small_c: IDWriteTextFormat,
    small_r: IDWriteTextFormat,
    hint_l: IDWriteTextFormat,
    emoji: IDWriteTextFormat,
    placeholder: Option<IDWriteTextLayout>,
}

fn build_formats(dw: &IDWriteFactory) -> Formats {
    unsafe { CUSTOM_FONT = load_custom_font(dw) };
    let custom_fam = unsafe { CUSTOM_FONT.as_ref() }.map(|(_, n)| n.clone());
    let fam: &str = match &custom_fam {
        Some(n) => n.as_str(),
        None if font_exists(dw, "Segoe UI Variable Text") => "Segoe UI Variable Text",
        None => "Segoe UI",
    };
    let icons = if font_exists(dw, "Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
    let text = make_format(dw, fam, FONT_SIZE, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING, false);
    let lang = read_string(SETTINGS_KEY, w!("Language")).unwrap_or_default();
    let placeholder = PLACEHOLDERS
        .iter()
        .find(|(code, _)| lang.len() >= 2 && lang[..2].eq_ignore_ascii_case(code))
        .map(|p| p.1)
        .unwrap_or(PLACEHOLDERS[0].1);
    let placeholder = unsafe { dw.CreateTextLayout(&ws(placeholder), &text, 10000.0, BAR_H).ok() };
    Formats {
        hint: make_format(dw, fam, HINT_SIZE, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING, true),
        row: make_format(dw, fam, 14.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING, true),
        row_right: make_format(dw, fam, 13.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING, false),
        glyph: make_format(dw, icons, 16.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, false),
        glyph_sm: make_format(dw, icons, 12.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, false),
        header: make_format(dw, fam, 12.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, false),
        card: make_format(dw, fam, 12.5, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, true),
        tile: make_format(dw, fam, 11.5, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, true),
        menu: make_format(dw, fam, 13.5, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING, true),
        big: make_format(dw, fam, 40.0, DWRITE_FONT_WEIGHT_SEMI_LIGHT, DWRITE_TEXT_ALIGNMENT_LEADING, false),
        big_c: make_format(dw, icons, 32.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, false),
        mid: make_format(dw, fam, 19.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, true),
        small: make_format(dw, fam, 11.5, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING, true),
        small_c: make_format(dw, fam, 11.5, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, false),
        small_r: make_format(dw, fam, 11.5, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING, false),
        hint_l: make_format(dw, fam, HINT_SIZE, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING, true),
        emoji: make_format(dw, "Segoe UI Emoji", 25.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, false),
        text,
        placeholder,
    }
}

// After "> font" changes the font: every text format is made again.
fn rebuild_formats() {
    let s = st();
    let f = build_formats(&s.dw);
    s.fmt_text = f.text;
    s.fmt_hint = f.hint;
    s.fmt_row = f.row;
    s.fmt_row_right = f.row_right;
    s.fmt_glyph = f.glyph;
    s.fmt_glyph_sm = f.glyph_sm;
    s.fmt_header = f.header;
    s.fmt_card = f.card;
    s.fmt_tile = f.tile;
    s.fmt_menu = f.menu;
    s.fmt_big = f.big;
    s.fmt_big_c = f.big_c;
    s.fmt_mid = f.mid;
    s.fmt_small = f.small;
    s.fmt_small_c = f.small_c;
    s.fmt_small_r = f.small_r;
    s.fmt_hint_l = f.hint_l;
    s.fmt_emoji = f.emoji;
    s.placeholder_layout = f.placeholder;
}

fn make_gradient(a: D2D_POINT_2F, b: D2D_POINT_2F, ca: D2D1_COLOR_F, cb: D2D1_COLOR_F) -> Option<ID2D1LinearGradientBrush> {
    unsafe {
        let rt = st().rt.as_ref()?;
        let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: ca }, D2D1_GRADIENT_STOP { position: 1.0, color: cb }];
        let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).ok()?;
        rt.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: a, endPoint: b }, None, &coll).ok()
    }
}

fn discard_device_resources() {
    let s = st();
    widgets::release_bitmaps();
    s.solid = None;
    s.logo_brush = None;
    s.bar_shadow.bmp = None;
    s.bar_shadow.dpi = 0.0;
    s.tile_shadow.bmp = None;
    s.tile_shadow.dpi = 0.0;
    s.bd_brush = None;
    s.bd_bmp = None;
    s.bd_dirty = !s.bd_px.is_empty();
    s.icon_bmps.clear();
    s.fonts.bmps.clear();
    s.rt = None;
    s.dcrt = None;
    if let Some(ctx) = &s.ctx {
        unsafe { ctx.SetTarget(None::<&ID2D1Image>) };
    }
    s.target = None;
    s.ctx = None;
    s.d2d_dev = None;
    s.dc_visual = None;
    s.dc_target = None;
    s.dcomp = None;
    s.swap = None;
    s.d3d = None;
}

fn create_d3d_device() -> Option<ID3D11Device> {
    for t in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut dev: Option<ID3D11Device> = None;
        let ok = unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                t,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut dev),
                None,
                None,
            )
        };
        if ok.is_ok() && dev.is_some() {
            return dev;
        }
    }
    None
}

fn bind_swap_target() -> bool {
    let s = st();
    let (Some(swap), Some(ctx)) = (&s.swap, &s.ctx) else { return false };
    unsafe {
        let Ok(surf) = swap.GetBuffer::<IDXGISurface>(0) else { return false };
        let bp = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: pixel_format(),
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        let Ok(bmp) = ctx.CreateBitmapFromDxgiSurface(&surf, Some(&bp)) else { return false };
        ctx.SetTarget(&bmp);
        ctx.SetDpi(s.dpi, s.dpi);
        s.target = Some(bmp);
    }
    true
}

fn create_gpu_target() -> windows::core::Result<()> {
    let s = st();
    let d3d = create_d3d_device().ok_or(windows::core::Error::from(E_FAIL))?;
    unsafe {
        let dxgi: IDXGIDevice1 = d3d.cast()?;
        let _ = dxgi.SetMaximumFrameLatency(1); // Present(1) then paces us at the display's refresh rate
        let dev = s.d2d.CreateDevice(&dxgi)?;
        let ctx = dev.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: s.px_w as u32,
            Height: s.px_h as u32,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            ..Default::default()
        };
        let swap = factory.CreateSwapChainForComposition(&d3d, &desc, None::<&IDXGIOutput>)?;
        let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
        let target = dcomp.CreateTargetForHwnd(hwnd(), TRUE)?;
        let visual = dcomp.CreateVisual()?;
        visual.SetContent(&swap)?;
        target.SetRoot(&visual)?;
        dcomp.Commit()?;
        s.d3d = Some(d3d);
        s.d2d_dev = Some(dev);
        s.ctx = Some(ctx.clone());
        s.swap = Some(swap);
        s.dcomp = Some(dcomp);
        s.dc_target = Some(target);
        s.dc_visual = Some(visual);
        if !bind_swap_target() {
            return Err(E_FAIL.into());
        }
        s.rt = Some(ctx.cast()?);
    }
    Ok(())
}

fn create_device_resources() -> bool {
    let s = st();
    if s.rt.is_some() {
        return true;
    }
    unsafe {
        if s.hw {
            if create_gpu_target().is_err() {
                discard_device_resources();
                return false;
            }
        } else {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: pixel_format(),
                dpiX: 0.0,
                dpiY: 0.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let Ok(dcrt) = s.d2d.CreateDCRenderTarget(&props) else { return false };
            let Ok(base) = dcrt.cast::<ID2D1RenderTarget>() else { return false };
            s.dcrt = Some(dcrt);
            s.rt = Some(base);
        }
        let rt = s.rt.clone().unwrap();
        rt.SetDpi(s.dpi, s.dpi);
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        s.solid = rt.CreateSolidColorBrush(&c(0, 1.0), None).ok();
        s.rt = Some(rt);
        let lx = MARGIN + LOGO_X;
        let ly = MARGIN + (BAR_H - LOGO_SIZE) / 2.0;
        s.logo_brush = make_gradient(pt(lx, ly), pt(lx + LOGO_SIZE, ly + LOGO_SIZE), c(0x4CC2FF, 1.0), c(0x0067C0, 1.0));
        s.solid.is_some() && s.logo_brush.is_some()
    }
}

// Gaussian-blurred rounded rectangle, drawn later as a nine-slice so it fits any size.
fn build_shadow(sh: &mut Shadow) {
    let s = st();
    if sh.bmp.is_some() && sh.dpi == s.dpi {
        return;
    }
    sh.bmp = None;
    let sc = s.dpi / 96.0;
    let sigma = sh.sigma * sc;
    let pad = (sigma * 3.0).ceil() as i32;
    let rr = sh.radius * sc;
    let cc = pad + rr.ceil() as i32 + 2;
    let n = (2 * cc + 1) as usize;
    let mut a = vec![0f32; n * n];
    let mut tmp = vec![0f32; n * n];
    let half = (n as f32 - 2.0 * pad as f32) / 2.0;
    let ctr = n as f32 / 2.0;
    for y in 0..n {
        for x in 0..n {
            let qx = (x as f32 + 0.5 - ctr).abs() - (half - rr);
            let qy = (y as f32 + 0.5 - ctr).abs() - (half - rr);
            let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rr;
            a[y * n + x] = (0.5 - d).clamp(0.0, 1.0);
        }
    }
    let kr = pad;
    let mut k: Vec<f32> = (-kr..=kr).map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp()).collect();
    let ks: f32 = k.iter().sum();
    for v in k.iter_mut() {
        *v /= ks;
    }
    for y in 0..n {
        for x in 0..n {
            let mut acc = 0.0;
            for i in -kr..=kr {
                let xx = x as i32 + i;
                if xx >= 0 && (xx as usize) < n {
                    acc += a[y * n + xx as usize] * k[(i + kr) as usize];
                }
            }
            tmp[y * n + x] = acc;
        }
    }
    let mut px = vec![0u32; n * n];
    for y in 0..n {
        for x in 0..n {
            let mut acc = 0.0;
            for i in -kr..=kr {
                let yy = y as i32 + i;
                if yy >= 0 && (yy as usize) < n {
                    acc += tmp[yy as usize * n + x] * k[(i + kr) as usize];
                }
            }
            px[y * n + x] = ((acc.clamp(0.0, 1.0) * 255.0 + 0.5) as u32) << 24;
        }
    }
    unsafe {
        let bp = D2D1_BITMAP_PROPERTIES { pixelFormat: pixel_format(), dpiX: 96.0, dpiY: 96.0 };
        sh.bmp = rt()
            .CreateBitmap(D2D_SIZE_U { width: n as u32, height: n as u32 }, Some(px.as_ptr() as *const c_void), (n * 4) as u32, &bp)
            .ok();
    }
    sh.c = cc;
    sh.n = n as i32;
    sh.pad = pad;
    sh.dpi = s.dpi;
}

fn ensure_surface(w: i32, h: i32) {
    let s = st();
    if s.hw {
        if w == s.px_w && h == s.px_h {
            return;
        }
        s.px_w = w;
        s.px_h = h;
        if let (Some(swap), Some(ctx)) = (s.swap.clone(), s.ctx.clone()) {
            unsafe {
                ctx.SetTarget(None::<&ID2D1Image>);
                s.target = None;
                let ok = swap.ResizeBuffers(0, w as u32, h as u32, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)).is_ok();
                if !ok || !bind_swap_target() {
                    discard_device_resources();
                }
            }
        }
        return;
    }
    if !s.dib.is_invalid() && w == s.px_w && h == s.px_h {
        return;
    }
    unsafe {
        if !s.dib.is_invalid() {
            SelectObject(s.mem_dc, s.old_bmp);
            let _ = DeleteObject(HGDIOBJ(s.dib.0));
            s.dib = HBITMAP::default();
        }
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut c_void = null_mut();
        if let Ok(dib) = CreateDIBSection(s.mem_dc, &bi, DIB_RGB_COLORS, &mut bits, HANDLE::default(), 0) {
            s.dib = dib;
            s.old_bmp = SelectObject(s.mem_dc, HGDIOBJ(dib.0));
        }
    }
    s.px_w = w;
    s.px_h = h;
}

fn position_on_monitor() {
    let s = st();
    unsafe {
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        // Over a game, open on the game's screen (it may be holding the cursor somewhere else).
        let mon = if !s.prev_fg.is_invalid() {
            MonitorFromWindow(s.prev_fg, MONITOR_DEFAULTTOPRIMARY)
        } else {
            MonitorFromPoint(p, MONITOR_DEFAULTTOPRIMARY)
        };
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        s.dpi = dx as f32;
        let sc = s.dpi / 96.0;
        let w = (win_w() * sc).ceil() as i32;
        let h = (WIN_MAX_H * sc).ceil() as i32;
        ensure_surface(w, h);
        if let Some(rt) = &s.rt {
            rt.SetDpi(s.dpi, s.dpi);
        }
        s.icon_px = (24.0 * sc).round() as i32;
        s.tile_px = (TILE_ICON * sc).round() as i32;

        // Centered horizontally, sitting a little above the vertical middle of the screen, and
        // lower if that's needed for the pinned tiles above it to fit.
        let wa = mi.rcWork;
        let mut bar_top = wa.top + ((wa.bottom - wa.top) as f32 * vertical_pos() - BAR_H / 2.0 * sc).round() as i32;
        bar_top = bar_top.max(wa.top + ((pins_area_h() + 16.0) * sc).round() as i32);
        s.win_pos.x = (wa.left + wa.right) / 2 - w / 2;
        s.win_pos.y = bar_top - ((MARGIN + PINS_MAX_H) * sc).round() as i32;
        s.win_h = -1; // hardware path: move/resize the window on the next frame
    }
}

// Snapshot the desktop behind the (hidden) window and blur it: the frosted backdrop.
fn box_blur(ch: &mut [f32], tmp: &mut [f32], w: usize, h: usize, r: i32) {
    let inv = 1.0 / (2 * r + 1) as f32;
    let cl = |v: i32, hi: usize| v.clamp(0, hi as i32 - 1) as usize;
    for y in 0..h {
        let row = y * w;
        let mut acc = 0.0;
        for i in -r..=r {
            acc += ch[row + cl(i, w)];
        }
        for x in 0..w {
            tmp[row + x] = acc * inv;
            acc += ch[row + cl(x as i32 + r + 1, w)] - ch[row + cl(x as i32 - r, w)];
        }
    }
    for x in 0..w {
        let mut acc = 0.0;
        for i in -r..=r {
            acc += tmp[cl(i, h) * w + x];
        }
        for y in 0..h {
            ch[y * w + x] = acc * inv;
            acc += tmp[cl(y as i32 + r + 1, h) * w + x] - tmp[cl(y as i32 - r, h) * w + x];
        }
    }
}

fn capture_backdrop() {
    let s = st();
    let (w, h) = (s.px_w, s.px_h);
    if w <= 0 || h <= 0 {
        return;
    }
    let f = BACKDROP_SCALE;
    let bw = ((w + f - 1) / f) as usize;
    let bh = ((h + f - 1) / f) as usize;
    let mut ch = [vec![0f32; bw * bh], vec![0f32; bw * bh], vec![0f32; bw * bh]];
    unsafe {
        let scr = GetDC(HWND::default());
        let mem = CreateCompatibleDC(scr);
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut c_void = null_mut();
        let Ok(bmp) = CreateDIBSection(mem, &bi, DIB_RGB_COLORS, &mut bits, HANDLE::default(), 0) else {
            let _ = DeleteDC(mem);
            ReleaseDC(HWND::default(), scr);
            return;
        };
        let old = SelectObject(mem, HGDIOBJ(bmp.0));
        let _ = BitBlt(mem, 0, 0, w, h, scr, s.win_pos.x, s.win_pos.y, SRCCOPY);
        let _ = GdiFlush();
        let src = std::slice::from_raw_parts(bits as *const u32, (w * h) as usize);
        let (wu, hu, fu) = (w as usize, h as usize, f as usize);
        for by in 0..bh {
            for bx in 0..bw {
                let (mut r, mut g, mut b, mut n) = (0f32, 0f32, 0f32, 0f32);
                for y in by * fu..(by * fu + fu).min(hu) {
                    for x in bx * fu..(bx * fu + fu).min(wu) {
                        let p = src[y * wu + x];
                        r += ((p >> 16) & 255) as f32;
                        g += ((p >> 8) & 255) as f32;
                        b += (p & 255) as f32;
                        n += 1.0;
                    }
                }
                let i = by * bw + bx;
                ch[0][i] = r / n;
                ch[1][i] = g / n;
                ch[2][i] = b / n;
            }
        }
        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        ReleaseDC(HWND::default(), scr);
    }

    let mut tmp = vec![0f32; bw * bh];
    for c in ch.iter_mut() {
        for _ in 0..3 {
            box_blur(c, &mut tmp, bw, bh, 5);
        }
    }
    s.bd_px.resize(bw * bh, 0);
    let sat = 1.35f32; // acrylic-style saturation boost
    for i in 0..bw * bh {
        let (r, g, b) = (ch[0][i], ch[1][i], ch[2][i]);
        let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let r = (l + (r - l) * sat).clamp(0.0, 255.0);
        let g = (l + (g - l) * sat).clamp(0.0, 255.0);
        let b = (l + (b - l) * sat).clamp(0.0, 255.0);
        s.bd_px[i] = 0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    }
    s.bd_w = bw as i32;
    s.bd_h = bh as i32;
    s.bd_dirty = true;
}

fn ensure_backdrop_brush() {
    let s = st();
    if !s.bd_dirty || s.bd_px.is_empty() || s.rt.is_none() {
        return;
    }
    s.bd_brush = None;
    s.bd_bmp = None;
    let bdpi = s.dpi / BACKDROP_SCALE as f32; // makes the bitmap's DIP size equal the window's
    unsafe {
        let bp = D2D1_BITMAP_PROPERTIES { pixelFormat: pixel_format(), dpiX: bdpi, dpiY: bdpi };
        if let Ok(bmp) = rt().CreateBitmap(
            D2D_SIZE_U { width: s.bd_w as u32, height: s.bd_h as u32 },
            Some(s.bd_px.as_ptr() as *const c_void),
            (s.bd_w * 4) as u32,
            &bp,
        ) {
            let bbp = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };
            s.bd_brush = rt().CreateBitmapBrush(&bmp, Some(&bbp), None).ok();
            s.bd_bmp = Some(bmp);
        }
    }
    s.bd_dirty = false;
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------
fn make_layout(text: &[u16], fmt: &IDWriteTextFormat, w: f32, h: f32) -> Option<IDWriteTextLayout> {
    unsafe { st().dw.CreateTextLayout(text, fmt, w, h).ok() }
}

fn caret_x_for(lay: &IDWriteTextLayout, pos: usize, len: usize) -> f32 {
    if len == 0 {
        return 0.0;
    }
    let mut m = DWRITE_HIT_TEST_METRICS::default();
    let (mut x, mut y) = (0f32, 0f32);
    unsafe {
        if pos >= len {
            let _ = lay.HitTestTextPosition((len - 1) as u32, TRUE, &mut x, &mut y, &mut m);
        } else {
            let _ = lay.HitTestTextPosition(pos as u32, FALSE, &mut x, &mut y, &mut m);
        }
    }
    x
}

fn set_color(col: D2D1_COLOR_F) {
    unsafe { solid().SetColor(&col) };
}
fn fill_rr(r: D2D_RECT_F, radius: f32, col: D2D1_COLOR_F) {
    set_color(col);
    unsafe { rt().FillRoundedRectangle(&rrect(r, radius), solid()) };
}
fn fill_rect(r: D2D_RECT_F, col: D2D1_COLOR_F) {
    set_color(col);
    unsafe { rt().FillRectangle(&r, solid()) };
}
fn get_transform() -> Matrix3x2 {
    let mut m = m_identity();
    unsafe { rt().GetTransform(&mut m) };
    m
}
fn set_transform(m: &Matrix3x2) {
    unsafe { rt().SetTransform(m) };
}

fn draw_shadow(bx: D2D_RECT_F, opacity: f32, sh: &Shadow) {
    let s = st();
    let Some(bmp) = sh.bmp.as_ref() else { return };
    if opacity <= 0.001 {
        return;
    }
    let sc = s.dpi / 96.0;
    let pad = sh.pad as f32 / sc;
    let cd = sh.c as f32 / sc;
    let o = rect(bx.left - pad, bx.top - pad + sh.offset_y, bx.right + pad, bx.bottom + pad + sh.offset_y);
    let dx = [o.left, o.left + cd, o.right - cd, o.right];
    let dy = [o.top, o.top + cd, o.bottom - cd, o.bottom];
    let sx = [0.0, sh.c as f32, sh.c as f32 + 1.0, sh.n as f32];
    unsafe {
        rt().SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED); // avoid seams between slices
        for j in 0..3 {
            for i in 0..3 {
                if dx[i + 1] <= dx[i] || dy[j + 1] <= dy[j] {
                    continue;
                }
                let d = rect(dx[i], dy[j], dx[i + 1], dy[j + 1]);
                let src = rect(sx[i], sx[j], sx[i + 1], sx[j + 1]);
                rt().DrawBitmap(bmp, Some(&d), opacity, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, Some(&src));
            }
        }
        rt().SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
}

// Fills with the blurred desktop (kept locked to the screen regardless of the current transform),
// then the theme tint on top.
fn fill_acrylic(rr: &D2D1_ROUNDED_RECT, t: &Theme, grad_top: f32, grad_bottom: f32) {
    let s = st();
    unsafe {
        if let Some(b) = &s.bd_brush {
            if let Some(inv) = m_invert(&get_transform()) {
                b.SetTransform(&inv);
            }
            rt().FillRoundedRectangle(rr, b);
        } else {
            set_color(c(0x202020, 1.0));
            rt().FillRoundedRectangle(rr, solid());
        }
        if let Some(g) = make_gradient(pt(0.0, grad_top), pt(0.0, grad_bottom), t.tint_top, t.tint_bottom) {
            rt().FillRoundedRectangle(rr, &g);
        }
    }
}

fn draw_logo_at(lx: f32, ly: f32, size: f32, brush: &ID2D1LinearGradientBrush, base: &Matrix3x2, t_now: f64, animate: bool) {
    let gap = size * 0.09;
    let q = (size - gap) / 2.0;
    let show_time = st().show_time;
    unsafe {
        for i in 0..4 {
            let x0 = lx + (i % 2) as f32 * (q + gap);
            let y0 = ly + (i / 2) as f32 * (q + gap);
            let mut k = 1.0f32;
            if animate {
                let t = (t_now - show_time - 0.04 - i as f64 * 0.045) / 0.42;
                k = ease_out_back(t.clamp(0.0, 1.0)) as f32;
            }
            let sk = 0.3 + 0.7 * k;
            set_transform(&m_mul(&m_scale(sk, sk, x0 + q / 2.0, y0 + q / 2.0), base));
            brush.SetOpacity(clamp01(k as f64 * 1.6));
            rt().FillRoundedRectangle(&rrect(rect(x0, y0, x0 + q, y0 + q), size * 0.08), brush);
        }
        brush.SetOpacity(1.0);
    }
    set_transform(base);
}

fn draw_text_area(t_now: f64, dt: f32) {
    let s = st();
    let th = s.th;
    let ox = MARGIN + TEXT_X;
    let right_edge = MARGIN + bar_w() - RIGHT_PAD;
    let cy = MARGIN + BAR_H / 2.0;

    unsafe {
        // Right-hand hint: shows which app Enter will open when it isn't an inline completion.
        let mut hint_w = 0.0f32;
        let pst = panels::ps();
        let flash = !pst.flash.is_empty() && t_now - pst.flash_time < 1.8;
        let hint: WStr = if flash {
            ws(&pst.flash)
        } else if s.hint_name.is_empty() {
            Vec::new()
        } else {
            let mut h = s.hint_name.clone();
            h.extend(ws("  ↵"));
            h
        };
        if !hint.is_empty() {
            if let Some(hl) = make_layout(&hint, &s.fmt_hint, HINT_MAX_W, BAR_H) {
                let mut tm = DWRITE_TEXT_METRICS::default();
                let _ = hl.GetMetrics(&mut tm);
                hint_w = tm.widthIncludingTrailingWhitespace.min(HINT_MAX_W);
                let o = clamp01((t_now - if flash { pst.flash_time } else { s.hint_time }) / 0.16);
                set_color(with_a(if flash { th.accent } else { th.dim }, ease_out_cubic(o as f64) as f32));
                rt().DrawTextLayout(pt(right_edge - HINT_MAX_W + (1.0 - o) * 6.0, MARGIN), &hl, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        }
        let avail = right_edge - ox - if hint_w > 0.0 { hint_w + 20.0 } else { 0.0 };

        let has_sel = s.anchor != s.caret;
        let len = s.text.len();
        let ghost: WStr = if !has_sel && s.caret == len { s.ghost.clone() } else { Vec::new() };
        let mut full = s.text.clone();
        full.extend_from_slice(&ghost);

        let Some(lay) = make_layout(&full, &s.fmt_text, 100000.0, BAR_H) else { return };

        // Per-character fade-in for freshly typed text (grouped by insertion time).
        const FADE: f64 = 0.14;
        let mut i = 0;
        while i < len {
            let age = t_now - s.birth[i];
            if age >= FADE {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < len && s.birth[j] == s.birth[i] {
                j += 1;
            }
            let o = 0.15 + 0.85 * ease_out_cubic(age / FADE) as f32;
            if let Ok(b) = rt().CreateSolidColorBrush(&with_a(th.text, o), None) {
                let _ = lay.SetDrawingEffect(&b, DWRITE_TEXT_RANGE { startPosition: i as u32, length: (j - i) as u32 });
            }
            i = j;
        }
        if !ghost.is_empty() {
            let o = ease_out_cubic(clamp01((t_now - s.last_input) / 0.12) as f64) as f32;
            if let Ok(b) = rt().CreateSolidColorBrush(&with_a(th.ghost, o), None) {
                let _ = lay.SetDrawingEffect(&b, DWRITE_TEXT_RANGE { startPosition: len as u32, length: ghost.len() as u32 });
            }
        }

        // Caret target, smoothing, and horizontal scrolling.
        let target = caret_x_for(&lay, s.caret, full.len());
        if !s.caret_init {
            s.caret_x = target;
            s.trail_x = target;
            s.caret_init = true;
        }
        s.caret_x = damp(s.caret_x, target, 32.0, dt);
        s.trail_x = damp(s.trail_x, target, 13.0, dt);

        let mut tm = DWRITE_TEXT_METRICS::default();
        let _ = lay.GetMetrics(&mut tm);
        let mut stt = s.scroll_target;
        if target - stt > avail - 4.0 {
            stt = target - avail + 4.0;
        }
        if target - stt < 0.0 {
            stt = target;
        }
        stt = stt.clamp(0.0, (tm.widthIncludingTrailingWhitespace - avail + 4.0).max(0.0));
        s.scroll_target = stt;
        s.scroll = damp(s.scroll, stt, 22.0, dt);

        s.placeholder_a = damp(s.placeholder_a, if s.text.is_empty() { 1.0 } else { 0.0 }, 18.0, dt);

        rt().PushAxisAlignedClip(&rect(ox - 4.0, MARGIN, ox + avail + 4.0, MARGIN + BAR_H), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        let tx = ox - s.scroll;

        if has_sel {
            let a = caret_x_for(&lay, s.anchor.min(s.caret), full.len());
            let b = caret_x_for(&lay, s.anchor.max(s.caret), full.len());
            fill_rr(rect(tx + a, cy - 12.0, tx + b, cy + 12.0), 3.0, with_a(th.accent, 0.30));
        }

        if s.placeholder_a > 0.01 {
            if let Some(pl) = &s.placeholder_layout {
                set_color(with_a(th.dim, s.placeholder_a));
                rt().DrawTextLayout(pt(ox + (1.0 - s.placeholder_a) * 10.0, MARGIN), pl, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        }

        set_color(th.text);
        rt().DrawTextLayout(pt(tx, MARGIN), &lay, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE);

        rt().PopAxisAlignedClip();

        // Caret: smooth glide + eased blink + small "pop" on input; its color, glow and effects
        // are in caret.rs.
        let idle = t_now - s.last_input;
        let blink = if idle < 0.5 { 1.0 } else { (0.5 + 0.5 * ((idle - 0.5) * 2.0 * std::f64::consts::PI / 1.05).cos()) as f32 };
        let intro = clamp01((t_now - s.show_time - 0.10) / 0.22);

        // clock, weather, what's playing: on the right while nothing is typed
        let mut ph_w = 0.0;
        if let Some(pl) = &s.placeholder_layout {
            let mut pm = DWRITE_TEXT_METRICS::default();
            let _ = pl.GetMetrics(&mut pm);
            ph_w = pm.width;
        }
        let strip_a = if hint.is_empty() { s.placeholder_a * ease_out_cubic(intro as f64) as f32 } else { 0.0 };
        widgets::draw_status_strip(ox + ph_w, right_edge, strip_a, t_now);
        let op = blink * ease_out_cubic(intro as f64) as f32;
        let h = CARET_H * (1.0 + 0.14 * (-idle * 16.0).exp() as f32);
        caret::draw_caret_and_fx(&lay, full.len(), tx, cy, op, h, t_now);
    }
}

fn draw_layout(text: &[u16], fmt: &IDWriteTextFormat, x: f32, y: f32, w: f32, h: f32, col: D2D1_COLOR_F) {
    if let Some(l) = make_layout(text, fmt, w, h) {
        set_color(col);
        unsafe { rt().DrawTextLayout(pt(x, y), &l, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE) };
    }
}

// List rows (results / commands / games / pins) with a sliding selection highlight.
fn draw_rows(top: f32, dt: f32, alpha: f32) {
    let s = st();
    let th = s.th;
    let apps = is_app_list(s.draw_mode);
    let prow = s.draw_mode == Mode::Panel;
    let n = if apps {
        s.results.len()
    } else if prow {
        panels::rows().len()
    } else {
        s.cmd_list.len()
    } as i32;
    let t_now = now();
    let mr = MAX_ROWS as i32;
    if n == 0 && prow {
        let a = alpha * clamp01(s.content_a as f64 * 1.4);
        draw_layout(&ws("\u{E946}"), &s.fmt_glyph, MARGIN + 20.0, top + PANEL_PAD, 24.0, ROW_H, with_a(th.dim, a));
        draw_layout(&ws(panels::cur_empty()), &s.fmt_row, MARGIN + 56.0, top + PANEL_PAD, bar_w() - 80.0, ROW_H, with_a(th.dim, a));
        return;
    }
    if n == 0 {
        if s.draw_mode == Mode::Games || s.draw_mode == Mode::Pins {
            let msg = if s.draw_mode == Mode::Pins {
                "Nothing pinned yet. Right-click an app to pin it."
            } else if GAMES_LOADING.load(Relaxed) {
                "Looking for games on your drives…"
            } else {
                "No games found"
            };
            let a = alpha * clamp01(s.content_a as f64 * 1.4);
            let glyph = if s.draw_mode == Mode::Pins { "\u{E718}" } else { "\u{E7FC}" };
            draw_layout(&ws(glyph), &s.fmt_glyph, MARGIN + 20.0, top + PANEL_PAD, 24.0, ROW_H, with_a(th.dim, a));
            draw_layout(&ws(msg), &s.fmt_row, MARGIN + 56.0, top + PANEL_PAD, bar_w() - 80.0, ROW_H, with_a(th.dim, a));
        }
        return;
    }
    let sel = s.sel.clamp(0, n - 1);
    // keep the selection inside the visible window of rows
    if sel < s.list_top {
        s.list_top = sel;
    }
    if sel >= s.list_top + mr {
        s.list_top = sel - mr + 1;
    }
    s.list_top = s.list_top.clamp(0, (n - mr).max(0));
    if !s.sb.drag {
        // while the scroll bar is dragged the list follows the mouse exactly
        s.list_off = damp(s.list_off, s.list_top as f32, 22.0, dt);
        if (s.list_off - s.list_top as f32).abs() < 0.002 {
            s.list_off = s.list_top as f32;
        }
    }
    if !s.sel_y_init {
        s.sel_row = sel as f32;
        s.sel_y_init = true;
    }
    s.sel_row = damp(s.sel_row, sel as f32, 26.0, dt);
    s.sel_y = top + PANEL_PAD + (s.sel_row - s.list_off) * ROW_H;

    // highlight + accent pill
    fill_rr(rect(MARGIN + 6.0, s.sel_y + 2.0, MARGIN + bar_w() - 6.0, s.sel_y + ROW_H - 2.0), 8.0, with_a(th.highlight, alpha));
    fill_rr(
        rect(MARGIN + 8.0, s.sel_y + ROW_H / 2.0 - 8.0, MARGIN + 11.0, s.sel_y + ROW_H / 2.0 + 8.0),
        1.5,
        with_a(th.accent, alpha),
    );

    let mut q = lower(&trim(&s.text));
    if s.draw_mode == Mode::Games {
        q = match q.iter().position(|&c| c == b' ' as u16) {
            Some(sp) => trim(&q[sp + 1..]),
            None => Vec::new(),
        };
    } else if s.draw_mode == Mode::Pins {
        q.clear();
    }
    let first = (s.list_off.floor() as i32 - 1).max(0);
    let last = n.min(s.list_off.ceil() as i32 + mr + 1);
    for i in first..last {
        let slide = (1.0 - s.content_a) * (6.0 + (i - first) as f32 * 3.0);
        let y = top + PANEL_PAD + (i as f32 - s.list_off) * ROW_H + slide;
        let row_a = alpha * clamp01(s.content_a as f64 * 1.4 - (i - first) as f64 * 0.06);
        s.hits.push((rect(MARGIN + 6.0, y, MARGIN + bar_w() - 6.0, y + ROW_H), i));
        let is_sel = i == sel;
        let ix = MARGIN + 20.0;
        let iy = y + (ROW_H - 24.0) / 2.0;

        if apps {
            let a = s.results[i as usize].clone();
            let icon = get_icon(icon_key_of(&a), 0);
            let io = match &icon {
                Some((_, shown)) => ease_out_cubic(clamp01((t_now - shown) / 0.18) as f64) as f32,
                None => 0.0,
            };
            if io < 1.0 {
                fill_rr(rect(ix, iy, ix + 24.0, iy + 24.0), 6.0, with_a(th.highlight, row_a * (1.0 - io) * 1.5));
                if s.draw_mode == Mode::Games {
                    draw_layout(&ws("\u{E7FC}"), &s.fmt_glyph, ix, y, 24.0, ROW_H, with_a(th.dim, row_a * (1.0 - io)));
                }
            }
            if let Some((bmp, _)) = &icon {
                unsafe {
                    rt().DrawBitmap(bmp, Some(&rect(ix, iy, ix + 24.0, iy + 24.0)), row_a * io, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None)
                };
            }
            let hidden_pin = s.draw_mode == Mode::Pins && a.detail == ws("Hidden");
            if let Some(l) = make_layout(&a.name, &s.fmt_row, bar_w() - 200.0, ROW_H) {
                if !q.is_empty() {
                    if let Some(p) = find(&a.lower, &q) {
                        unsafe {
                            let _ = l.SetFontWeight(DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_RANGE { startPosition: p as u32, length: q.len() as u32 });
                        }
                    }
                }
                let mut col = if is_sel { th.text } else { lerp_c(th.dim, th.text, 0.55) };
                if hidden_pin {
                    col = th.dim;
                }
                set_color(with_a(col, row_a));
                unsafe { rt().DrawTextLayout(pt(MARGIN + 56.0, y), &l, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE) };
            }
            let mut rc = th.dim;
            let right: WStr = match s.draw_mode {
                Mode::Results => {
                    if is_sel {
                        ws("Open  ↵")
                    } else {
                        Vec::new()
                    }
                }
                Mode::Games => {
                    if is_sel {
                        ws("Play  ↵")
                    } else {
                        a.detail.clone()
                    }
                }
                _ => {
                    if !hidden_pin && !is_sel {
                        rc = th.accent;
                    }
                    if is_sel {
                        ws(if hidden_pin { "Show  ↵" } else { "Hide  ↵" })
                    } else {
                        a.detail.clone()
                    }
                }
            };
            if !right.is_empty() {
                draw_layout(
                    &right,
                    &s.fmt_row_right,
                    MARGIN + bar_w() - 160.0,
                    y,
                    140.0,
                    ROW_H,
                    with_a(rc, row_a * if is_sel { 1.0 } else { 0.8 }),
                );
            }
        } else if prow {
            let r = panels::rows()[i as usize].clone();
            panels::draw_panel_row(&r, y, is_sel, row_a, t_now);
        } else {
            let cmd = s.cmd_list[i as usize];
            draw_layout(&ws(panels::cmd_glyph(cmd)), &s.fmt_glyph, ix, y, 24.0, ROW_H, with_a(if is_sel { th.accent } else { th.dim }, row_a));
            draw_layout(
                &ws(&panels::cmd_title(cmd)),
                &s.fmt_row,
                MARGIN + 56.0,
                y,
                bar_w() - 200.0,
                ROW_H,
                with_a(if is_sel { th.text } else { lerp_c(th.dim, th.text, 0.55) }, row_a),
            );
            let d = panels::cmd_detail(cmd);
            let on = d == "On";
            draw_layout(&ws(&d), &s.fmt_row_right, MARGIN + bar_w() - 180.0, y, 160.0, ROW_H, with_a(if on { th.accent } else { th.dim }, row_a));
        }
    }

    // scroll bar when the list is longer than the panel
    if n > mr {
        grid::draw_scroll_bar(top + PANEL_PAD + 4.0, mr as f32 * ROW_H - 8.0, s.list_off / (n - mr) as f32, mr as f32 / n as f32, alpha, dt);
    }
}

// Theme picker: a grid of live preview cards.
fn draw_theme_grid(top: f32, dt: f32, alpha: f32, t_now: f64) {
    let s = st();
    let th = s.th;
    draw_layout(&ws("Themes"), &s.fmt_header, MARGIN + GRID_PAD + 2.0, top + GRID_PAD - 4.0, 200.0, GRID_HEADER_H, with_a(th.dim, alpha));
    draw_layout(
        &ws("← → preview   ↵ apply"),
        &s.fmt_row_right,
        MARGIN + bar_w() - GRID_PAD - 260.0,
        top + GRID_PAD - 4.0,
        258.0,
        GRID_HEADER_H,
        with_a(th.dim, alpha * 0.85),
    );

    let base = get_transform();
    let cw = (bar_w() - 2.0 * GRID_PAD - (GRID_COLS as f32 - 1.0) * CARD_GAP) / GRID_COLS as f32;
    let y0 = top + GRID_PAD + GRID_HEADER_H - 4.0;
    for i in 0..THEME_COUNT {
        let col = i % GRID_COLS;
        let row = i / GRID_COLS;
        let x = MARGIN + GRID_PAD + col as f32 * (cw + CARD_GAP);
        let slide = (1.0 - s.content_a) * (8.0 + row as f32 * 6.0);
        let y = y0 + row as f32 * (CARD_PREVIEW_H + CARD_NAME_H + 4.0) + slide;
        let card_a = alpha * clamp01(s.content_a as f64 * 1.3 - (row * GRID_COLS + col) as f64 * 0.04);
        s.card_sel[i] = damp(s.card_sel[i], if i as i32 == s.sel { 1.0 } else { 0.0 }, 16.0, dt);
        let sk = 1.0 + 0.035 * s.card_sel[i];
        let card = rect(x, y, x + cw, y + CARD_PREVIEW_H);
        s.hits.push((rect(x, y, x + cw, y + CARD_PREVIEW_H + CARD_NAME_H), i as i32));

        let m = m_mul(&m_scale(sk, sk, x + cw / 2.0, y + CARD_PREVIEW_H / 2.0), &base);
        set_transform(&m);

        // Preview: the theme's real material on the real blurred desktop.
        let t = &themes()[i];
        unsafe {
            let lp = D2D1_LAYER_PARAMETERS {
                contentBounds: rect(-f32::MAX, -f32::MAX, f32::MAX, f32::MAX),
                geometricMask: ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: m_identity(),
                opacity: card_a,
                opacityBrush: ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS_NONE,
            };
            rt().PushLayer(&lp, None::<&ID2D1Layer>);
        }
        fill_acrylic(&rrect(card, 9.0), t, card.top, card.bottom);
        if let Some(b) = make_gradient(pt(0.0, card.top), pt(0.0, card.bottom), t.border_top, t.border_bottom) {
            unsafe {
                rt().DrawRoundedRectangle(&rrect(rect(x + 0.5, y + 0.5, x + cw - 0.5, y + CARD_PREVIEW_H - 0.5), 8.5), &b, 1.0, None::<&ID2D1StrokeStyle>)
            };
        }
        // mini bar contents: logo, faux text, caret, and a faux results row
        let ly = y + 14.0;
        if let Some(lb) = make_gradient(pt(x + 12.0, ly), pt(x + 24.0, ly + 12.0), c(0x4CC2FF, 1.0), c(0x0067C0, 1.0)) {
            draw_logo_at(x + 12.0, ly, 12.0, &lb, &m, t_now, false);
        }
        fill_rr(rect(x + 32.0, ly + 3.5, x + 32.0 + cw * 0.28, ly + 8.5), 2.5, with_a(t.text, 0.85));
        fill_rr(rect(x + 34.0 + cw * 0.28, ly + 3.5, x + 32.0 + cw * 0.52, ly + 8.5), 2.5, t.ghost);
        fill_rect(rect(x + 32.0 + cw * 0.28, ly - 1.0, x + 33.5 + cw * 0.28, ly + 13.0), t.accent);
        fill_rect(rect(x + 1.0, y + 34.0, x + cw - 1.0, y + 35.0), t.divider);
        fill_rr(rect(x + 6.0, y + 41.0, x + cw - 6.0, y + 61.0), 5.0, t.highlight);
        fill_rr(rect(x + 8.0, y + 46.0, x + 10.0, y + 56.0), 1.0, t.accent);
        fill_rr(rect(x + 16.0, y + 48.5, x + 16.0 + cw * 0.38, y + 53.5), 2.5, with_a(t.dim, 0.9));
        unsafe { rt().PopLayer() };

        // selection ring
        if s.card_sel[i] > 0.01 {
            set_color(with_a(th.accent, card_a * s.card_sel[i]));
            unsafe {
                rt().DrawRoundedRectangle(&rrect(rect(x - 3.0, y - 3.0, x + cw + 3.0, y + CARD_PREVIEW_H + 3.0), 12.0), solid(), 2.0, None::<&ID2D1StrokeStyle>)
            };
        }
        // active-theme badge
        if i == s.theme_idx {
            set_color(with_a(th.accent, card_a));
            unsafe {
                rt().FillEllipse(&D2D1_ELLIPSE { point: pt(x + cw - 13.0, y + 13.0), radiusX: 8.0, radiusY: 8.0 }, solid());
            }
            draw_layout(&ws("\u{E73E}"), &s.fmt_glyph, x + cw - 21.0, y + 5.0, 16.0, 16.0, with_a(c(0xFFFFFF, 1.0), card_a));
        }
        set_transform(&base);

        // name
        let nc = lerp_c(th.dim, th.text, s.card_sel[i]);
        draw_layout(&ws(t.name), &s.fmt_card, x, y + CARD_PREVIEW_H + 2.0, cw, CARD_NAME_H - 4.0, with_a(nc, card_a));
    }
}

fn draw_panel(t_now: f64, dt: f32) {
    let s = st();
    if s.panel_h < 0.5 || s.draw_mode == Mode::None {
        return;
    }
    let top = MARGIN + BAR_H;
    let alpha = ease_out_cubic(s.content_a as f64) as f32;

    // divider between the bar and the panel
    fill_rect(rect(MARGIN + 1.0, top, MARGIN + bar_w() - 1.0, top + 1.0), with_a(s.th.divider, clamp01(s.panel_h as f64 / 20.0)));

    unsafe {
        rt().PushAxisAlignedClip(&rect(MARGIN, top + 1.0, MARGIN + bar_w(), top + s.panel_h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
    if s.draw_mode == Mode::Themes {
        draw_theme_grid(top, dt, alpha, t_now);
    } else if s.draw_mode == Mode::Fonts {
        fonts::draw_grid(top, dt, alpha, t_now);
    } else if s.draw_mode == Mode::Caret {
        caret::draw_grid(top, dt, alpha, t_now);
    } else if let (Mode::Panel, Some(d)) = (s.draw_mode, panels::cur().and_then(|p| p.draw)) {
        d(top, dt, alpha, t_now);
    } else {
        draw_rows(top, dt, alpha);
    }
    unsafe { rt().PopAxisAlignedClip() };
}

fn push_layer(opacity: f32) {
    unsafe {
        let lp = D2D1_LAYER_PARAMETERS {
            contentBounds: rect(-f32::MAX, -f32::MAX, f32::MAX, f32::MAX),
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: m_identity(),
            opacity,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS_NONE,
        };
        rt().PushLayer(&lp, None::<&ID2D1Layer>);
    }
}

// ---------------------------------------------------------------------------
// Pinned tiles (above the bar)
// ---------------------------------------------------------------------------
fn draw_tile(pi: usize, alpha: f32, scale: f32, lift: f32, base: &Matrix3x2, t_now: f64) {
    let s = st();
    let th = s.th;
    let p = s.pins[pi].clone();
    let (x, y) = (p.x, p.y);
    let r = rect(x, y, x + tile_w(), y + TILE_H);
    set_transform(&m_mul(&m_scale(scale, scale, x + tile_w() / 2.0, y + TILE_H / 2.0), base));
    let layer = alpha < 0.999;
    if layer {
        push_layer(alpha);
    }
    draw_shadow(r, th.shadow * (0.5 + 0.5 * lift), &s.tile_shadow);
    let rr = rrect(r, TILE_RADIUS);
    fill_acrylic(&rr, &th, y, y + TILE_H);
    let hl = p.hover.max(lift);
    if hl > 0.01 {
        set_color(with_a(th.highlight, hl * 1.3));
        unsafe { rt().FillRoundedRectangle(&rr, solid()) };
    }
    if let Some(b) = make_gradient(pt(0.0, y), pt(0.0, y + TILE_H), th.border_top, th.border_bottom) {
        unsafe {
            rt().DrawRoundedRectangle(
                &rrect(rect(x + 0.5, y + 0.5, x + tile_w() - 0.5, y + TILE_H - 0.5), TILE_RADIUS - 0.5),
                &b,
                1.0,
                None::<&ID2D1StrokeStyle>,
            )
        };
    }
    let ix = x + (tile_w() - TILE_ICON) / 2.0;
    let iy = y + 12.0;
    let icon = get_icon(icon_key_of(&p.app), s.tile_px);
    let io = match &icon {
        Some((_, shown)) => ease_out_cubic(clamp01((t_now - shown) / 0.18) as f64) as f32,
        None => 0.0,
    };
    if io < 1.0 {
        fill_rr(rect(ix, iy, ix + TILE_ICON, iy + TILE_ICON), 8.0, with_a(th.highlight, (1.0 - io) * 1.5));
    }
    if let Some((bmp, _)) = &icon {
        unsafe {
            rt().DrawBitmap(bmp, Some(&rect(ix, iy, ix + TILE_ICON, iy + TILE_ICON)), io, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None)
        };
    }
    draw_layout(&p.app.name, &s.fmt_tile, x + 5.0, y + 50.0, tile_w() - 10.0, 22.0, lerp_c(lerp_c(th.dim, th.text, 0.7), th.text, hl));
    if layer {
        unsafe { rt().PopLayer() };
    }
    set_transform(base);
}

fn draw_pins(base: &Matrix3x2, t_now: f64, dt: f32) {
    let s = st();
    s.pin_hits.clear();
    let vis = visible_pins();
    let n_vis = vis.len() as i32;
    if n_vis == 0 {
        return;
    }
    let rows_shown = pin_rows_shown();
    s.pin_row_off = s.pin_row_off.clamp(0, (pin_rows_total() - rows_shown).max(0));
    let mut dragged_k: i32 = -1;
    for k in 0..n_vis {
        let pi = vis[k as usize];
        let row = k / PIN_COLS;
        let shown = row >= s.pin_row_off && row < s.pin_row_off + rows_shown;
        let dragged = s.dragging && pi as i32 == s.press_pin;
        let slot = pin_slot(k, n_vis);
        let (drag_pt, grab) = (s.drag_pt, s.grab);
        let hot = (s.hover_pin == pi as i32 && !s.dragging && s.press_pin < 0) || (s.menu_open && s.menu_pin == pi as i32);
        let pressed = s.press_pin == pi as i32 && !s.dragging;
        let p = &mut s.pins[pi];
        if !shown && !dragged {
            p.placed = false;
            continue;
        }
        let (tx, ty) = if dragged { (drag_pt.x - grab.x, drag_pt.y - grab.y) } else { (slot.x, slot.y) };
        if !p.placed || dragged {
            p.x = tx;
            p.y = ty;
            p.placed = true;
        } else {
            p.x = damp(p.x, tx, 20.0, dt);
            p.y = damp(p.y, ty, 20.0, dt);
        }
        p.hover = damp(p.hover, if hot { 1.0 } else { 0.0 }, 18.0, dt);
        p.press = damp(p.press, if pressed { 1.0 } else { 0.0 }, 30.0, dt);
        let (hover, press) = (p.hover, p.press);
        if dragged {
            dragged_k = k;
            continue;
        }
        s.pin_hits.push((rect(slot.x, slot.y, slot.x + tile_w(), slot.y + TILE_H), pi as i32));
        let t = (t_now - s.show_time - 0.02 - k as f64 * 0.022) / 0.38;
        let kin = ease_out_back(t.clamp(0.0, 1.0)) as f32;
        let scale = (0.86 + 0.14 * kin) * (1.0 + 0.035 * hover) * (1.0 - 0.05 * press);
        draw_tile(pi, clamp01(kin as f64 * 1.6), scale, 0.0, base, t_now);
    }
    if dragged_k >= 0 {
        draw_tile(vis[dragged_k as usize], 1.0, 1.07, 1.0, base, t_now);
    }
}

// ---------------------------------------------------------------------------
// Right-click menu
// ---------------------------------------------------------------------------
fn draw_menu(base: &Matrix3x2, t_now: f64, dt: f32) {
    let s = st();
    if !s.menu_open || s.menu.is_empty() {
        return;
    }
    let th = s.th;
    let t = ease_out_cubic(clamp01((t_now - s.menu_time) / 0.14) as f64) as f32;
    let (x, y, h) = (s.menu_pos.x, s.menu_pos.y, menu_h());
    let r = rect(x, y, x + MENU_W, y + h);
    set_transform(&m_mul(&m_scale(0.95 + 0.05 * t, 0.95 + 0.05 * t, x, y), base));
    push_layer(t);
    draw_shadow(r, th.shadow * 0.9, &s.tile_shadow);
    let rr = rrect(r, 9.0);
    fill_acrylic(&rr, &th, y, y + h);
    set_color(with_a(th.tint_bottom, 0.55)); // a little more solid than the bar: it sits on top of it
    unsafe { rt().FillRoundedRectangle(&rr, solid()) };
    if let Some(b) = make_gradient(pt(0.0, y), pt(0.0, y + h), th.border_top, th.border_bottom) {
        unsafe {
            rt().DrawRoundedRectangle(&rrect(rect(x + 0.5, y + 0.5, x + MENU_W - 0.5, y + h - 0.5), 8.5), &b, 1.0, None::<&ID2D1StrokeStyle>)
        };
    }
    if s.menu_sel >= 0 {
        if !s.menu_hl_init {
            s.menu_hl = s.menu_sel as f32;
            s.menu_hl_init = true;
        }
        s.menu_hl = damp(s.menu_hl, s.menu_sel as f32, 30.0, dt);
        let hy = y + MENU_PAD + s.menu_hl * MENU_ROW_H;
        fill_rr(rect(x + 4.0, hy + 1.0, x + MENU_W - 4.0, hy + MENU_ROW_H - 1.0), 6.0, th.highlight);
    }
    for (i, &(_, glyph, text)) in s.menu.iter().enumerate() {
        let ry = y + MENU_PAD + i as f32 * MENU_ROW_H;
        let sel = i as i32 == s.menu_sel;
        draw_layout(&ws(glyph), &s.fmt_glyph, x + 12.0, ry, 22.0, MENU_ROW_H, if sel { th.accent } else { lerp_c(th.dim, th.text, 0.5) });
        draw_layout(&ws(text), &s.fmt_menu, x + 44.0, ry, MENU_W - 56.0, MENU_ROW_H, th.text);
    }
    unsafe { rt().PopLayer() };
    set_transform(base);
}

// -2: outside the menu, -1: on its padding
fn menu_item_at(x: f32, y: f32) -> i32 {
    let s = st();
    if !s.menu_open || x < s.menu_pos.x || x > s.menu_pos.x + MENU_W || y < s.menu_pos.y || y > s.menu_pos.y + menu_h() {
        return -2;
    }
    let i = ((y - s.menu_pos.y - MENU_PAD) / MENU_ROW_H).floor() as i32;
    if i >= 0 && (i as usize) < s.menu.len() {
        i
    } else {
        -1
    }
}

fn render() {
    let s = st();
    if s.px_w == 0 || !create_device_resources() {
        return;
    }
    let t_now = now();
    let dt = (t_now - s.last_frame).clamp(0.0, 0.05) as f32;
    s.last_frame = t_now;

    let p = ((t_now - s.anim_start) / s.anim_dur).clamp(0.0, 1.0);
    let e = if s.anim_to > 0.5 { ease_out_quart(p) } else { ease_out_cubic(p) };
    s.anim = s.anim_from + (s.anim_to - s.anim_from) * e;
    if p >= 1.0 && s.anim_to < 0.5 {
        s.anim = 0.0;
        s.visible = false;
        set_bar_open(false);
        unsafe {
            let _ = ShowWindow(hwnd(), SW_HIDE);
        }
        s.preview_idx = s.theme_idx;
        s.th = themes()[s.theme_idx];
        s.th_from = s.th;
        return;
    }

    // theme cross-fade
    s.th = lerp_theme(&s.th_from, &themes()[s.preview_idx], ease_out_cubic(clamp01((t_now - s.th_start) / 0.35) as f64) as f32);

    // panel expansion
    panels::tick(t_now);
    let target = panel_target_h();
    s.panel_h = damp(s.panel_h, target, 20.0, dt);
    if (s.panel_h - target).abs() < 0.25 {
        s.panel_h = target;
    }
    if s.mode != Mode::None {
        s.content_a = damp(s.content_a, 1.0, 12.0, dt);
    } else if s.panel_h < 0.5 {
        s.draw_mode = Mode::None;
    }
    s.hits.clear();
    s.sb.shown = false; // drawn again below when the panel has one

    build_shadow(&mut s.bar_shadow);
    build_shadow(&mut s.tile_shadow);
    ensure_backdrop_brush();

    unsafe {
        if let Some(dcrt) = &s.dcrt {
            let rc = RECT { left: 0, top: 0, right: s.px_w, bottom: s.px_h };
            if dcrt.BindDC(s.mem_dc, &rc).is_err() {
                return;
            }
        }
        rt().BeginDraw();
        set_transform(&m_identity());
        rt().Clear(Some(&c(0, 0.0)));
        // The layered window fades through its constant alpha; the swap chain fades through a layer.
        let fade_layer = s.hw && s.anim < 0.999;
        if fade_layer {
            let lp = D2D1_LAYER_PARAMETERS {
                contentBounds: rect(-f32::MAX, -f32::MAX, f32::MAX, f32::MAX),
                geometricMask: ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: m_identity(),
                opacity: s.anim.clamp(0.0, 1.0) as f32,
                opacityBrush: ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS_NONE,
            };
            rt().PushLayer(&lp, None::<&ID2D1Layer>);
        }

        let a = s.anim as f32;
        let mut sc = 0.94 + 0.06 * a;
        let mut ty = (1.0 - a) * 16.0;
        let lay_anim = panels::ps().lay_anim;
        if lay_anim == 1 {
            // pop
            let ob = if s.anim_to > 0.5 { ease_out_back(p) as f32 } else { a };
            sc = 0.86 + 0.14 * ob;
            ty = 0.0;
        } else if lay_anim == 2 {
            // fade
            sc = 1.0;
            ty = 0.0;
        } else if lay_anim == 3 {
            // drop
            sc = 0.985 + 0.015 * a;
            ty = -(1.0 - a) * 22.0;
        }
        // bar space -> window: the pin area sits above the bar
        let base = m_mul(&m_scale(sc, sc, MARGIN + bar_w() / 2.0, MARGIN + BAR_H / 2.0), &m_translate(0.0, ty + PINS_MAX_H));
        set_transform(&base);

        draw_pins(&base, t_now, dt);

        let bx = rect(MARGIN, MARGIN, MARGIN + bar_w(), MARGIN + BAR_H + s.panel_h);
        let rr = rrect(bx, radius());
        draw_shadow(bx, s.th.shadow * if lay_anim == 1 { clamp01(a as f64) } else { 1.0 }, &s.bar_shadow);
        let th = s.th;
        fill_acrylic(&rr, &th, MARGIN, MARGIN + BAR_H);

        draw_panel(t_now, dt);

        if let Some(b) = make_gradient(pt(0.0, MARGIN), pt(0.0, MARGIN + BAR_H), th.border_top, th.border_bottom) {
            let inner = rect(bx.left + 0.5, bx.top + 0.5, bx.right - 0.5, bx.bottom - 0.5);
            rt().DrawRoundedRectangle(&rrect(inner, radius() - 0.5), &b, 1.0, None::<&ID2D1StrokeStyle>);
        }

        if let Some(lb) = s.logo_brush.clone() {
            draw_logo_at(MARGIN + LOGO_X, MARGIN + (BAR_H - LOGO_SIZE) / 2.0, LOGO_SIZE, &lb, &base, t_now, true);
        }
        draw_text_area(t_now, dt);
        draw_menu(&base, t_now, dt);

        if fade_layer {
            set_transform(&m_identity());
            rt().PopLayer();
        }
        if let Err(e) = rt().EndDraw(None, None) {
            if e.code() == D2DERR_RECREATE_TARGET {
                discard_device_resources();
            }
            return;
        }

        let scale = s.dpi / 96.0;
        let mut content_bottom = MARGIN * 2.0 + BAR_H + s.panel_h;
        if s.menu_open {
            content_bottom = content_bottom.max(s.menu_pos.y + menu_h() + 24.0);
        }
        let cur_h = s.px_h.min(((PINS_MAX_H + content_bottom) * scale).ceil() as i32);
        publish_box();
        if s.hw {
            // The window follows the panel's height so the empty space below never catches clicks.
            if cur_h != s.win_h {
                let _ = SetWindowPos(hwnd(), HWND::default(), s.win_pos.x, s.win_pos.y, s.px_w, cur_h, SWP_NOZORDER | SWP_NOACTIVATE);
                s.win_h = cur_h;
            }
            if let Some(swap) = &s.swap {
                let hr = swap.Present(1, DXGI_PRESENT(0));
                if hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET {
                    discard_device_resources();
                }
            }
            return;
        }
        let bf = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (255.0 * s.anim.clamp(0.0, 1.0)).round() as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let src = POINT { x: 0, y: 0 };
        let sz = SIZE { cx: s.px_w, cy: cur_h };
        let _ = UpdateLayeredWindow(hwnd(), HDC::default(), Some(&s.win_pos), Some(&sz), s.mem_dc, Some(&src), COLORREF(0), Some(&bf), ULW_ALPHA);
    }
}

// ---------------------------------------------------------------------------
// Show / hide
// ---------------------------------------------------------------------------
fn start_anim(to: f64, dur: f64) {
    let s = st();
    s.anim_from = s.anim;
    s.anim_to = to;
    s.anim_start = now();
    s.anim_dur = dur;
}

fn force_foreground(h: HWND) {
    unsafe {
        let fg = GetForegroundWindow();
        let me = GetCurrentThreadId();
        let other = if fg.is_invalid() { 0 } else { GetWindowThreadProcessId(fg, None) };
        if other != 0 && other != me && AttachThreadInput(me, other, TRUE).as_bool() {
            let _ = BringWindowToTop(h);
            let _ = SetForegroundWindow(h);
            let _ = AttachThreadInput(me, other, FALSE);
        } else {
            let _ = SetForegroundWindow(h);
        }
        if GetForegroundWindow() != h {
            // Windows refuses focus changes from background apps unless they just got input. A
            // no-op key (VK 0xE8 is unassigned) counts as that input, then try again. Needed over
            // games, which keep the keyboard otherwise.
            let key = |up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0xE8),
                        wScan: 0,
                        dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                        time: 0,
                        dwExtraInfo: INJECT_MAGIC,
                    },
                },
            };
            SendInput(&[key(false), key(true)], size_of::<INPUT>() as i32);
            let _ = SetForegroundWindow(h);
            let _ = BringWindowToTop(h);
        }
        let _ = SetFocus(h);
    }
}

fn close_menu() {
    let s = st();
    s.menu_open = false;
    s.menu_pin = -1;
}

fn show() {
    let s = st();
    if s.visible && s.anim_to > 0.5 {
        return;
    }
    if !s.visible {
        let fg = unsafe { GetForegroundWindow() };
        s.prev_fg = if !fg.is_invalid() && fg != hwnd() && (FULLSCREEN.load(Relaxed) || foreground_is_fullscreen()) {
            fg
        } else {
            HWND::default()
        };
        close_menu();
        s.press_pin = -1;
        s.hover_pin = -1;
        s.dragging = false;
        s.pin_row_off = 0;
        for p in s.pins.iter_mut() {
            p.placed = false;
            p.hover = 0.0;
            p.press = 0.0;
        }
        custom::apply_layout(); // a new width / position from "> layout"
        position_on_monitor();
        capture_backdrop();
        panels::ps().flash.clear();
        s.text.clear();
        s.birth.clear();
        s.caret = 0;
        s.anchor = 0;
        s.caret_init = false;
        s.scroll = 0.0;
        s.scroll_target = 0.0;
        s.placeholder_a = 1.0;
        s.panel_h = 0.0;
        s.content_a = 0.0;
        s.mode = Mode::None;
        s.draw_mode = Mode::None;
        s.show_time = now();
        s.autostart = is_auto_start();
        s.preview_idx = s.theme_idx;
        s.th = themes()[s.theme_idx];
        s.th_from = s.th;
        update_state();
        custom::sound_opened();
    }
    start_anim(1.0, if panels::ps().lay_anim == 1 { 0.38 } else { 0.32 });
    s.visible = true;
    publish_box();
    set_bar_open(true);
    s.last_frame = now();
    s.last_input = s.last_frame;
    render();
    unsafe {
        let _ = SetWindowPos(hwnd(), HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
    }
    force_foreground(hwnd());
    refresh_apps(false);
    widgets::on_show();
}

fn hide() {
    let s = st();
    if !s.visible || s.anim_to < 0.5 {
        return;
    }
    start_anim(0.0, 0.18);
    set_bar_open(false);
    s.last_hide_time = now();
    unsafe {
        if GetCapture() == hwnd() {
            let _ = ReleaseCapture();
        }
    }
    if s.dragging {
        save_pins();
    }
    s.dragging = false;
    s.press_pin = -1;
    s.sb.drag = false;
    s.sb.hot = false;
    caret::clear_fx();
    widgets::viz_stop();
    close_menu();
}

// Closing without launching anything (Esc, Win): hand the keyboard back to the game it was
// opened over.
fn dismiss() {
    let s = st();
    let back = s.prev_fg;
    hide();
    s.prev_fg = HWND::default();
    unsafe {
        if !back.is_invalid() && IsWindow(back).as_bool() {
            let _ = SetForegroundWindow(back);
        }
    }
}

fn toggle() {
    let s = st();
    if s.visible && s.anim_to > 0.5 {
        dismiss();
    } else {
        show();
    }
}

// ---------------------------------------------------------------------------
// Running commands
// ---------------------------------------------------------------------------
fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn looks_like_url(s: &str) -> bool {
    if s.contains("://") || s.starts_with("www.") {
        return true;
    }
    if s.contains(' ') {
        return false;
    }
    let Some(dot) = s.rfind('.') else { return false };
    if dot == 0 {
        return false;
    }
    let mut tld = &s[dot + 1..];
    if let Some(sl) = tld.find('/') {
        tld = &tld[..sl];
    }
    (2..=6).contains(&tld.chars().count()) && tld.chars().all(|c| c.is_alphabetic())
}

fn split_command(q: &str) -> (String, String) {
    if std::path::Path::new(q).exists() {
        return (q.to_string(), String::new());
    }
    let (file, rest) = if let Some(stripped) = q.strip_prefix('"') {
        match stripped.find('"') {
            Some(e) => (stripped[..e].to_string(), &stripped[e + 1..]),
            None => (stripped.to_string(), ""),
        }
    } else {
        match q.find(' ') {
            Some(e) => (q[..e].to_string(), &q[e..]),
            None => (q.to_string(), ""),
        }
    };
    (file, rest.trim().to_string())
}

fn launch(app: Option<&App>, q: &[u16], admin: bool) {
    let qs = String::from_utf16_lossy(q);
    let is_app = app.is_some();
    let mut dir = String::new();
    let (file, params) = match app {
        Some(a) => {
            let t = String::from_utf16_lossy(&a.target);
            if is_path_target(&a.target) && t.to_lowercase().ends_with(".exe") {
                dir = games::parent_dir(&t); // games expect to start in their own folder
            }
            if is_path_target(&a.target) || is_url_target(&a.target) {
                (t, String::new())
            } else {
                (format!("shell:AppsFolder\\{}", t), String::new())
            }
        }
        None => split_command(&qs),
    };
    let verb = if admin { "runas" } else { "open" };

    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    st().prev_fg = HWND::default();
    hide();
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let (fv, pv, vv, dv) = (wz(&file), wz(&params), wz(verb), wz(&dir));
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
            lpVerb: PCWSTR(vv.as_ptr()),
            lpFile: PCWSTR(fv.as_ptr()),
            lpParameters: if params.is_empty() { PCWSTR::null() } else { PCWSTR(pv.as_ptr()) },
            lpDirectory: if dir.is_empty() { PCWSTR::null() } else { PCWSTR(dv.as_ptr()) },
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        if ShellExecuteExW(&mut sei).is_err() && !is_app && GetLastError() != ERROR_CANCELLED {
            let url = if looks_like_url(&qs) {
                if qs.contains("://") {
                    qs.clone()
                } else {
                    format!("https://{}", qs)
                }
            } else {
                format!("https://www.bing.com/search?q={}", url_encode(&qs))
            };
            ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        }
        CoUninitialize();
    });
}

fn run_command(cmd: usize) {
    let s = st();
    match cmd {
        CMD_GAMES => set_text(&ws("> games ")),
        CMD_PINS => set_text(&ws("> pins")),
        CMD_THEME => set_text(&ws("> theme")),
        CMD_CARET => set_text(&ws("> caret")),
        CMD_FONT => set_text(&ws("> font ")),
        CMD_FONTFOLDER => fonts::open_folder(),
        CMD_EXPANDED => {
            s.expanded = !s.expanded;
            write_dword("Expanded", s.expanded as u32);
        }
        CMD_STARTUP => set_auto_start(!s.autostart),
        CMD_QUIT => unsafe {
            let _ = DestroyWindow(hwnd());
        },
        _ => panels::cmd_run(cmd),
    }
}

// Enter / click on the current selection.
fn activate(force_raw: bool, admin: bool) {
    let s = st();
    let q = trim(&s.text);
    match s.mode {
        Mode::Themes => apply_theme(s.sel.max(0) as usize),
        Mode::Fonts => fonts::activate_sel(),
        Mode::Caret => caret::apply_sel(),
        Mode::Panel => panels::activate(key_down(VK_SHIFT.0 as i32)),
        Mode::Commands => {
            if s.sel >= 0 && (s.sel as usize) < s.cmd_list.len() {
                run_command(s.cmd_list[s.sel as usize]);
            }
        }
        Mode::Results => {
            if q.is_empty() {
                return;
            }
            if !force_raw && s.sel >= 0 && (s.sel as usize) < s.results.len() {
                let a = s.results[s.sel as usize].clone();
                launch(Some(&a), &q, admin);
                return;
            }
            launch(None, &q, admin);
        }
        Mode::Games => {
            if s.sel >= 0 && (s.sel as usize) < s.results.len() {
                let a = s.results[s.sel as usize].clone();
                launch(Some(&a), &[], admin);
            }
        }
        Mode::Pins => {
            if s.sel >= 0 && (s.sel as usize) < s.results.len() {
                let i = find_pin(&s.results[s.sel as usize].target);
                if i >= 0 {
                    let p = &mut s.pins[i as usize];
                    p.hidden = !p.hidden;
                    p.placed = false;
                    save_pins();
                    let sel = s.sel;
                    update_state();
                    s.sel = sel.min(s.results.len() as i32 - 1);
                }
            }
        }
        Mode::None => {
            if q.is_empty() {
                return;
            }
            if !force_raw && calc::inline_activate() {
                return;
            }
            let m = s.match_app.clone();
            launch(if s.has_match && !force_raw { Some(&m) } else { None }, &q, admin);
        }
    }
}

fn select(idx: i32) {
    let n = item_count();
    if n == 0 {
        return;
    }
    let idx = idx.clamp(0, n - 1);
    let s = st();
    if idx == s.sel {
        return;
    }
    s.sel = idx;
    if s.mode == Mode::Themes {
        set_preview(idx as usize);
    }
    recompute_ghost();
}

// ---------------------------------------------------------------------------
// Right-click menu actions
// ---------------------------------------------------------------------------
// The file behind an app: the exe of a desktop app, the shortcut / exe of a game.
fn app_file_path(a: &App) -> String {
    if is_path_target(&a.target) {
        return String::from_utf16_lossy(&a.target);
    }
    if is_path_target(&a.icon) {
        return String::from_utf16_lossy(&a.icon);
    }
    if is_url_target(&a.target) || is_packaged_target(&a.target) {
        return String::new();
    }
    let path = HSTRING::from(format!("shell:AppsFolder\\{}", String::from_utf16_lossy(&a.target)));
    let mut r = String::new();
    unsafe {
        // PKEY_Link_TargetParsingPath
        let key = windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY {
            fmtid: windows::core::GUID::from_u128(0xB9B4B3FC_2B51_4A42_B5D8_324146AFCF25),
            pid: 2,
        };
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem2>(&path, None::<&IBindCtx>) {
            if let Ok(p) = item.GetString(&key) {
                r = games::expand_env(&String::from_utf16_lossy(p.as_wide()));
                CoTaskMemFree(Some(p.0 as *const c_void));
            }
        }
        if r.is_empty() && a.target.contains(&(b'\\' as u16)) {
            // "{known folder GUID}\rest\of\path.exe"
            let mut pidl: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST = null_mut();
            if SHParseDisplayName(&path, None::<&IBindCtx>, &mut pidl, 0, None).is_ok() && !pidl.is_null() {
                if let Ok(p) = SHGetNameFromIDList(pidl, SIGDN_FILESYSPATH) {
                    r = String::from_utf16_lossy(p.as_wide());
                    CoTaskMemFree(Some(p.0 as *const c_void));
                }
                CoTaskMemFree(Some(pidl as *const c_void));
            }
        }
    }
    if games::file_exists(&r) {
        r
    } else {
        String::new()
    }
}

fn show_in_explorer(file: String) {
    if file.is_empty() {
        return;
    }
    st().prev_fg = HWND::default();
    hide();
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut pidl: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST = null_mut();
        if SHParseDisplayName(&HSTRING::from(file), None::<&IBindCtx>, &mut pidl, 0, None).is_ok() && !pidl.is_null() {
            let _ = SHOpenFolderAndSelectItems(pidl, None, 0);
            CoTaskMemFree(Some(pidl as *const c_void));
        }
        CoUninitialize();
    });
}

fn open_menu(app: &App, pin: i32, mut at: D2D_POINT_2F) {
    let s = st();
    s.menu.clear();
    let file = app_file_path(app);
    let runnable = !is_url_target(&app.target) && !is_packaged_target(&app.target);
    if runnable {
        s.menu.push((MA_ADMIN, "\u{E7EF}", "Run as administrator"));
    }
    if !file.is_empty() {
        s.menu.push((MA_LOCATION, "\u{E838}", "Open file location"));
    }
    if find_pin(&app.target) >= 0 {
        s.menu.push((MA_UNPIN_START, "\u{E77A}", "Unpin from Start"));
    } else {
        s.menu.push((MA_PIN_START, "\u{E718}", "Pin to Start"));
    }
    if !file.is_empty() {
        s.menu.push((MA_PIN_TASKBAR, "\u{E718}", "Pin to taskbar"));
    }
    if pin >= 0 {
        s.menu.push((MA_HIDE, "\u{ED1A}", "Hide"));
    }
    s.menu_app = app.clone();
    s.menu_pin = pin;
    // keep it inside the window (bar space: x 0..win_w(), y -PINS_MAX_H..WIN_MAX_H-PINS_MAX_H)
    let h = menu_h();
    at.x = at.x.clamp(6.0, win_w() - MENU_W - 6.0);
    let bottom = WIN_MAX_H - PINS_MAX_H - 6.0;
    if at.y + h > bottom {
        at.y = (-PINS_MAX_H + 6.0).max(at.y - h);
    }
    s.menu_pos = at;
    s.menu_time = now();
    s.menu_sel = -1;
    s.menu_hl_init = false;
    s.menu_open = true;
    publish_box();
}

fn run_menu_action(i: i32) {
    let s = st();
    if i < 0 || i as usize >= s.menu.len() {
        return;
    }
    let action = s.menu[i as usize].0;
    let app = s.menu_app.clone();
    let pin = s.menu_pin;
    close_menu();
    match action {
        MA_ADMIN => launch(Some(&app), &[], true),
        MA_LOCATION => show_in_explorer(app_file_path(&app)),
        // Windows 11 only lets people pin to the taskbar themselves (from Explorer's menu or a
        // running app's taskbar button): show the file so it's one right-click away.
        MA_PIN_TASKBAR => show_in_explorer(app_file_path(&app)),
        MA_PIN_START => {
            let mut a = app.clone();
            a.alias.clear();
            a.detail.clear();
            s.pins.push(Pin { app: a, ..Default::default() });
            save_pins();
        }
        MA_UNPIN_START => {
            let k = find_pin(&app.target);
            if k >= 0 {
                s.pins.remove(k as usize);
            }
            save_pins();
        }
        MA_HIDE => {
            if pin >= 0 && (pin as usize) < s.pins.len() {
                s.pins[pin as usize].hidden = true;
                save_pins();
            }
        }
        _ => {}
    }
    if action == MA_PIN_START || action == MA_UNPIN_START || action == MA_HIDE {
        for p in s.pins.iter_mut() {
            p.placed = p.placed && !p.hidden;
        }
        if s.mode == Mode::Pins || s.mode == Mode::Commands {
            let sel = s.sel;
            update_state();
            s.sel = sel.clamp(0, (item_count() - 1).max(0));
        }
        publish_box();
    }
}

// ---------------------------------------------------------------------------
// Text editing
// ---------------------------------------------------------------------------
fn has_sel() -> bool {
    st().anchor != st().caret
}
fn touched() {
    st().last_input = now();
}
fn edited() {
    touched();
    update_state();
}
fn set_text(t: &[u16]) {
    let s = st();
    s.text = t.to_vec();
    s.birth = vec![0.0; t.len()];
    s.caret = t.len();
    s.anchor = t.len();
    edited();
}
fn delete_range(a: usize, b: usize) {
    let s = st();
    let (a, b) = if a > b { (b, a) } else { (a, b) };
    if b > a {
        caret::deleted(a, b - a);
        custom::sound_typed();
    }
    s.text.drain(a..b);
    s.birth.drain(a..b);
    s.caret = a;
    s.anchor = a;
}
fn delete_sel() {
    let s = st();
    delete_range(s.anchor, s.caret);
}
fn insert_text(t: &[u16]) {
    let t: WStr = t.iter().map(|&c| if c == 13 || c == 10 || c == 9 { 32 } else { c }).collect();
    if has_sel() {
        delete_sel();
    }
    let s = st();
    let tm = now();
    let at = s.caret;
    if !t.is_empty() {
        caret::typed(at, t.len());
        custom::sound_typed();
    }
    s.text.splice(at..at, t.iter().copied());
    s.birth.splice(at..at, std::iter::repeat(tm).take(t.len()));
    s.caret += t.len();
    s.anchor = s.caret;
    edited();
}
fn word_left(mut i: usize) -> usize {
    let t = &st().text;
    while i > 0 && is_space(t[i - 1]) {
        i -= 1;
    }
    while i > 0 && !is_space(t[i - 1]) {
        i -= 1;
    }
    i
}
fn word_right(mut i: usize) -> usize {
    let t = &st().text;
    let n = t.len();
    while i < n && !is_space(t[i]) {
        i += 1;
    }
    while i < n && is_space(t[i]) {
        i += 1;
    }
    i
}
fn prev_char(i: usize) -> usize {
    let t = &st().text;
    if i == 0 {
        return 0;
    }
    let mut i = i - 1;
    if i > 0 && is_low(t[i]) && is_high(t[i - 1]) {
        i -= 1;
    }
    i
}
fn next_char(i: usize) -> usize {
    let t = &st().text;
    let n = t.len();
    if i >= n {
        return n;
    }
    let mut i = i + 1;
    if i < n && is_low(t[i]) && is_high(t[i - 1]) {
        i += 1;
    }
    i
}

fn copy_selection() {
    if !has_sel() {
        return;
    }
    let s = st();
    let (a, b) = (s.anchor.min(s.caret), s.anchor.max(s.caret));
    let mut v = s.text[a..b].to_vec();
    v.push(0);
    unsafe {
        if OpenClipboard(hwnd()).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, v.len() * 2) {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(v.as_ptr(), p, v.len());
                let _ = GlobalUnlock(h);
                if SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(h.0)).is_err() {
                    let _ = GlobalFree(h);
                }
            }
        }
        let _ = CloseClipboard();
    }
}

fn paste() {
    let mut v: WStr = Vec::new();
    unsafe {
        if OpenClipboard(hwnd()).is_err() {
            return;
        }
        if let Ok(h) = GetClipboardData(CF_UNICODETEXT.0 as u32) {
            let hg = HGLOBAL(h.0);
            let p = GlobalLock(hg) as *const u16;
            if !p.is_null() {
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                v = std::slice::from_raw_parts(p, n).to_vec();
                let _ = GlobalUnlock(hg);
            }
        }
        let _ = CloseClipboard();
    }
    if !v.is_empty() {
        insert_text(&v);
    }
}

fn accept_ghost() {
    let g = st().ghost.clone();
    if !g.is_empty() {
        insert_text(&g);
    }
}

fn move_caret(pos: usize, extend: bool) {
    let s = st();
    s.caret = pos.min(s.text.len());
    if !extend {
        s.anchor = s.caret;
    }
    touched();
}

fn key_down(vk: i32) -> bool {
    unsafe { GetKeyState(vk) < 0 }
}

fn on_key(vk: u16) {
    let s = st();
    if s.anim_to < 0.5 {
        return;
    }
    let ctrl = key_down(VK_CONTROL.0 as i32);
    let shift = key_down(VK_SHIFT.0 as i32);
    let n = s.text.len();
    let grid = s.mode == Mode::Themes;
    let list = is_app_list(s.mode) || s.mode == Mode::Commands || (s.mode == Mode::Panel && panels::has_rows());
    let sel = s.sel;
    let gc = GRID_COLS as i32;
    if s.menu_open {
        // the menu has the keyboard while it's open
        let n2 = s.menu.len() as i32;
        match VIRTUAL_KEY(vk) {
            VK_ESCAPE => close_menu(),
            VK_UP => s.menu_sel = if s.menu_sel <= 0 { n2 - 1 } else { s.menu_sel - 1 },
            VK_DOWN => s.menu_sel = (s.menu_sel + 1) % n2,
            VK_RETURN if s.menu_sel >= 0 => run_menu_action(s.menu_sel),
            _ => {}
        }
        return;
    }
    if s.dragging && VIRTUAL_KEY(vk) == VK_ESCAPE {
        s.dragging = false;
        s.press_pin = -1;
        unsafe {
            if GetCapture() == hwnd() {
                let _ = ReleaseCapture();
            }
        }
        save_pins();
        return;
    }
    if s.mode == Mode::Fonts && fonts::on_key(VIRTUAL_KEY(vk), shift) {
        return;
    }
    if s.mode == Mode::Caret && caret::on_key(VIRTUAL_KEY(vk), shift) {
        return;
    }
    if s.mode == Mode::Panel && !ctrl && panels::key(VIRTUAL_KEY(vk), shift) {
        return;
    }
    match VIRTUAL_KEY(vk) {
        VK_ESCAPE => dismiss(),
        VK_APPS => {
            // the keyboard's menu key opens the right-click menu for the selection
            if is_app_list(s.mode) && sel >= 0 && (sel as usize) < s.results.len() {
                let a = s.results[sel as usize].clone();
                open_menu(&a, -1, pt(MARGIN + 60.0, s.sel_y + ROW_H));
            }
        }
        VK_PRIOR => {
            if list {
                select(sel - MAX_ROWS as i32);
            }
        }
        VK_NEXT => {
            if list {
                select(sel + MAX_ROWS as i32);
            }
        }
        VK_RETURN => activate(shift && !ctrl, ctrl && shift),
        VK_TAB => {
            if s.mode == Mode::Commands && s.ghost.is_empty() && (sel as usize) < s.cmd_list.len() {
                set_text(&ws(&format!("> {}", panels::cmd_key(s.cmd_list[sel as usize]))));
            } else {
                accept_ghost();
            }
        }
        VK_UP => {
            if grid {
                select(if sel - gc >= 0 { sel - gc } else { sel });
            } else if list {
                select(sel - 1);
            }
        }
        VK_DOWN => {
            if grid {
                select(if sel + gc < THEME_COUNT as i32 { sel + gc } else { sel });
            } else if list {
                select(sel + 1);
            }
        }
        VK_LEFT => {
            if grid && !shift {
                select(sel - 1);
            } else if !shift && has_sel() {
                move_caret(s.anchor.min(s.caret), false);
            } else {
                move_caret(if ctrl { word_left(s.caret) } else { prev_char(s.caret) }, shift);
            }
        }
        VK_RIGHT => {
            if grid && !shift {
                select(sel + 1);
            } else if !shift && has_sel() {
                move_caret(s.anchor.max(s.caret), false);
            } else if !shift && s.caret == n && !s.ghost.is_empty() {
                accept_ghost();
            } else {
                move_caret(if ctrl { word_right(s.caret) } else { next_char(s.caret) }, shift);
            }
        }
        VK_HOME => move_caret(0, shift),
        VK_END => {
            if !shift && s.caret == n && !s.ghost.is_empty() {
                accept_ghost();
            } else {
                move_caret(n, shift);
            }
        }
        VK_BACK => {
            if has_sel() {
                delete_sel();
            } else if s.caret > 0 {
                delete_range(if ctrl { word_left(s.caret) } else { prev_char(s.caret) }, s.caret);
            } else {
                return;
            }
            edited();
        }
        VK_DELETE => {
            if has_sel() {
                delete_sel();
            } else if s.caret < n {
                delete_range(s.caret, if ctrl { word_right(s.caret) } else { next_char(s.caret) });
            } else {
                return;
            }
            edited();
        }
        VK_A if ctrl => {
            s.anchor = 0;
            s.caret = n;
            touched();
        }
        VK_C if ctrl => copy_selection(),
        VK_X if ctrl && has_sel() => {
            copy_selection();
            delete_sel();
            edited();
        }
        VK_V if ctrl => paste(),
        _ => {}
    }
}

fn on_char(ch: u16) {
    if st().anim_to < 0.5 {
        return;
    }
    if ch < 32 || ch == 127 {
        return; // control chars handled in on_key
    }
    insert_text(&[ch]);
}

fn hit_text_index(x_dip: f32) -> usize {
    let s = st();
    let Some(lay) = make_layout(&s.text, &s.fmt_text, 100000.0, BAR_H) else { return s.caret };
    let (mut trailing, mut inside) = (BOOL(0), BOOL(0));
    let mut m = DWRITE_HIT_TEST_METRICS::default();
    unsafe {
        let _ = lay.HitTestPoint(x_dip - (MARGIN + TEXT_X) + s.scroll, BAR_H / 2.0, &mut trailing, &mut inside, &mut m);
    }
    let pos = m.textPosition as usize + if trailing.as_bool() { m.length as usize } else { 0 };
    pos.min(s.text.len())
}

fn pin_at(x: f32, y: f32) -> i32 {
    for (r, idx) in &st().pin_hits {
        if x >= r.left && x <= r.right && y >= r.top && y <= r.bottom {
            return *idx;
        }
    }
    -1
}

// Dragging the panel's scroll bar: y is the mouse (bar space).
fn scroll_bar_drag_to(y: f32) {
    let s = st();
    let f = grid::frac(y);
    if s.mode == Mode::Fonts {
        let g = &mut s.fonts.grid;
        g.scroll_to(f * g.max_off(), true);
    } else if s.mode == Mode::Caret {
        let g = &mut s.cfx.grid;
        g.scroll_to(f * g.max_off(), true);
    } else if let (Mode::Panel, Some(sf)) = (s.mode, panels::cur().and_then(|p| p.scroll_frac)) {
        sf(f);
    } else if is_app_list(s.mode) || s.mode == Mode::Commands || (s.mode == Mode::Panel && panels::has_rows()) {
        let n = item_count();
        let room = (n - MAX_ROWS as i32).max(0);
        s.list_off = f * room as f32;
        s.list_top = (s.list_off.round() as i32).clamp(0, room);
        s.sel = s.sel.clamp(s.list_top, (n - 1).min(s.list_top + MAX_ROWS as i32 - 1).max(s.list_top));
        recompute_ghost();
    }
}

fn hit_item_at(x: f32, y: f32) -> i32 {
    let s = st();
    if y < MARGIN + BAR_H || y > MARGIN + BAR_H + s.panel_h {
        return -1;
    }
    for (r, idx) in &s.hits {
        if x >= r.left && x <= r.right && y >= r.top && y <= r.bottom {
            return *idx;
        }
    }
    -1
}

// ---------------------------------------------------------------------------
// Tray icon (the Windows 11 logo, drawn at runtime)
// ---------------------------------------------------------------------------
fn make_logo_icon(size: i32) -> HICON {
    unsafe {
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = size;
        bi.bmiHeader.biHeight = -size;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut c_void = null_mut();
        let dc = GetDC(HWND::default());
        let color = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, HANDLE::default(), 0);
        ReleaseDC(HWND::default(), dc);
        let Ok(color) = color else { return HICON::default() };

        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        let fs = size as f32;
        let pad = fs * 0.06;
        let sz = fs - 2.0 * pad;
        let gap = sz * 0.09;
        let q = (sz - gap) / 2.0;
        let inside = |x: f32, y: f32| {
            let (x, y) = (x - pad, y - pad);
            if x < 0.0 || y < 0.0 || x >= sz || y >= sz {
                return false;
            }
            (x < q || x >= q + gap) && (y < q || y >= q + gap)
        };
        let (r0, g0, b0, r1, g1, b1) = (0x4C as f32, 0xC2 as f32, 0xFF as f32, 0.0f32, 0x67 as f32, 0xC0 as f32);
        for y in 0..size {
            for x in 0..size {
                let mut hits = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        if inside(x as f32 + (sx as f32 + 0.5) / 4.0, y as f32 + (sy as f32 + 0.5) / 4.0) {
                            hits += 1;
                        }
                    }
                }
                let a = hits as f32 / 16.0;
                let t = ((x + y) as f32 / (2.0 * fs)).clamp(0.0, 1.0);
                let r = (r0 + (r1 - r0) * t) * a;
                let g = (g0 + (g1 - g0) * t) * a;
                let b = (b0 + (b1 - b0) * t) * a;
                px[(y * size + x) as usize] =
                    (((a * 255.0 + 0.5) as u32) << 24) | (((r + 0.5) as u32) << 16) | (((g + 0.5) as u32) << 8) | (b + 0.5) as u32;
            }
        }
        let mask_bits = vec![0u8; (((size + 15) / 16) * 2 * size) as usize];
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bits.as_ptr() as *const c_void));
        let ii = ICONINFO { fIcon: TRUE, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&ii).unwrap_or_default();
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(color.0));
        icon
    }
}

static mut TRAY_ICON: HICON = HICON(null_mut());

fn add_tray_icon() {
    unsafe {
        let mut nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd(),
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_APP_TRAY,
            hIcon: TRAY_ICON,
            ..Default::default()
        };
        let tip = ws("Custom Start — press the Windows key");
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

fn remove_tray_icon() {
    unsafe {
        let nid = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd(), uID: 1, ..Default::default() };
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn show_tray_menu() {
    unsafe {
        let s = st();
        let Ok(theme_menu) = CreatePopupMenu() else { return };
        let names: Vec<WStr> = themes().iter().map(|t| wz(t.name)).collect();
        for (i, n) in names.iter().enumerate() {
            let f = if i == s.theme_idx { MF_STRING | MF_CHECKED } else { MF_STRING };
            let _ = AppendMenuW(theme_menu, f, ID_THEME0 + i, PCWSTR(n.as_ptr()));
        }
        let Ok(m) = CreatePopupMenu() else { return };
        let chk = |on: bool| if on { MF_STRING | MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(m, MF_STRING, ID_OPEN, w!("Open command bar"));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(m, MF_POPUP, theme_menu.0 as usize, w!("Theme"));
        let _ = AppendMenuW(m, chk(s.expanded), ID_EXPANDED, w!("Expanded results"));
        let _ = AppendMenuW(m, chk(INTERCEPT.load(Relaxed)), ID_INTERCEPT, w!("Replace Start menu"));
        let _ = AppendMenuW(m, chk(is_auto_start()), ID_AUTOSTART, w!("Start with Windows"));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(m, MF_STRING, ID_EXIT, w!("Exit"));
        let _ = SetMenuDefaultItem(m, ID_OPEN as u32, 0);

        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let _ = SetForegroundWindow(hwnd());
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, p.x, p.y, 0, hwnd(), None).0 as usize;
        let _ = PostMessageW(hwnd(), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m); // also destroys the submenu

        if (ID_THEME0..ID_THEME0 + THEME_COUNT).contains(&cmd) {
            apply_theme(cmd - ID_THEME0);
            return;
        }
        match cmd {
            ID_OPEN => show(),
            ID_INTERCEPT => {
                let on = !INTERCEPT.load(Relaxed);
                INTERCEPT.store(on, Relaxed);
                write_dword("Intercept", on as u32);
            }
            ID_EXPANDED => {
                s.expanded = !s.expanded;
                write_dword("Expanded", s.expanded as u32);
            }
            ID_AUTOSTART => set_auto_start(!is_auto_start()),
            ID_EXIT => {
                let _ = DestroyWindow(hwnd());
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Window procedure
// ---------------------------------------------------------------------------
static WM_TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn lparam_xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if ST.is_null() {
        return DefWindowProcW(h, msg, wp, lp);
    }
    let tc = WM_TASKBAR_CREATED.load(Relaxed);
    if tc != 0 && msg == tc {
        add_tray_icon();
        return LRESULT(0);
    }
    match msg {
        WM_APP_TOGGLE => toggle(),
        WM_APP_HIDE => hide(),
        WM_APP_QUIT => {
            let _ = DestroyWindow(h);
        }
        WM_HOTKEY => toggle(),
        WM_CLIPBOARDUPDATE => tools::clip_changed(),
        WM_APP_PANEL => {
            if wp.0 == 2 {
                panels::flash("Windows didn't allow that");
            }
            let m = st().mode;
            if m == Mode::Panel {
                panels::fill(false);
            } else if wp.0 == 1 && (m == Mode::None || m == Mode::Results) {
                update_state();
            }
        }
        WM_TIMER => {
            if wp.0 == TIMER_TICK {
                widgets::timer_tick();
                return LRESULT(0);
            }
            if wp.0 == TIMER_UPDATE_CHECK {
                // Ask Setup (installed next to us) whether GitHub has a newer version; it stays
                // hidden unless there is one, then shows "Update to V<x>?". Never in the middle
                // of a game: wait until nothing is fullscreen.
                if FULLSCREEN.load(Relaxed) || foreground_is_fullscreen() {
                    SetTimer(h, TIMER_UPDATE_CHECK, 5 * 60 * 1000, None);
                    return LRESULT(0);
                }
                SetTimer(h, TIMER_UPDATE_CHECK, 6 * 60 * 60 * 1000, None);
                if let Ok(exe) = std::env::current_exe() {
                    let setup = exe.with_file_name("CSMInstaller.exe");
                    if setup.exists() {
                        ShellExecuteW(
                            HWND::default(),
                            w!("open"),
                            &HSTRING::from(setup.as_os_str()),
                            w!("--check-update"),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );
                    }
                }
            }
        }
        WM_APP_APPS => {
            let m = st().mode;
            if m == Mode::Results || m == Mode::None {
                update_state();
            }
        }
        WM_APP_GAMES => {
            let s = st();
            if s.mode == Mode::Games {
                let sel = s.sel;
                update_state();
                s.sel = sel.clamp(0, (item_count() - 1).max(0));
            }
        }
        WM_APP_FONT => {
            let id = Box::from_raw(lp.0 as *mut String);
            fonts::downloaded(&id, wp.0 != 0);
        }
        WM_APP_PINS => {
            let s = st();
            let imported = IMPORTED_PINS.lock().unwrap().take().unwrap_or_default();
            if s.pins.is_empty() {
                for a in imported {
                    if find_pin(&a.target) >= 0 {
                        continue;
                    }
                    s.pins.push(Pin { app: a, ..Default::default() });
                }
            }
            save_pins(); // saved even when empty, so Windows' list is only copied once
            publish_box();
        }
        WM_APP_TRAY => match (lp.0 & 0xFFFF) as u32 {
            WM_LBUTTONUP => {
                // A tray click first deactivates the open bar; don't immediately re-open it.
                if now() - st().last_hide_time > 0.3 {
                    toggle();
                }
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => show_tray_menu(),
            _ => {}
        },
        WM_ACTIVATE => {
            if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                hide();
            }
        }
        WM_MOUSEACTIVATE => return LRESULT(MA_ACTIVATE as isize),
        WM_CHAR => on_char(wp.0 as u16),
        WM_KEYDOWN => on_key(wp.0 as u16),
        WM_LBUTTONDOWN => {
            // window pixels -> bar space
            let s = st();
            let k = 96.0 / s.dpi;
            let (px, py) = lparam_xy(lp);
            let (x, y) = (px as f32 * k, py as f32 * k - PINS_MAX_H);
            if s.menu_open {
                let mi = menu_item_at(x, y);
                if mi >= 0 {
                    run_menu_action(mi);
                } else if mi == -2 {
                    close_menu();
                }
                return LRESULT(0);
            }
            let pin = pin_at(x, y);
            if pin >= 0 {
                s.press_pin = pin;
                s.press_pt = pt(x, y);
                s.drag_pt = pt(x, y);
                s.grab = pt(x - s.pins[pin as usize].x, y - s.pins[pin as usize].y);
                s.dragging = false;
                SetCapture(h);
                return LRESULT(0);
            }
            if x < MARGIN || x > MARGIN + bar_w() || y < MARGIN || y > MARGIN + BAR_H + s.panel_h {
                hide();
                return LRESULT(0);
            }
            if y > MARGIN + BAR_H {
                if grid::hit(x, y) {
                    // grab the thumb, or jump to where the track was clicked
                    s.sb.drag = true;
                    s.sb.grab = if grid::on_thumb(y) { y - s.sb.thumb_y } else { s.sb.thumb_h / 2.0 };
                    SetCapture(h);
                    scroll_bar_drag_to(y);
                    return LRESULT(0);
                }
                if s.mode == Mode::Fonts {
                    fonts::click(x, y);
                    return LRESULT(0);
                }
                if s.mode == Mode::Caret {
                    caret::click(x, y);
                    return LRESULT(0);
                }
                if let (Mode::Panel, Some(c)) = (s.mode, panels::cur().and_then(|p| p.click)) {
                    if c(x, y) {
                        return LRESULT(0);
                    }
                }
                let idx = hit_item_at(x, y);
                if idx >= 0 {
                    select(idx);
                    st().sel = idx;
                    activate(false, false);
                }
                return LRESULT(0);
            }
            if s.text.is_empty() {
                // the clock, weather... on the empty bar
                if let Some(cmd) = widgets::strip_at(x, y) {
                    set_text(&ws(&format!("> {}", cmd)));
                    return LRESULT(0);
                }
            }
            SetCapture(h);
            move_caret(hit_text_index(x), key_down(VK_SHIFT.0 as i32));
        }
        WM_RBUTTONDOWN => {}
        WM_RBUTTONUP => {
            let s = st();
            let k = 96.0 / s.dpi;
            let (px, py) = lparam_xy(lp);
            let (x, y) = (px as f32 * k, py as f32 * k - PINS_MAX_H);
            if s.dragging || (s.menu_open && menu_item_at(x, y) != -2) {
                return LRESULT(0);
            }
            close_menu();
            let pin = pin_at(x, y);
            if pin >= 0 {
                let a = s.pins[pin as usize].app.clone();
                open_menu(&a, pin, pt(x, y));
                return LRESULT(0);
            }
            if is_app_list(s.mode) && y > MARGIN + BAR_H {
                let idx = hit_item_at(x, y);
                if idx >= 0 && (idx as usize) < s.results.len() {
                    select(idx);
                    s.sel = idx;
                    let a = s.results[idx as usize].clone();
                    open_menu(&a, -1, pt(x, y));
                }
            }
        }
        WM_MOUSEWHEEL => {
            let s = st();
            let mut p = POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 };
            let _ = ScreenToClient(h, &mut p);
            let y = p.y as f32 * 96.0 / s.dpi - PINS_MAX_H;
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut steps = -delta / WHEEL_DELTA as i32;
            if steps == 0 {
                steps = if delta > 0 { -1 } else { 1 };
            }
            close_menu();
            if y < MARGIN {
                // over the pins: page through rows
                s.pin_row_off = (s.pin_row_off + if steps > 0 { 1 } else { -1 }).clamp(0, (pin_rows_total() - pin_rows_shown()).max(0));
                for pp in s.pins.iter_mut() {
                    pp.placed = false;
                }
            } else if s.mode == Mode::Fonts {
                fonts::wheel(steps);
            } else if s.mode == Mode::Caret {
                caret::wheel(steps);
            } else if let (Mode::Panel, Some(wh)) = (s.mode, panels::cur().and_then(|p| p.wheel)) {
                wh(steps);
            } else if is_app_list(s.mode) || s.mode == Mode::Commands || (s.mode == Mode::Panel && panels::has_rows()) {
                let n = item_count();
                let mr = MAX_ROWS as i32;
                s.list_top = (s.list_top + steps * 3).clamp(0, (n - mr).max(0));
                s.sel = s.sel.clamp(s.list_top, (n - 1).min(s.list_top + mr - 1));
                recompute_ghost();
            }
        }
        WM_MOUSEMOVE => {
            let s = st();
            let (mx, my) = lparam_xy(lp);
            if mx == s.last_mouse.x && my == s.last_mouse.y {
                return LRESULT(0);
            }
            s.last_mouse = POINT { x: mx, y: my };
            let k = 96.0 / s.dpi;
            let (x, y) = (mx as f32 * k, my as f32 * k - PINS_MAX_H);
            if s.press_pin >= 0 && GetCapture() == h {
                s.drag_pt = pt(x, y);
                if !s.dragging && (x - s.press_pt.x).hypot(y - s.press_pt.y) > 6.0 {
                    s.dragging = true;
                }
                if s.dragging {
                    // move the dragged pin to the slot nearest to where its tile is now
                    let vis = visible_pins();
                    let n_vis = vis.len() as i32;
                    let (mut from, mut best, mut best_d) = (-1i32, -1i32, 1e9f32);
                    let cx = x - s.grab.x + tile_w() / 2.0;
                    let cy = y - s.grab.y + TILE_H / 2.0;
                    for kk in 0..n_vis {
                        if vis[kk as usize] as i32 == s.press_pin {
                            from = kk;
                        }
                        let row = kk / PIN_COLS;
                        if row < s.pin_row_off || row >= s.pin_row_off + pin_rows_shown() {
                            continue;
                        }
                        let sl = pin_slot(kk, n_vis);
                        let d = (sl.x + tile_w() / 2.0 - cx).hypot(sl.y + TILE_H / 2.0 - cy);
                        if d < best_d {
                            best_d = d;
                            best = kk;
                        }
                    }
                    if from >= 0 && best >= 0 && best != from {
                        let a = vis[from as usize];
                        let mut b = vis[best as usize];
                        let moving = s.pins.remove(a);
                        if b > a {
                            b -= 1;
                        }
                        let at = if best > from { b + 1 } else { b };
                        let target = moving.app.target.clone();
                        s.pins.insert(at, moving);
                        s.press_pin = find_pin(&target);
                    }
                }
                return LRESULT(0);
            }
            if s.sb.drag {
                scroll_bar_drag_to(y);
                return LRESULT(0);
            }
            if (wp.0 & 0x0001) != 0 && GetCapture() == h {
                move_caret(hit_text_index(x), true);
                return LRESULT(0);
            }
            if s.menu_open {
                let mi = menu_item_at(x, y);
                if mi >= 0 {
                    s.menu_sel = mi;
                }
                return LRESULT(0);
            }
            s.hover_pin = pin_at(x, y);
            fonts::hover(x, y);
            s.sb.hot = grid::hit(x, y);
            let idx = if s.sb.hot { -1 } else { hit_item_at(x, y) };
            if idx >= 0 {
                select(idx);
            }
            if !s.tracking_leave {
                let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                s.tracking_leave = TrackMouseEvent(&mut tme).is_ok();
            }
        }
        WM_MOUSELEAVE => {
            let s = st();
            s.tracking_leave = false;
            s.hover_pin = -1;
            s.fonts.btn_hot = -1;
            s.fonts.default_hot = false;
            s.sb.hot = false;
        }
        WM_LBUTTONUP => {
            st().sb.drag = false;
            if GetCapture() == h {
                let _ = ReleaseCapture();
            }
            let s = st();
            if s.press_pin >= 0 {
                let pin = s.press_pin;
                let dragged = s.dragging;
                s.press_pin = -1;
                s.dragging = false;
                if dragged {
                    save_pins();
                } else {
                    let k = 96.0 / s.dpi;
                    let (px, py) = lparam_xy(lp);
                    if pin_at(px as f32 * k, py as f32 * k - PINS_MAX_H) == pin {
                        let a = s.pins[pin as usize].app.clone();
                        launch(Some(&a), &[], false);
                    }
                }
            }
        }
        WM_SETCURSOR => {
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            let _ = ScreenToClient(h, &mut p);
            let k = 96.0 / st().dpi;
            let (x, y) = (p.x as f32 * k, p.y as f32 * k - PINS_MAX_H);
            let s = st();
            let hand = !s.menu_open
                && ((s.text.is_empty() && widgets::strip_at(x, y).is_some())
                    || (s.mode == Mode::Panel && widgets::button_at(x, y) >= 0)
                    || fonts::hand_at(x, y));
            let text = !hand && !s.menu_open && x >= MARGIN + TEXT_X - 4.0 && x <= MARGIN + bar_w() && y >= MARGIN && y <= MARGIN + BAR_H;
            if let Ok(cur) = LoadCursorW(HINSTANCE::default(), if text { IDC_IBEAM } else if hand { IDC_HAND } else { IDC_ARROW }) {
                SetCursor(cur);
            }
            return LRESULT(1);
        }
        WM_CAPTURECHANGED => {
            st().sb.drag = false;
            return DefWindowProcW(h, msg, wp, lp);
        }
        WM_DPICHANGED => {} // sizing is handled when the bar is shown
        WM_CLOSE => hide(),
        WM_DESTROY => PostQuitMessage(0),
        _ => return DefWindowProcW(h, msg, wp, lp),
    }
    LRESULT(0)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------
fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let mutex = CreateMutexW(None, TRUE, w!("Local\\CustomStartMenu.Singleton")).unwrap_or_default();
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(other) = FindWindowW(CLASS_NAME, PCWSTR::null()) {
                let _ = PostMessageW(other, WM_APP_TOGGLE, WPARAM(0), LPARAM(0));
            }
            return;
        }

        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let mut f = 0i64;
        let _ = QueryPerformanceFrequency(&mut f);
        QPF = f as f64;

        let d2d: ID2D1Factory1 = match D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) {
            Ok(f) => f,
            Err(_) => {
                MessageBoxW(HWND::default(), w!("Direct2D / DirectWrite unavailable."), w!("Custom Start"), MB_ICONERROR);
                return;
            }
        };
        let dw: IDWriteFactory = match DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) {
            Ok(f) => f,
            Err(_) => {
                MessageBoxW(HWND::default(), w!("Direct2D / DirectWrite unavailable."), w!("Custom Start"), MB_ICONERROR);
                return;
            }
        };

        let fm = build_formats(&dw);

        custom::load_custom_theme();
        custom::load_sound_settings();
        custom::load_layout_settings();
        custom::load_hotkey_settings();
        register::register_panels();
        let theme_idx = (read_dword("Theme", 0) as usize).min(THEME_COUNT - 1);
        // Hardware acceleration (Setup asks; on by default). Falls back to software if there
        // is no usable Direct3D 11 device at all.
        let hw = read_dword("HardwareAcceleration", 1) != 0 && create_d3d_device().is_some();
        let state = Box::new(State {
            fmt_text: fm.text,
            fmt_hint: fm.hint,
            fmt_row: fm.row,
            fmt_row_right: fm.row_right,
            fmt_glyph: fm.glyph,
            fmt_glyph_sm: fm.glyph_sm,
            fmt_big: fm.big,
            fmt_big_c: fm.big_c,
            fmt_mid: fm.mid,
            fmt_small: fm.small,
            fmt_small_c: fm.small_c,
            fmt_small_r: fm.small_r,
            fmt_hint_l: fm.hint_l,
            fmt_emoji: fm.emoji,
            fmt_header: fm.header,
            fmt_card: fm.card,
            fmt_tile: fm.tile,
            fmt_menu: fm.menu,
            placeholder_layout: fm.placeholder,
            d2d,
            dw,
            hw,
            rt: None,
            dcrt: None,
            d3d: None,
            d2d_dev: None,
            ctx: None,
            swap: None,
            target: None,
            dcomp: None,
            dc_target: None,
            dc_visual: None,
            win_h: -1,
            solid: None,
            logo_brush: None,
            bar_shadow: Shadow::new(SHADOW_SIGMA, 14.0, SHADOW_OFFSET_Y),
            tile_shadow: Shadow::new(7.0, 12.0, 3.0),
            bd_px: Vec::new(),
            bd_w: 0,
            bd_h: 0,
            bd_dirty: false,
            bd_bmp: None,
            bd_brush: None,
            mem_dc: CreateCompatibleDC(HDC::default()),
            dib: HBITMAP::default(),
            old_bmp: HGDIOBJ::default(),
            px_w: 0,
            px_h: 0,
            dpi: 96.0,
            win_pos: POINT::default(),
            theme_idx,
            expanded: read_dword("Expanded", 1) != 0,
            autostart: is_auto_start(),
            preview_idx: theme_idx,
            th: themes()[theme_idx],
            th_from: themes()[theme_idx],
            th_start: -10.0,
            visible: false,
            anim: 0.0,
            anim_from: 0.0,
            anim_to: 0.0,
            anim_start: 0.0,
            anim_dur: 0.001,
            show_time: 0.0,
            last_frame: 0.0,
            last_hide_time: 0.0,
            text: Vec::new(),
            birth: Vec::new(),
            caret: 0,
            anchor: 0,
            last_input: 0.0,
            caret_x: 0.0,
            trail_x: 0.0,
            scroll: 0.0,
            scroll_target: 0.0,
            placeholder_a: 1.0,
            caret_init: false,
            mode: Mode::None,
            draw_mode: Mode::None,
            sel: 0,
            panel_h: 0.0,
            content_a: 0.0,
            sel_y: 0.0,
            sel_y_init: false,
            card_sel: [0.0; THEME_COUNT],
            hits: Vec::new(),
            last_mouse: POINT { x: -1, y: -1 },
            apps_loaded_at: -1e9,
            results: Vec::new(),
            cmd_list: Vec::new(),
            has_match: false,
            match_app: App::default(),
            ghost: Vec::new(),
            hint_name: Vec::new(),
            hint_time: 0.0,
            icon_px: 32,
            tile_px: 32,
            icon_bmps: HashMap::new(),
            pins: Vec::new(),
            pin_row_off: 0,
            press_pin: -1,
            dragging: false,
            press_pt: pt(0.0, 0.0),
            drag_pt: pt(0.0, 0.0),
            grab: pt(0.0, 0.0),
            hover_pin: -1,
            pin_hits: Vec::new(),
            tracking_leave: false,
            games_at: -1e9,
            list_off: 0.0,
            sel_row: 0.0,
            list_top: 0,
            menu: Vec::new(),
            menu_open: false,
            menu_app: App::default(),
            menu_pin: -1,
            menu_pos: pt(0.0, 0.0),
            menu_time: 0.0,
            menu_sel: -1,
            menu_hl: 0.0,
            menu_hl_init: false,
            prev_fg: HWND::default(),
            fonts: fonts::FontUi { btn_hot: -1, grid: grid::CardGrid::new(3, 64.0, 26.0), ..Default::default() },
            cfx: caret::CaretUi::default(),
            sb: grid::ScrollBar::default(),
        });
        ST = Box::into_raw(state);
        *ICONS.lock().unwrap() = Some(IconCache { queue: VecDeque::new(), map: HashMap::new(), quit: false });

        let inst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).map(|m| HINSTANCE(m.0)).unwrap_or_default();
        let sys_dpi = GetDpiForSystem();
        TRAY_ICON = make_logo_icon(GetSystemMetricsForDpi(SM_CXSMICON, sys_dpi));
        let big_icon = make_logo_icon(GetSystemMetricsForDpi(SM_CXICON, sys_dpi));

        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: inst,
            hCursor: LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default(),
            hIcon: LoadIconW(inst, PCWSTR(1 as *const u16)).unwrap_or(big_icon),
            hIconSm: TRAY_ICON,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let ex_style = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | if hw { WS_EX_NOREDIRECTIONBITMAP } else { WS_EX_LAYERED };
        let Ok(h) = CreateWindowExW(
            ex_style,
            CLASS_NAME,
            w!("Custom Start"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            HWND::default(),
            HMENU::default(),
            inst,
            None,
        ) else {
            return;
        };
        HWND_G.store(h.0 as isize, Relaxed);

        WM_TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Relaxed);
        add_tray_icon();
        tools::clip_init();
        custom::apply_hotkey();
        SetTimer(h, TIMER_UPDATE_CHECK, 20 * 1000, None);

        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let hook_thread = std::thread::spawn(move || hook_thread(ready_tx));
        let _ = ready_rx.recv_timeout(std::time::Duration::from_secs(2));

        let quit_event = CreateEventW(None, TRUE, FALSE, PCWSTR::null()).unwrap_or_default();
        QUIT_EVENT.store(quit_event.0 as isize, Relaxed);
        let uia_thread = std::thread::spawn(start_button_thread);
        let icon_thread = std::thread::spawn(icon_worker);
        fonts::load_catalog();
        caret::load_settings();
        fonts::start_workers();
        refresh_apps(true);
        if !load_pins() {
            start_import(); // first run: start from the Windows Start menu's pins
        }

        // Message loop: block while hidden; while visible, pump messages and present one
        // frame per display refresh (DwmFlush waits for the next composition pass).
        let mut msg = MSG::default();
        'outer: loop {
            if st().visible {
                while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'outer;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if st().visible {
                    // The swap chain's Present(1) already waits for the next refresh; the layered
                    // window (or a Present that returned early) waits on the compositor instead.
                    let t0 = now();
                    render();
                    if (!st().hw || now() - t0 < 0.004) && DwmFlush().is_err() {
                        std::thread::sleep(std::time::Duration::from_millis(8));
                    }
                }
            } else {
                let r = GetMessageW(&mut msg, HWND::default(), 0, 0);
                if r.0 <= 0 {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        let _ = SetEvent(quit_event);
        if let Some(c) = ICONS.lock().unwrap().as_mut() {
            c.quit = true;
        }
        ICON_CV.notify_all();
        let _ = icon_thread.join();
        fonts::stop_workers();
        // The UI Automation thread wakes on the quit event; don't wait long for it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !uia_thread.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let tid = HOOK_TID.load(Relaxed);
        if tid != 0 {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            let _ = hook_thread.join();
        }
        remove_tray_icon();
        discard_device_resources();
        let s = st();
        if !s.dib.is_invalid() {
            SelectObject(s.mem_dc, s.old_bmp);
            let _ = DeleteObject(HGDIOBJ(s.dib.0));
        }
        let _ = DeleteDC(s.mem_dc);
        let _ = DestroyIcon(TRAY_ICON);
        let _ = DestroyIcon(big_icon);
        let _ = ReleaseMutex(mutex);
        let _ = CloseHandle(mutex);
        std::process::exit(0);
    }
}
