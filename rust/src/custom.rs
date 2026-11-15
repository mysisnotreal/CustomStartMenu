// Making it yours: "> theme new" (your own theme), "> sounds", "> layout", "> hotkey".
// Same as cpp/custom.inl.

use super::panels::*;
use super::*;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

// ---- your own theme -----------------------------------------------------------------------
const ACCENTS: [(&str, u32); 12] = [
    ("Sky", 0x60CDFF),
    ("Blue", 0x4C8DFF),
    ("Violet", 0xA78BFA),
    ("Pink", 0xFF7AC6),
    ("Red", 0xFF6B6B),
    ("Orange", 0xFF9E64),
    ("Gold", 0xF5C451),
    ("Lime", 0xA3E635),
    ("Mint", 0x5EE6A8),
    ("Teal", 0x2DD4BF),
    ("Silver", 0xC0C4CC),
    ("Rose", 0xFDA4AF),
];
const BASES: [&str; 4] = ["Dark", "Light", "Black", "Navy"];
const MATERIALS: [&str; 3] = ["Solid", "Frosted", "Clear"];
const CUSTOM_THEME: usize = THEME_COUNT - 1;
static mut CT_BASE: i32 = 0;
static mut CT_ACCENT: i32 = 0;
static mut CT_MAT: i32 = 1; // being edited
static mut CT_SAVED: bool = false;

fn build_custom_theme(base: i32, accent: i32, mat: i32) -> Theme {
    let mut t = themes()[0];
    t.name = "Custom";
    let mat = mat as usize;
    const D_TOP: [f32; 3] = [0.95, 0.74, 0.40];
    const D_BOT: [f32; 3] = [0.97, 0.82, 0.52];
    const L_TOP: [f32; 3] = [0.93, 0.72, 0.38];
    const L_BOT: [f32; 3] = [0.95, 0.80, 0.48];
    let mut ac = c(ACCENTS[accent as usize].1, 1.0);
    if base == 1 {
        t.tint_top = c(0xFAFAFA, L_TOP[mat]);
        t.tint_bottom = c(0xF1F1F1, L_BOT[mat]);
        t.border_top = c(0xFFFFFF, 0.80);
        t.border_bottom = c(0x000000, 0.07);
        t.text = c(0x1A1A1A, 1.0);
        t.dim = c(0x6E6E6E, 1.0);
        t.ghost = c(0xA3A3A3, 1.0);
        t.highlight = c(0x000000, 0.055);
        t.divider = c(0x000000, 0.07);
        t.shadow = 0.30;
        ac = lerp_c(ac, c(0x000000, 1.0), 0.38); // darker, so it reads on white
    } else {
        let top = match base {
            2 => 0x050505,
            3 => 0x141C33,
            _ => 0x2F2D2B,
        };
        let bot = match base {
            2 => 0x000000,
            3 => 0x0E1528,
            _ => 0x292725,
        };
        t.tint_top = c(top, D_TOP[mat]);
        t.tint_bottom = c(bot, D_BOT[mat]);
        t.border_top = c(0xFFFFFF, 0.13);
        t.border_bottom = c(0xFFFFFF, 0.04);
        t.text = c(0xEDEBE8, 1.0);
        t.dim = if base == 3 { c(0x7D879E, 1.0) } else { c(0x8E8983, 1.0) };
        t.ghost = if base == 3 { c(0x5D667C, 1.0) } else { c(0x726D67, 1.0) };
        t.highlight = c(0xFFFFFF, 0.07);
        t.divider = c(0xFFFFFF, 0.07);
        t.shadow = 0.62;
        if mat == 2 {
            t.text = c(0xFFFFFF, 1.0);
            t.dim = c(0xD2D2D2, 1.0);
            t.ghost = c(0xA8A8A8, 1.0);
        }
    }
    t.accent = ac;
    t
}
pub fn load_custom_theme() {
    unsafe {
        CT_BASE = (read_dword("CustomBase", 0) as i32).clamp(0, 3);
        CT_ACCENT = (read_dword("CustomAccent", 0) as i32).clamp(0, ACCENTS.len() as i32 - 1);
        CT_MAT = (read_dword("CustomMaterial", 1) as i32).clamp(0, 2);
        themes()[CUSTOM_THEME] = build_custom_theme(CT_BASE, CT_ACCENT, CT_MAT);
    }
}
fn theme_edit_preview() {
    unsafe {
        themes()[CUSTOM_THEME] = build_custom_theme(CT_BASE, CT_ACCENT, CT_MAT);
    }
    let s = st();
    s.th_from = s.th;
    s.preview_idx = CUSTOM_THEME;
    s.th_start = now();
}
fn theme_edit_defs() -> Vec<SettingDef> {
    vec![
        SettingDef {
            name: "Base",
            glyph: "\u{E790}",
            options: BASES.iter().map(|s| s.to_string()).collect(),
            get: Box::new(|| unsafe { CT_BASE }),
            set: Box::new(|v| {
                unsafe { CT_BASE = v };
                theme_edit_preview();
            }),
        },
        SettingDef {
            name: "Accent",
            glyph: "\u{E771}",
            options: ACCENTS.iter().map(|a| a.0.to_string()).collect(),
            get: Box::new(|| unsafe { CT_ACCENT }),
            set: Box::new(|v| {
                unsafe { CT_ACCENT = v };
                theme_edit_preview();
            }),
        },
        SettingDef {
            name: "Material",
            glyph: "\u{F4A5}",
            options: MATERIALS.iter().map(|s| s.to_string()).collect(),
            get: Box::new(|| unsafe { CT_MAT }),
            set: Box::new(|v| {
                unsafe { CT_MAT = v };
                theme_edit_preview();
            }),
        },
    ]
}
fn accent_swatch() {
    if let Some(r) = rows().get_mut(1) {
        r.swatch = Some(c(ACCENTS[unsafe { CT_ACCENT } as usize].1, 1.0));
    }
}
pub fn theme_edit_fill(first: bool) {
    if first {
        unsafe { CT_SAVED = false };
        theme_edit_preview();
    }
    settings_fill(&theme_edit_defs());
    accent_swatch();
    rows().push(PRow {
        glyph: "\u{E73E}".into(),
        title: "Use this theme".into(),
        detail: if st().theme_idx == CUSTOM_THEME { "In use".into() } else { String::new() },
        action: "Save  ↵".into(),
        on: true,
        ..Default::default()
    });
}
pub fn theme_edit_enter(sel: i32, _: bool) {
    if sel < 3 {
        settings_enter(&theme_edit_defs(), sel);
        return;
    }
    unsafe {
        write_dword("CustomBase", CT_BASE as u32);
        write_dword("CustomAccent", CT_ACCENT as u32);
        write_dword("CustomMaterial", CT_MAT as u32);
        CT_SAVED = true;
    }
    apply_theme(CUSTOM_THEME);
    flash("Theme saved");
    fill(false);
}
pub fn theme_edit_key(vk: VIRTUAL_KEY, shift: bool) -> bool {
    settings_key(&theme_edit_defs(), vk, shift)
}
pub fn theme_edit_leave() {
    if unsafe { !CT_SAVED } {
        load_custom_theme(); // back to what was saved
        let s = st();
        set_preview(s.theme_idx);
        s.preview_idx = s.theme_idx;
    }
}

// ---- sounds: made up on the spot (no files) -------------------------------------------------
static mut SND_TYPE: i32 = 0;
static mut SND_OPEN: i32 = 0;
static mut SND_VOL: i32 = 2;
const TYPE_SOUNDS: [&str; 5] = ["Off", "Soft", "Typewriter", "Bubble", "Click"];
const OPEN_SOUNDS: [&str; 4] = ["Off", "Whoosh", "Chime", "Pop"];
const VOLUMES: [&str; 4] = ["25%", "50%", "75%", "100%"];
use std::f32::consts::PI;

fn make_wav(s: &[f32]) -> Vec<u8> {
    let rate = 44100u32;
    let mut w = Vec::with_capacity(44 + s.len() * 2);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + s.len() as u32 * 2).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(s.len() as u32 * 2).to_le_bytes());
    for &x in s {
        w.extend_from_slice(&((x * 32767.0).clamp(-32767.0, 32767.0) as i16).to_le_bytes());
    }
    w
}
fn noise(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    (*seed >> 9) as f32 / 4194304.0 - 1.0
}
// kind: 1-4 typing packs (variant 0-3), 11-13 open sounds, 20 timer chime
fn synth(kind: i32, variant: i32) -> Vec<f32> {
    const R: f32 = 44100.0;
    let mut seed: u32 = (1234 + variant * 977 + kind * 31) as u32;
    let pitch = 1.0 + (variant as f32 - 1.5) * 0.045;
    let len = |sec: f32| vec![0.0f32; (sec * R) as usize];
    let mut s;
    match kind {
        1 => {
            // soft: a muted tick
            s = len(0.035);
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f32 / R;
                *v = 0.35 * (2.0 * PI * 1650.0 * pitch * t).sin() * (-t * 160.0).exp() + 0.08 * noise(&mut seed) * (-t * 400.0).exp();
            }
        }
        2 => {
            // typewriter: a clack and a thump
            s = len(0.06);
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f32 / R;
                *v = 0.55 * noise(&mut seed) * (-t * 220.0).exp() + 0.35 * (2.0 * PI * 180.0 * pitch * t).sin() * (-t * 70.0).exp();
            }
        }
        3 => {
            // bubble: a quick upward blip
            s = len(0.07);
            let mut ph = 0.0f32;
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f32 / R;
                let f = (520.0 + 1400.0 * t / 0.07) * pitch;
                ph += 2.0 * PI * f / R;
                *v = 0.4 * ph.sin() * (PI * t / 0.07).sin();
            }
        }
        4 => {
            // click: tiny and sharp
            s = len(0.012);
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f32 / R;
                *v = 0.5 * if (2.0 * PI * 3800.0 * pitch * t).sin() > 0.0 { 1.0 } else { -1.0 } * (-t * 600.0).exp();
            }
        }
        11 => {
            // whoosh: filtered noise swelling and fading
            s = len(0.32);
            let mut lp = 0.0f32;
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f32 / R;
                let env = (PI * (t / 0.32).powf(0.7)).sin();
                let k = 0.02 + 0.25 * t / 0.32;
                lp += (noise(&mut seed) - lp) * k;
                *v = 0.9 * lp * env;
            }
        }
        12 | 20 => {
            // chime: two (or three) bell notes
            let dur = if kind == 20 { 1.6 } else { 0.6 };
            s = len(dur);
            let notes = [880.0f32, 1318.5, 1760.0];
            let n = if kind == 20 { 3 } else { 2 };
            for (k, note) in notes.iter().enumerate().take(n) {
                let start = (k as f32 * if kind == 20 { 0.28 } else { 0.09 } * R) as usize;
                for i in start..s.len() {
                    let t = (i - start) as f32 / R;
                    let env = (-t * if kind == 20 { 3.2 } else { 6.0 }).exp() * (t * 400.0).min(1.0);
                    s[i] += 0.22 * env * ((2.0 * PI * note * t).sin() + 0.3 * (2.0 * PI * note * 2.01 * t).sin());
                }
            }
        }
        13 => {
            // pop
            s = len(0.08);
            let mut ph = 0.0f32;
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f32 / R;
                let f = 900.0 * (-t * 30.0).exp() + 140.0;
                ph += 2.0 * PI * f / R;
                *v = 0.6 * ph.sin() * (-t * 45.0).exp();
            }
        }
        _ => s = Vec::new(),
    }
    s
}
static mut SND_CACHE: Option<HashMap<i32, Vec<u8>>> = None;
fn sound_data(kind: i32, variant: i32) -> &'static Vec<u8> {
    let cache = unsafe { (*std::ptr::addr_of_mut!(SND_CACHE)).get_or_insert_with(HashMap::new) };
    cache.entry(kind * 4 + variant).or_insert_with(|| {
        let mut s = synth(kind, variant);
        let mut vol = (unsafe { SND_VOL } + 1) as f32 / 4.0;
        if kind == 20 {
            vol = vol.max(0.75);
        }
        for x in s.iter_mut() {
            *x *= vol;
        }
        make_wav(&s)
    })
}
fn sound_clear_cache() {
    unsafe {
        let _ = PlaySoundW(PCWSTR::null(), HMODULE::default(), windows::Win32::Media::Audio::SND_FLAGS(0));
        if let Some(c) = (*std::ptr::addr_of_mut!(SND_CACHE)).as_mut() {
            c.clear();
        }
    }
}
fn play_synth(kind: i32, variant: i32) {
    let d = sound_data(kind, variant);
    unsafe {
        let _ = PlaySoundW(PCWSTR(d.as_ptr() as *const u16), HMODULE::default(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}
static mut TYPE_VARIANT: i32 = 0;
pub fn sound_typed() {
    unsafe {
        if SND_TYPE > 0 {
            play_synth(SND_TYPE, TYPE_VARIANT % 4);
            TYPE_VARIANT += 1;
        }
    }
}
pub fn sound_opened() {
    unsafe {
        if SND_OPEN > 0 {
            play_synth(10 + SND_OPEN, 0);
        }
    }
}
pub fn play_chime() {
    play_synth(20, 0);
}
pub fn load_sound_settings() {
    unsafe {
        SND_TYPE = (read_dword("SoundTyping", 0) as i32).clamp(0, 4);
        SND_OPEN = (read_dword("SoundOpen", 0) as i32).clamp(0, 3);
        SND_VOL = (read_dword("SoundVolume", 2) as i32).clamp(0, 3);
    }
}
pub fn sounds_on() -> bool {
    unsafe { SND_TYPE != 0 || SND_OPEN != 0 }
}
pub fn sound_defs() -> Vec<SettingDef> {
    vec![
        SettingDef {
            name: "Typing",
            glyph: "\u{E765}",
            options: TYPE_SOUNDS.iter().map(|s| s.to_string()).collect(),
            get: Box::new(|| unsafe { SND_TYPE }),
            set: Box::new(|v| {
                unsafe { SND_TYPE = v };
                write_dword("SoundTyping", v as u32);
                if v != 0 {
                    play_synth(v, 1);
                }
            }),
        },
        SettingDef {
            name: "Opening the bar",
            glyph: "\u{E8A7}",
            options: OPEN_SOUNDS.iter().map(|s| s.to_string()).collect(),
            get: Box::new(|| unsafe { SND_OPEN }),
            set: Box::new(|v| {
                unsafe { SND_OPEN = v };
                write_dword("SoundOpen", v as u32);
                if v != 0 {
                    play_synth(10 + v, 0);
                }
            }),
        },
        SettingDef {
            name: "Volume",
            glyph: "\u{E767}",
            options: VOLUMES.iter().map(|s| s.to_string()).collect(),
            get: Box::new(|| unsafe { SND_VOL }),
            set: Box::new(|v| {
                unsafe { SND_VOL = v };
                write_dword("SoundVolume", v as u32);
                sound_clear_cache();
                let t = unsafe { SND_TYPE };
                play_synth(if t != 0 { t } else { 12 }, 1);
            }),
        },
    ]
}

// ---- layout ---------------------------------------------------------------------------------
static mut LAY_POS: i32 = 0;
static mut LAY_WIDTH: i32 = 1;
static mut LAY_CORNERS: i32 = 0;
pub const LAY_POS_NAMES: [&str; 3] = ["Middle", "Higher", "Top"];
pub const LAY_WIDTH_NAMES: [&str; 3] = ["Compact", "Normal", "Wide"];
const LAY_CORNER_NAMES: [&str; 3] = ["Round", "Rounder", "Square"];
const LAY_ANIM_NAMES: [&str; 4] = ["Rise", "Pop", "Fade", "Drop"];
const WIDTHS: [f32; 3] = [540.0, 620.0, 760.0];
// the size changes the next time the bar opens
pub fn apply_layout() {
    unsafe {
        LAY_BAR_W = WIDTHS[LAY_WIDTH as usize];
        LAY_VERTICAL_POS = match LAY_POS {
            0 => 0.40,
            1 => 0.27,
            _ => 0.0,
        };
    }
}
fn apply_corners() {
    unsafe {
        LAY_RADIUS = match LAY_CORNERS {
            0 => 14.0,
            1 => 24.0,
            _ => 5.0,
        };
    }
}
pub fn load_layout_settings() {
    unsafe {
        LAY_POS = (read_dword("LayoutPosition", 0) as i32).clamp(0, 2);
        LAY_WIDTH = (read_dword("LayoutWidth", 1) as i32).clamp(0, 2);
        LAY_CORNERS = (read_dword("LayoutCorners", 0) as i32).clamp(0, 2);
        widgets::STRIP_ON = read_dword("StatusStrip", 1) != 0;
    }
    ps().lay_anim = (read_dword("LayoutAnimation", 0) as i32).clamp(0, 3);
    apply_layout();
    apply_corners();
}
pub fn layout_detail() -> String {
    unsafe { format!("{} · {}", LAY_WIDTH_NAMES[LAY_WIDTH as usize], LAY_POS_NAMES[LAY_POS as usize]) }
}
const ON_OFF: [&str; 2] = ["Off", "On"];
fn opts(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}
pub fn layout_defs() -> Vec<SettingDef> {
    vec![
        SettingDef {
            name: "Position on screen",
            glyph: "\u{E8A0}",
            options: opts(&LAY_POS_NAMES),
            get: Box::new(|| unsafe { LAY_POS }),
            set: Box::new(|v| {
                unsafe { LAY_POS = v };
                write_dword("LayoutPosition", v as u32);
                flash("Moves the next time it opens");
            }),
        },
        SettingDef {
            name: "Width",
            glyph: "\u{E740}",
            options: opts(&LAY_WIDTH_NAMES),
            get: Box::new(|| unsafe { LAY_WIDTH }),
            set: Box::new(|v| {
                unsafe { LAY_WIDTH = v };
                write_dword("LayoutWidth", v as u32);
                flash("Resizes the next time it opens");
            }),
        },
        SettingDef {
            name: "Corners",
            glyph: "\u{F16B}",
            options: opts(&LAY_CORNER_NAMES),
            get: Box::new(|| unsafe { LAY_CORNERS }),
            set: Box::new(|v| {
                unsafe { LAY_CORNERS = v };
                write_dword("LayoutCorners", v as u32);
                apply_corners();
            }),
        },
        SettingDef {
            name: "Opening animation",
            glyph: "\u{E916}",
            options: opts(&LAY_ANIM_NAMES),
            get: Box::new(|| ps().lay_anim),
            set: Box::new(|v| {
                ps().lay_anim = v;
                write_dword("LayoutAnimation", v as u32);
            }),
        },
        SettingDef {
            name: "Clock and weather on the bar",
            glyph: "\u{E121}",
            options: opts(&ON_OFF),
            get: Box::new(|| unsafe { widgets::STRIP_ON as i32 }),
            set: Box::new(|v| {
                unsafe { widgets::STRIP_ON = v != 0 };
                write_dword("StatusStrip", v as u32);
            }),
        },
    ]
}

// ---- hotkey ---------------------------------------------------------------------------------
static mut HOTKEY: i32 = 0;
const HOTKEYS: [&str; 5] = ["None", "Alt + Space", "Ctrl + Space", "Ctrl + Alt + Space", "Ctrl + Shift + Space"];
pub fn apply_hotkey() -> bool {
    unsafe {
        let _ = UnregisterHotKey(hwnd(), 1);
        let mods = [HOT_KEY_MODIFIERS(0), MOD_ALT, MOD_CONTROL, MOD_CONTROL | MOD_ALT, MOD_CONTROL | MOD_SHIFT];
        if HOTKEY == 0 {
            return true;
        }
        RegisterHotKey(hwnd(), 1, mods[HOTKEY as usize] | MOD_NOREPEAT, VK_SPACE.0 as u32).is_ok()
    }
}
pub fn load_hotkey_settings() {
    unsafe { HOTKEY = (read_dword("Hotkey", 0) as i32).clamp(0, 4) };
    INTERCEPT.store(read_dword("Intercept", 1) != 0, Relaxed);
}
pub fn hotkey_detail() -> String {
    let h = unsafe { HOTKEY };
    if h != 0 {
        HOTKEYS[h as usize].into()
    } else if INTERCEPT.load(Relaxed) {
        "Windows key".into()
    } else {
        "Tray only".into()
    }
}
pub fn hotkey_defs() -> Vec<SettingDef> {
    vec![
        SettingDef {
            name: "Windows key opens Custom Start",
            glyph: "\u{E782}",
            options: opts(&ON_OFF),
            get: Box::new(|| INTERCEPT.load(Relaxed) as i32),
            set: Box::new(|v| {
                INTERCEPT.store(v != 0, Relaxed);
                write_dword("Intercept", v as u32);
            }),
        },
        SettingDef {
            name: "Also open with",
            glyph: "\u{E765}",
            options: opts(&HOTKEYS),
            get: Box::new(|| unsafe { HOTKEY }),
            set: Box::new(|v| {
                unsafe { HOTKEY = v };
                write_dword("Hotkey", v as u32);
                if !apply_hotkey() {
                    flash("Another app already uses that shortcut");
                }
            }),
        },
    ]
}

pub fn tray_balloon(title: &str, text: &str) {
    unsafe {
        let mut nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd(),
            uID: 1,
            uFlags: NIF_INFO,
            dwInfoFlags: NIIF_INFO,
            ..Default::default()
        };
        let t: Vec<u16> = title.encode_utf16().take(nid.szInfoTitle.len() - 1).collect();
        nid.szInfoTitle[..t.len()].copy_from_slice(&t);
        let b: Vec<u16> = text.encode_utf16().take(nid.szInfo.len() - 1).collect();
        nid.szInfo[..b.len()].copy_from_slice(&b);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}
