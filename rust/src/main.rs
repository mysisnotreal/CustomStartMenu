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
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering::*};
use std::sync::{Condvar, Mutex, RwLock};

use windows::core::{w, HSTRING, PCWSTR, PWSTR, VARIANT};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
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

// ---------------------------------------------------------------------------
// Layout (device independent pixels)
// ---------------------------------------------------------------------------
const BAR_W: f32 = 620.0;
const BAR_H: f32 = 56.0;
const MARGIN: f32 = 52.0; // room around the container for the shadow / animation
const RADIUS: f32 = 14.0;
const VERTICAL_POS: f32 = 0.40; // bar center, as a fraction of work-area height
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

const WIN_W: f32 = BAR_W + 2.0 * MARGIN;
const WIN_MAX_H: f32 = BAR_H + MAX_PANEL_H + 2.0 * MARGIN;

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

const THEMES: [Theme; 8] = [
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
];
const THEME_COUNT: usize = THEMES.len();

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
const CMDS: [CmdDef; 4] = [
    CmdDef { key: "theme", title: "Theme", glyph: "\u{E790}" },
    CmdDef { key: "expanded", title: "Expanded results", glyph: "\u{E8FD}" },
    CmdDef { key: "startup", title: "Start with Windows", glyph: "\u{E7E8}" },
    CmdDef { key: "quit", title: "Quit Custom Start", glyph: "\u{E711}" },
];
const CMD_THEME: usize = 0;
const CMD_EXPANDED: usize = 1;
const CMD_STARTUP: usize = 2;
const CMD_QUIT: usize = 3;

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
const TIMER_UPDATE_CHECK: usize = 1;

const ID_OPEN: usize = 1;
const ID_AUTOSTART: usize = 2;
const ID_INTERCEPT: usize = 3;
const ID_EXPANDED: usize = 4;
const ID_EXIT: usize = 5;
const ID_THEME0: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    None,
    Results,
    Commands,
    Themes,
}

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
#[derive(Clone, Default)]
struct App {
    name: WStr,
    lower: WStr,
    initials: WStr,
    target: WStr,
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

// keyboard / mouse hook state (hooks run on the UI thread, while it pumps messages)
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
    d2d: ID2D1Factory,
    dw: IDWriteFactory,
    fmt_text: IDWriteTextFormat,
    fmt_hint: IDWriteTextFormat,
    fmt_row: IDWriteTextFormat,
    fmt_row_right: IDWriteTextFormat,
    fmt_glyph: IDWriteTextFormat,
    fmt_header: IDWriteTextFormat,
    fmt_card: IDWriteTextFormat,
    placeholder_layout: Option<IDWriteTextLayout>,

    rt: Option<ID2D1DCRenderTarget>,
    solid: Option<ID2D1SolidColorBrush>,
    logo_brush: Option<ID2D1LinearGradientBrush>,
    shadow_bmp: Option<ID2D1Bitmap>,
    shadow_dpi: f32,
    shadow_c: i32,
    shadow_n: i32,
    shadow_pad: i32,

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
    icon_bmps: HashMap<String, (ID2D1Bitmap, f64)>,
}

static mut ST: *mut State = null_mut();
#[inline]
fn st() -> &'static mut State {
    unsafe { &mut *ST }
}
fn rt() -> &'static ID2D1DCRenderTarget {
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
        s.th = THEMES[s.theme_idx];
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
                    apps.push(App { initials: initials(&lw), lower: lw, name: nv, target: pwstr_to_vec(*p) });
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
    match find(&a.lower, q) {
        Some(0) => base = 3000,
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
    unsafe {
        let path = HSTRING::from(format!("shell:AppsFolder\\{}", target));
        let f: IShellItemImageFactory = SHCreateItemFromParsingName(&path, None::<&IBindCtx>).ok()?;
        let hb = f.GetImage(SIZE { cx: size, cy: size }, SIIGBF_ICONONLY).ok()?;
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
fn get_icon(target: &[u16]) -> Option<(ID2D1Bitmap, f64)> {
    let s = st();
    let key = format!("{}|{}", String::from_utf16_lossy(target), s.icon_px);
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
fn command_detail(c: usize) -> &'static str {
    let s = st();
    match c {
        CMD_THEME => THEMES[s.theme_idx].name,
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
            let key = ws(CMDS[s.cmd_list[s.sel as usize]].key);
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

    let tl = lower(&trim(&s.text));
    if !tl.is_empty() && tl[0] == b'>' as u16 {
        let q = trim(&tl[1..]);
        if q == ws("theme") || q == ws("themes") || starts_with(&q, &ws("theme ")) {
            s.mode = Mode::Themes;
        } else {
            s.mode = Mode::Commands;
            for (i, cmd) in CMDS.iter().enumerate() {
                if q.is_empty() || starts_with(&ws(cmd.key), &q) || find(&lower(&ws(cmd.title)), &q).is_some() {
                    s.cmd_list.push(i);
                }
            }
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
    } else {
        s.mode = Mode::None;
    }

    if s.mode != old {
        if old == Mode::Themes {
            set_preview(s.theme_idx); // leaving without applying reverts the preview
        }
        s.sel = if s.mode == Mode::Themes { s.theme_idx as i32 } else { 0 };
        if s.mode != Mode::None {
            s.content_a = if old == Mode::None { 0.0 } else { s.content_a.min(0.35) };
            s.draw_mode = s.mode;
            s.sel_y_init = false;
        }
    } else if s.mode == Mode::Results || s.mode == Mode::Commands {
        s.sel = 0;
    }
    recompute_ghost();

    let mut hint = Vec::new();
    if s.mode == Mode::None && s.has_match && !starts_with(&s.match_app.lower, &lower(&s.text)) {
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
        Mode::Results => s.results.len() as i32,
        Mode::Commands => s.cmd_list.len() as i32,
        Mode::Themes => THEME_COUNT as i32,
        Mode::None => 0,
    }
}

fn theme_grid_height() -> f32 {
    let rows = THEME_COUNT.div_ceil(GRID_COLS) as f32;
    GRID_PAD + GRID_HEADER_H + rows * (CARD_PREVIEW_H + CARD_NAME_H) + (rows - 1.0) * 4.0 + GRID_PAD - 4.0
}

fn panel_target_h() -> f32 {
    let s = st();
    match s.mode {
        Mode::Results => PANEL_PAD * 2.0 + ROW_H * s.results.len() as f32,
        Mode::Commands => {
            if s.cmd_list.is_empty() {
                0.0
            } else {
                PANEL_PAD * 2.0 + ROW_H * s.cmd_list.len() as f32
            }
        }
        Mode::Themes => theme_grid_height(),
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

fn start_button_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: Option<IUIAutomation> = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok();
        let cond = uia.as_ref().and_then(|u| {
            let v = VARIANT::from(windows::core::BSTR::from("StartButton"));
            u.CreatePropertyCondition(UIA_AutomationIdPropertyId, &v).ok()
        });
        if let (Some(uia), Some(cond)) = (&uia, &cond) {
            loop {
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
                let ev = HANDLE(QUIT_EVENT.load(Relaxed) as *mut c_void);
                if WaitForSingleObject(ev, 2500) != WAIT_TIMEOUT {
                    break;
                }
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

fn container_screen_rect() -> RECT {
    let s = st();
    let sc = s.dpi / 96.0;
    let left = s.win_pos.x + (MARGIN * sc) as i32;
    let top = s.win_pos.y + (MARGIN * sc) as i32;
    RECT { left, top, right: left + (BAR_W * sc) as i32, bottom: top + ((BAR_H + s.panel_h) * sc) as i32 }
}

unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && !ST.is_null() {
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
            if (msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN) && st().visible && st().anim_to > 0.5 {
                let r = container_screen_rect();
                if !pt_in(&r, m.pt) {
                    let _ = PostMessageW(hwnd(), WM_APP_HIDE, WPARAM(0), LPARAM(0));
                }
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wp, lp)
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

fn make_format(dw: &IDWriteFactory, fam: &str, size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT, ellipsis: bool) -> IDWriteTextFormat {
    unsafe {
        let f = dw
            .CreateTextFormat(
                &HSTRING::from(fam),
                None::<&IDWriteFontCollection>,
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
    s.solid = None;
    s.logo_brush = None;
    s.shadow_bmp = None;
    s.bd_brush = None;
    s.bd_bmp = None;
    s.bd_dirty = !s.bd_px.is_empty();
    s.shadow_dpi = 0.0;
    s.icon_bmps.clear();
    s.rt = None;
}

fn create_device_resources() -> bool {
    let s = st();
    if s.rt.is_some() {
        return true;
    }
    unsafe {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: pixel_format(),
            dpiX: 0.0,
            dpiY: 0.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let Ok(rt) = s.d2d.CreateDCRenderTarget(&props) else { return false };
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
fn build_shadow() {
    let s = st();
    if s.shadow_bmp.is_some() && s.shadow_dpi == s.dpi {
        return;
    }
    s.shadow_bmp = None;
    let sc = s.dpi / 96.0;
    let sigma = SHADOW_SIGMA * sc;
    let pad = (sigma * 3.0).ceil() as i32;
    let rr = RADIUS * sc;
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
        s.shadow_bmp = rt()
            .CreateBitmap(D2D_SIZE_U { width: n as u32, height: n as u32 }, Some(px.as_ptr() as *const c_void), (n * 4) as u32, &bp)
            .ok();
    }
    s.shadow_c = cc;
    s.shadow_n = n as i32;
    s.shadow_pad = pad;
    s.shadow_dpi = s.dpi;
}

fn ensure_surface(w: i32, h: i32) {
    let s = st();
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
        let mon = MonitorFromPoint(p, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        s.dpi = dx as f32;
        let sc = s.dpi / 96.0;
        let w = (WIN_W * sc).ceil() as i32;
        let h = (WIN_MAX_H * sc).ceil() as i32;
        ensure_surface(w, h);
        if let Some(rt) = &s.rt {
            rt.SetDpi(s.dpi, s.dpi);
        }
        s.icon_px = (24.0 * sc).round() as i32;

        // Centered horizontally, sitting a little above the vertical middle of the screen.
        let wa = mi.rcWork;
        let bar_center_y = wa.top + ((wa.bottom - wa.top) as f32 * VERTICAL_POS).round() as i32;
        s.win_pos.x = (wa.left + wa.right) / 2 - w / 2;
        s.win_pos.y = bar_center_y - ((MARGIN + BAR_H / 2.0) * sc).round() as i32;
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

fn draw_shadow(bx: D2D_RECT_F, opacity: f32) {
    let s = st();
    let Some(bmp) = s.shadow_bmp.as_ref() else { return };
    if opacity <= 0.001 {
        return;
    }
    let sc = s.dpi / 96.0;
    let pad = s.shadow_pad as f32 / sc;
    let cd = s.shadow_c as f32 / sc;
    let o = rect(bx.left - pad, bx.top - pad + SHADOW_OFFSET_Y, bx.right + pad, bx.bottom + pad + SHADOW_OFFSET_Y);
    let dx = [o.left, o.left + cd, o.right - cd, o.right];
    let dy = [o.top, o.top + cd, o.bottom - cd, o.bottom];
    let sx = [0.0, s.shadow_c as f32, s.shadow_c as f32 + 1.0, s.shadow_n as f32];
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
    let right_edge = MARGIN + BAR_W - RIGHT_PAD;
    let cy = MARGIN + BAR_H / 2.0;

    unsafe {
        // Right-hand hint: shows which app Enter will open when it isn't an inline completion.
        let mut hint_w = 0.0f32;
        if !s.hint_name.is_empty() {
            let mut hint = s.hint_name.clone();
            hint.extend(ws("  ↵"));
            if let Some(hl) = make_layout(&hint, &s.fmt_hint, HINT_MAX_W, BAR_H) {
                let mut tm = DWRITE_TEXT_METRICS::default();
                let _ = hl.GetMetrics(&mut tm);
                hint_w = tm.widthIncludingTrailingWhitespace.min(HINT_MAX_W);
                let o = clamp01((t_now - s.hint_time) / 0.16);
                set_color(with_a(th.dim, ease_out_cubic(o as f64) as f32));
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

        // Caret: smooth glide + soft trailing smear + eased blink + small "pop" on input.
        let idle = t_now - s.last_input;
        let blink = if idle < 0.5 { 1.0 } else { (0.5 + 0.5 * ((idle - 0.5) * 2.0 * std::f64::consts::PI / 1.05).cos()) as f32 };
        let intro = clamp01((t_now - s.show_time - 0.10) / 0.22);
        let op = blink * ease_out_cubic(intro as f64) as f32;
        let h = CARET_H * (1.0 + 0.14 * (-idle * 16.0).exp() as f32);
        if op > 0.003 {
            let x0 = tx + s.caret_x.min(s.trail_x) - CARET_W / 2.0;
            let x1 = tx + s.caret_x.max(s.trail_x) + CARET_W / 2.0;
            if x1 - x0 > CARET_W + 0.6 {
                fill_rr(rect(x0, cy - h * 0.38, x1, cy + h * 0.38), 1.0, with_a(th.accent, op * 0.28));
            }
            fill_rr(rect(tx + s.caret_x - CARET_W / 2.0, cy - h / 2.0, tx + s.caret_x + CARET_W / 2.0, cy + h / 2.0), 1.0, with_a(th.accent, op));
        }

        rt().PopAxisAlignedClip();
    }
}

fn draw_layout(text: &[u16], fmt: &IDWriteTextFormat, x: f32, y: f32, w: f32, h: f32, col: D2D1_COLOR_F) {
    if let Some(l) = make_layout(text, fmt, w, h) {
        set_color(col);
        unsafe { rt().DrawTextLayout(pt(x, y), &l, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE) };
    }
}

// List rows (results / commands) with a sliding selection highlight.
fn draw_rows(top: f32, dt: f32, alpha: f32) {
    let s = st();
    let th = s.th;
    let n = if s.draw_mode == Mode::Results { s.results.len() } else { s.cmd_list.len() } as i32;
    if n == 0 {
        return;
    }
    let sel = s.sel.clamp(0, n - 1);
    let sel_target = top + PANEL_PAD + sel as f32 * ROW_H;
    if !s.sel_y_init {
        s.sel_y = sel_target;
        s.sel_y_init = true;
    }
    s.sel_y = damp(s.sel_y, sel_target, 26.0, dt);

    // highlight + accent pill
    fill_rr(rect(MARGIN + 6.0, s.sel_y + 2.0, MARGIN + BAR_W - 6.0, s.sel_y + ROW_H - 2.0), 8.0, with_a(th.highlight, alpha));
    fill_rr(
        rect(MARGIN + 8.0, s.sel_y + ROW_H / 2.0 - 8.0, MARGIN + 11.0, s.sel_y + ROW_H / 2.0 + 8.0),
        1.5,
        with_a(th.accent, alpha),
    );

    let q = lower(&trim(&s.text));
    let t_now = now();
    for i in 0..n {
        let slide = (1.0 - s.content_a) * (6.0 + i as f32 * 3.0);
        let y = top + PANEL_PAD + i as f32 * ROW_H + slide;
        let row_a = alpha * clamp01(s.content_a as f64 * 1.4 - i as f64 * 0.06);
        s.hits.push((rect(MARGIN + 6.0, y, MARGIN + BAR_W - 6.0, y + ROW_H), i));
        let is_sel = i == sel;
        let ix = MARGIN + 20.0;
        let iy = y + (ROW_H - 24.0) / 2.0;

        if s.draw_mode == Mode::Results {
            let a = s.results[i as usize].clone();
            let icon = get_icon(&a.target);
            let io = match &icon {
                Some((_, shown)) => ease_out_cubic(clamp01((t_now - shown) / 0.18) as f64) as f32,
                None => 0.0,
            };
            if io < 1.0 {
                fill_rr(rect(ix, iy, ix + 24.0, iy + 24.0), 6.0, with_a(th.highlight, row_a * (1.0 - io) * 1.5));
            }
            if let Some((bmp, _)) = &icon {
                unsafe {
                    rt().DrawBitmap(bmp, Some(&rect(ix, iy, ix + 24.0, iy + 24.0)), row_a * io, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None)
                };
            }
            if let Some(l) = make_layout(&a.name, &s.fmt_row, BAR_W - 120.0, ROW_H) {
                if !q.is_empty() {
                    if let Some(p) = find(&a.lower, &q) {
                        unsafe {
                            let _ = l.SetFontWeight(DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_RANGE { startPosition: p as u32, length: q.len() as u32 });
                        }
                    }
                }
                set_color(with_a(if is_sel { th.text } else { lerp_c(th.dim, th.text, 0.55) }, row_a));
                unsafe { rt().DrawTextLayout(pt(MARGIN + 56.0, y), &l, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE) };
            }
            if is_sel {
                draw_layout(&ws("Open  ↵"), &s.fmt_row_right, MARGIN + BAR_W - 140.0, y, 120.0, ROW_H, with_a(th.dim, row_a));
            }
        } else {
            let cmd = s.cmd_list[i as usize];
            draw_layout(&ws(CMDS[cmd].glyph), &s.fmt_glyph, ix, y, 24.0, ROW_H, with_a(if is_sel { th.accent } else { th.dim }, row_a));
            draw_layout(
                &ws(CMDS[cmd].title),
                &s.fmt_row,
                MARGIN + 56.0,
                y,
                BAR_W - 200.0,
                ROW_H,
                with_a(if is_sel { th.text } else { lerp_c(th.dim, th.text, 0.55) }, row_a),
            );
            let d = command_detail(cmd);
            let on = d == "On";
            draw_layout(&ws(d), &s.fmt_row_right, MARGIN + BAR_W - 180.0, y, 160.0, ROW_H, with_a(if on { th.accent } else { th.dim }, row_a));
        }
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
        MARGIN + BAR_W - GRID_PAD - 260.0,
        top + GRID_PAD - 4.0,
        258.0,
        GRID_HEADER_H,
        with_a(th.dim, alpha * 0.85),
    );

    let base = get_transform();
    let cw = (BAR_W - 2.0 * GRID_PAD - (GRID_COLS as f32 - 1.0) * CARD_GAP) / GRID_COLS as f32;
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
        let t = &THEMES[i];
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
    fill_rect(rect(MARGIN + 1.0, top, MARGIN + BAR_W - 1.0, top + 1.0), with_a(s.th.divider, clamp01(s.panel_h as f64 / 20.0)));

    unsafe {
        rt().PushAxisAlignedClip(&rect(MARGIN, top + 1.0, MARGIN + BAR_W, top + s.panel_h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
    if s.draw_mode == Mode::Themes {
        draw_theme_grid(top, dt, alpha, t_now);
    } else {
        draw_rows(top, dt, alpha);
    }
    unsafe { rt().PopAxisAlignedClip() };
}

fn render() {
    let s = st();
    if s.dib.is_invalid() || !create_device_resources() {
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
        unsafe {
            let _ = ShowWindow(hwnd(), SW_HIDE);
        }
        s.preview_idx = s.theme_idx;
        s.th = THEMES[s.theme_idx];
        s.th_from = s.th;
        return;
    }

    // theme cross-fade
    s.th = lerp_theme(&s.th_from, &THEMES[s.preview_idx], ease_out_cubic(clamp01((t_now - s.th_start) / 0.35) as f64) as f32);

    // panel expansion
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

    build_shadow();
    ensure_backdrop_brush();

    unsafe {
        let rc = RECT { left: 0, top: 0, right: s.px_w, bottom: s.px_h };
        if rt().BindDC(s.mem_dc, &rc).is_err() {
            return;
        }
        rt().BeginDraw();
        set_transform(&m_identity());
        rt().Clear(Some(&c(0, 0.0)));

        let a = s.anim as f32;
        let sc = 0.94 + 0.06 * a;
        let ty = (1.0 - a) * 16.0;
        let base = m_mul(&m_scale(sc, sc, MARGIN + BAR_W / 2.0, MARGIN + BAR_H / 2.0), &m_translate(0.0, ty));
        set_transform(&base);

        let bx = rect(MARGIN, MARGIN, MARGIN + BAR_W, MARGIN + BAR_H + s.panel_h);
        let rr = rrect(bx, RADIUS);
        draw_shadow(bx, s.th.shadow);
        let th = s.th;
        fill_acrylic(&rr, &th, MARGIN, MARGIN + BAR_H);

        draw_panel(t_now, dt);

        if let Some(b) = make_gradient(pt(0.0, MARGIN), pt(0.0, MARGIN + BAR_H), th.border_top, th.border_bottom) {
            let inner = rect(bx.left + 0.5, bx.top + 0.5, bx.right - 0.5, bx.bottom - 0.5);
            rt().DrawRoundedRectangle(&rrect(inner, RADIUS - 0.5), &b, 1.0, None::<&ID2D1StrokeStyle>);
        }

        if let Some(lb) = s.logo_brush.clone() {
            draw_logo_at(MARGIN + LOGO_X, MARGIN + (BAR_H - LOGO_SIZE) / 2.0, LOGO_SIZE, &lb, &base, t_now, true);
        }
        draw_text_area(t_now, dt);

        if let Err(e) = rt().EndDraw(None, None) {
            if e.code() == D2DERR_RECREATE_TARGET {
                discard_device_resources();
            }
            return;
        }

        let scale = s.dpi / 96.0;
        let cur_h = s.px_h.min(((MARGIN * 2.0 + BAR_H + s.panel_h) * scale).ceil() as i32);
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
        let _ = SetFocus(h);
    }
}

fn show() {
    let s = st();
    if s.visible && s.anim_to > 0.5 {
        return;
    }
    if !s.visible {
        position_on_monitor();
        capture_backdrop();
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
        s.th = THEMES[s.theme_idx];
        s.th_from = s.th;
        update_state();
    }
    start_anim(1.0, 0.32);
    s.visible = true;
    s.last_frame = now();
    s.last_input = s.last_frame;
    render();
    unsafe {
        let _ = SetWindowPos(hwnd(), HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
    }
    force_foreground(hwnd());
    refresh_apps(false);
}

fn hide() {
    let s = st();
    if !s.visible || s.anim_to < 0.5 {
        return;
    }
    start_anim(0.0, 0.18);
    s.last_hide_time = now();
    unsafe {
        if GetCapture() == hwnd() {
            let _ = ReleaseCapture();
        }
    }
}

fn toggle() {
    let s = st();
    if s.visible && s.anim_to > 0.5 {
        hide();
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
    let (file, params) = match app {
        Some(a) => (format!("shell:AppsFolder\\{}", String::from_utf16_lossy(&a.target)), String::new()),
        None => split_command(&qs),
    };
    let verb = if admin { "runas" } else { "open" };

    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    hide();
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let (fv, pv, vv) = (wz(&file), wz(&params), wz(verb));
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
            lpVerb: PCWSTR(vv.as_ptr()),
            lpFile: PCWSTR(fv.as_ptr()),
            lpParameters: if params.is_empty() { PCWSTR::null() } else { PCWSTR(pv.as_ptr()) },
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
        CMD_THEME => set_text(&ws("> theme")),
        CMD_EXPANDED => {
            s.expanded = !s.expanded;
            write_dword("Expanded", s.expanded as u32);
        }
        CMD_STARTUP => set_auto_start(!s.autostart),
        CMD_QUIT => unsafe {
            let _ = DestroyWindow(hwnd());
        },
        _ => {}
    }
}

// Enter / click on the current selection.
fn activate(force_raw: bool, admin: bool) {
    let s = st();
    let q = trim(&s.text);
    match s.mode {
        Mode::Themes => apply_theme(s.sel.max(0) as usize),
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
        Mode::None => {
            if q.is_empty() {
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
    let list = s.mode == Mode::Results || s.mode == Mode::Commands;
    let sel = s.sel;
    let gc = GRID_COLS as i32;
    match VIRTUAL_KEY(vk) {
        VK_ESCAPE => hide(),
        VK_RETURN => activate(shift && !ctrl, ctrl && shift),
        VK_TAB => {
            if s.mode == Mode::Commands && s.ghost.is_empty() && (sel as usize) < s.cmd_list.len() {
                set_text(&ws(&format!("> {}", CMDS[s.cmd_list[sel as usize]].key)));
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
        let Ok(themes) = CreatePopupMenu() else { return };
        let names: Vec<WStr> = THEMES.iter().map(|t| wz(t.name)).collect();
        for (i, n) in names.iter().enumerate() {
            let f = if i == s.theme_idx { MF_STRING | MF_CHECKED } else { MF_STRING };
            let _ = AppendMenuW(themes, f, ID_THEME0 + i, PCWSTR(n.as_ptr()));
        }
        let Ok(m) = CreatePopupMenu() else { return };
        let chk = |on: bool| if on { MF_STRING | MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(m, MF_STRING, ID_OPEN, w!("Open command bar"));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(m, MF_POPUP, themes.0 as usize, w!("Theme"));
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
            ID_INTERCEPT => INTERCEPT.store(!INTERCEPT.load(Relaxed), Relaxed),
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
        WM_TIMER => {
            if wp.0 == TIMER_UPDATE_CHECK {
                // Ask Setup (installed next to us) whether GitHub has a newer version; it stays
                // hidden unless there is one, then shows "Update to V<x>?".
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
            let s = st();
            let k = 96.0 / s.dpi;
            let (px, py) = lparam_xy(lp);
            let (x, y) = (px as f32 * k, py as f32 * k);
            if x < MARGIN || x > MARGIN + BAR_W || y < MARGIN || y > MARGIN + BAR_H + s.panel_h {
                hide();
                return LRESULT(0);
            }
            if y > MARGIN + BAR_H {
                let idx = hit_item_at(x, y);
                if idx >= 0 {
                    select(idx);
                    st().sel = idx;
                    activate(false, false);
                }
                return LRESULT(0);
            }
            SetCapture(h);
            move_caret(hit_text_index(x), key_down(VK_SHIFT.0 as i32));
        }
        WM_MOUSEMOVE => {
            let s = st();
            let (x, y) = lparam_xy(lp);
            if x == s.last_mouse.x && y == s.last_mouse.y {
                return LRESULT(0);
            }
            s.last_mouse = POINT { x, y };
            let k = 96.0 / s.dpi;
            if (wp.0 & 0x0001) != 0 && GetCapture() == h {
                move_caret(hit_text_index(x as f32 * k), true);
            } else {
                let idx = hit_item_at(x as f32 * k, y as f32 * k);
                if idx >= 0 {
                    select(idx);
                }
            }
        }
        WM_LBUTTONUP => {
            if GetCapture() == h {
                let _ = ReleaseCapture();
            }
        }
        WM_SETCURSOR => {
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            let _ = ScreenToClient(h, &mut p);
            let k = 96.0 / st().dpi;
            let (x, y) = (p.x as f32 * k, p.y as f32 * k);
            let text = x >= MARGIN + TEXT_X - 4.0 && x <= MARGIN + BAR_W && y >= MARGIN && y <= MARGIN + BAR_H;
            if let Ok(cur) = LoadCursorW(HINSTANCE::default(), if text { IDC_IBEAM } else { IDC_ARROW }) {
                SetCursor(cur);
            }
            return LRESULT(1);
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

        let d2d: ID2D1Factory = match D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) {
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

        let fam = if font_exists(&dw, "Segoe UI Variable Text") { "Segoe UI Variable Text" } else { "Segoe UI" };
        let icons = if font_exists(&dw, "Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
        let fmt_text = make_format(&dw, fam, FONT_SIZE, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING, false);
        let lang = read_string(SETTINGS_KEY, w!("Language")).unwrap_or_default();
        let placeholder = PLACEHOLDERS
            .iter()
            .find(|(code, _)| lang.len() >= 2 && lang[..2].eq_ignore_ascii_case(code))
            .map(|p| p.1)
            .unwrap_or(PLACEHOLDERS[0].1);
        let placeholder_layout = dw.CreateTextLayout(&ws(placeholder), &fmt_text, 10000.0, BAR_H).ok();

        let theme_idx = (read_dword("Theme", 0) as usize).min(THEME_COUNT - 1);
        let state = Box::new(State {
            fmt_hint: make_format(&dw, fam, HINT_SIZE, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING, true),
            fmt_row: make_format(&dw, fam, 14.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING, true),
            fmt_row_right: make_format(&dw, fam, 13.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING, false),
            fmt_glyph: make_format(&dw, icons, 16.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, false),
            fmt_header: make_format(&dw, fam, 12.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, false),
            fmt_card: make_format(&dw, fam, 12.5, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, true),
            fmt_text,
            placeholder_layout,
            d2d,
            dw,
            rt: None,
            solid: None,
            logo_brush: None,
            shadow_bmp: None,
            shadow_dpi: 0.0,
            shadow_c: 0,
            shadow_n: 0,
            shadow_pad: 0,
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
            th: THEMES[theme_idx],
            th_from: THEMES[theme_idx],
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
            icon_bmps: HashMap::new(),
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

        let Ok(h) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
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
        SetTimer(h, TIMER_UPDATE_CHECK, 20 * 1000, None);

        let kb_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kbd_proc), inst, 0).ok();
        let ms_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), inst, 0).ok();

        let quit_event = CreateEventW(None, TRUE, FALSE, PCWSTR::null()).unwrap_or_default();
        QUIT_EVENT.store(quit_event.0 as isize, Relaxed);
        let uia_thread = std::thread::spawn(start_button_thread);
        let icon_thread = std::thread::spawn(icon_worker);
        refresh_apps(true);

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
                    render();
                    if DwmFlush().is_err() {
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
        // The UI Automation thread wakes on the quit event; don't wait long for it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !uia_thread.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if let Some(k) = kb_hook {
            let _ = UnhookWindowsHookEx(k);
        }
        if let Some(m) = ms_hook {
            let _ = UnhookWindowsHookEx(m);
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
