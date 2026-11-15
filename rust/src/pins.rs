// Pinned apps: HKCU\Software\CustomStartMenu\Pins (REG_MULTI_SZ, one "hidden\ttarget\tname\ticon"
// per pin, in order). On first run they are copied from the Windows Start menu.

use super::games::{expand_env, file_exists, file_stem, make_app, read_text_file};
use super::*;
use std::os::windows::process::CommandExt;

#[derive(Clone, Default)]
pub struct Pin {
    pub app: App,
    pub hidden: bool,
    pub x: f32, // animated
    pub y: f32,
    pub hover: f32,
    pub press: f32,
    pub placed: bool,
}

pub fn save_pins() {
    let mut blob: Vec<u16> = Vec::new();
    for p in &st().pins {
        blob.extend(ws(if p.hidden { "1\t" } else { "0\t" }));
        blob.extend_from_slice(&p.app.target);
        blob.push(9);
        blob.extend_from_slice(&p.app.name);
        blob.push(9);
        blob.extend_from_slice(&p.app.icon);
        blob.push(0);
    }
    blob.push(0);
    unsafe {
        let mut k = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, SETTINGS_KEY, 0, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None)
            == ERROR_SUCCESS
        {
            let bytes = std::slice::from_raw_parts(blob.as_ptr() as *const u8, blob.len() * 2);
            let _ = RegSetValueExW(k, w!("Pins"), 0, REG_MULTI_SZ, Some(bytes));
            let _ = RegCloseKey(k);
        }
    }
}

// false if the user has no saved pins yet
pub fn load_pins() -> bool {
    unsafe {
        let mut sz = 0u32;
        if RegGetValueW(HKEY_CURRENT_USER, SETTINGS_KEY, w!("Pins"), RRF_RT_REG_MULTI_SZ, None, None, Some(&mut sz)) != ERROR_SUCCESS {
            return false;
        }
        let mut buf = vec![0u16; sz as usize / 2 + 2];
        if RegGetValueW(HKEY_CURRENT_USER, SETTINGS_KEY, w!("Pins"), RRF_RT_REG_MULTI_SZ, None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut sz))
            != ERROR_SUCCESS
        {
            return false;
        }
        let s = st();
        s.pins.clear();
        for entry in buf.split(|&c| c == 0) {
            if entry.is_empty() {
                break;
            }
            let f: Vec<&[u16]> = entry.split(|&c| c == 9).collect();
            if f.len() < 3 || f[1].is_empty() {
                continue;
            }
            let n = f[2].to_vec();
            let lw = lower(&n);
            let app = App {
                initials: initials(&lw),
                lower: lw,
                name: n,
                target: f[1].to_vec(),
                alias: Vec::new(),
                icon: if f.len() > 3 { f[3].to_vec() } else { Vec::new() },
                detail: Vec::new(),
            };
            s.pins.push(Pin { app, hidden: f[0] == ws("1").as_slice(), ..Default::default() });
        }
    }
    true
}

pub fn find_pin(target: &[u16]) -> i32 {
    let t = lower(target);
    st().pins.iter().position(|p| lower(&p.app.target) == t).map(|i| i as i32).unwrap_or(-1)
}

pub fn visible_pins() -> Vec<usize> {
    st().pins.iter().enumerate().filter(|(_, p)| !p.hidden).map(|(i, _)| i).collect()
}
pub fn pin_rows_total() -> i32 {
    (visible_pins().len() as i32 + PIN_COLS - 1) / PIN_COLS
}
pub fn pin_rows_shown() -> i32 {
    pin_rows_total().min(PIN_MAX_ROWS)
}
pub fn pins_area_h() -> f32 {
    let rows = pin_rows_shown();
    if rows > 0 {
        rows as f32 * TILE_H + (rows - 1) as f32 * TILE_GAP + PINS_GAP
    } else {
        0.0
    }
}

// Where the k-th visible pin sits (bar space). Rows are centered over the bar.
pub fn pin_slot(k: i32, n_vis: i32) -> D2D_POINT_2F {
    let rows_shown = pin_rows_shown();
    let (row, col) = (k / PIN_COLS, k % PIN_COLS);
    let in_row = PIN_COLS.min(n_vis - row * PIN_COLS);
    let row_w = in_row as f32 * tile_w() + (in_row - 1) as f32 * TILE_GAP;
    let top = MARGIN - PINS_GAP - rows_shown as f32 * TILE_H - (rows_shown - 1) as f32 * TILE_GAP;
    pt(
        MARGIN + (bar_w() - row_w) / 2.0 + col as f32 * (tile_w() + TILE_GAP),
        top + (row - st().pin_row_off) as f32 * (TILE_H + TILE_GAP),
    )
}

// Runs "Export-StartLayout" (the Windows Start menu's own pin list, as JSON) and maps every
// entry to an app. The result goes to the UI thread through IMPORTED_PINS + WM_APP_PINS.
pub static IMPORTED_PINS: Mutex<Option<Vec<App>>> = Mutex::new(None);

pub fn start_import() {
    std::thread::spawn(|| unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let out = std::env::temp_dir().join("csm_start_layout.json");
        let outs = out.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&out);
        let cmd = format!("Export-StartLayout -Path '{}'", outs);
        if let Ok(mut child) = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &cmd])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn()
        {
            let t0 = std::time::Instant::now();
            while child.try_wait().ok().flatten().is_none() && t0.elapsed().as_secs() < 20 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        let json = read_text_file(&outs);
        let _ = std::fs::remove_file(&out);

        // the app list is needed to turn shortcuts into apps; it loads in parallel at startup
        for _ in 0..100 {
            if !APPS.read().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let mut pins = Vec::new();
        let mut pos = 0;
        loop {
            let a = json[pos..].find("\"packagedAppId\"").map(|i| i + pos);
            let b = json[pos..].find("\"desktopAppLink\"").map(|i| i + pos);
            let (pkg, at) = match (a, b) {
                (None, None) => break,
                (Some(a), None) => (true, a),
                (None, Some(b)) => (false, b),
                (Some(a), Some(b)) => (a < b, a.min(b)),
            };
            let Some(colon) = json[at + 1..].find(':').map(|i| i + at + 1) else { break };
            let Some(q1) = json[colon + 1..].find('"').map(|i| i + colon + 1) else { break };
            let mut val = String::new();
            let mut it = json[q1 + 1..].char_indices();
            let mut end = json.len();
            while let Some((i, ch)) = it.next() {
                match ch {
                    '"' => {
                        end = q1 + 1 + i;
                        break;
                    }
                    '\\' => {
                        if let Some((_, n)) = it.next() {
                            val.push(n);
                        }
                    }
                    _ => val.push(ch),
                }
            }
            pos = (end + 1).min(json.len());
            if val.is_empty() {
                continue;
            }
            let lnk = if pkg { String::new() } else { expand_env(&val) };
            let name_lower = if pkg { Vec::new() } else { lower(&ws(&file_stem(&lnk))) };
            let valw = lower(&ws(&val));
            let found = APPS
                .read()
                .unwrap()
                .iter()
                .find(|app| if pkg { lower(&app.target) == valw } else { app.lower == name_lower })
                .cloned();
            let found = match found {
                Some(f) => Some(f),
                None if !pkg && file_exists(&lnk) => Some(make_app(&file_stem(&lnk), &lnk, "", "")),
                None => None,
            };
            if let Some(mut f) = found {
                f.alias.clear();
                f.detail.clear();
                pins.push(f);
            }
        }
        CoUninitialize();
        *IMPORTED_PINS.lock().unwrap() = Some(pins);
        let h = hwnd();
        if !h.is_invalid() {
            let _ = PostMessageW(h, WM_APP_PINS, WPARAM(0), LPARAM(0));
        }
    });
}
