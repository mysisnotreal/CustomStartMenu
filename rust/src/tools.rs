// Tools: switch windows, clipboard history, find files, close apps, notes, power, dark mode,
// Wi-Fi / Bluetooth, and the user's own commands (the Commands folder). Same as cpp/tools.inl.

use super::panels::*;
use super::*;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};

fn process_path(pid: u32) -> String {
    unsafe {
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) else { return String::new() };
        let mut buf = [0u16; 260];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n).is_ok();
        let _ = CloseHandle(p);
        if ok {
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            String::new()
        }
    }
}
fn matches(hay: &str, needle_lower: &str) -> bool {
    needle_lower.is_empty() || hay.to_lowercase().contains(needle_lower)
}
fn arg_lower() -> String {
    ps().arg.to_lowercase()
}

// ---- > windows ------------------------------------------------------------------------------
struct WinItem {
    h: HWND,
    title: String,
    exe: String,
    path: String,
}
static mut WIN_ITEMS: Vec<WinItem> = Vec::new();
fn win_items() -> &'static mut Vec<WinItem> {
    unsafe { &mut *std::ptr::addr_of_mut!(WIN_ITEMS) }
}
unsafe extern "system" fn enum_win(h: HWND, _: LPARAM) -> BOOL {
    if !IsWindowVisible(h).as_bool() || h == hwnd() || GetWindow(h, GW_OWNER).map(|o| !o.is_invalid()).unwrap_or(false) {
        return TRUE;
    }
    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
        return TRUE;
    }
    let mut cloaked = 0u32;
    let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4);
    if cloaked != 0 {
        return TRUE;
    }
    let mut t = [0u16; 512];
    let n = GetWindowTextW(h, &mut t);
    if n <= 0 {
        return TRUE;
    }
    let mut cls = [0u16; 64];
    let cn = GetClassNameW(h, &mut cls);
    let cls = String::from_utf16_lossy(&cls[..cn.max(0) as usize]);
    if cls == "Progman" || cls == "Shell_TrayWnd" {
        return TRUE;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    let path = process_path(pid);
    win_items().push(WinItem { h, title: String::from_utf16_lossy(&t[..n as usize]), exe: games::file_stem(&path), path });
    TRUE
}
pub fn windows_fill(first: bool) {
    let sel = st().sel;
    let keep = if !first && sel >= 0 && (sel as usize) < rows().len() { rows()[sel as usize].tag } else { 0 };
    win_items().clear();
    unsafe {
        let _ = EnumWindows(Some(enum_win), LPARAM(0));
    }
    let a = arg_lower();
    let r = rows();
    r.clear();
    for w in win_items().iter() {
        if !matches(&w.title, &a) && !matches(&w.exe, &a) {
            continue;
        }
        r.push(PRow {
            icon: w.path.clone(),
            glyph: "\u{E737}".into(),
            title: w.title.clone(),
            detail: w.exe.clone(),
            action: if unsafe { IsIconic(w.h).as_bool() } { "Restore  ↵".into() } else { "Switch  ↵".into() },
            tag: w.h.0 as isize as i32,
            ..Default::default()
        });
        if keep != 0 && w.h.0 as isize as i32 == keep {
            st().sel = r.len() as i32 - 1;
        }
    }
}
fn row_hwnd(sel: i32) -> Option<HWND> {
    let r = rows().get(sel.max(0) as usize)?;
    if sel < 0 {
        return None;
    }
    Some(HWND(r.tag as isize as *mut c_void))
}
pub fn windows_enter(sel: i32, _: bool) {
    let Some(h) = row_hwnd(sel) else { return };
    unsafe {
        if !IsWindow(h).as_bool() {
            return;
        }
        st().prev_fg = HWND::default();
        hide();
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        SwitchToThisWindow(h, TRUE);
        let _ = SetForegroundWindow(h);
    }
}
pub fn windows_del(sel: i32) -> bool {
    let Some(h) = row_hwnd(sel) else { return true };
    unsafe {
        let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    flash(&format!("Closing {}", rows()[sel as usize].detail));
    ps().fill_at = now() - 10.0; // look again soon
    true
}

// ---- > clip: clipboard history (kept in memory only, never saved) ---------------------------
struct ClipItem {
    text: String,
    at: f64,
}
static mut CLIPS: VecDeque<ClipItem> = VecDeque::new();
static mut CF_EXCLUDE: u32 = 0;
static mut CF_HISTORY: u32 = 0;
fn clips() -> &'static mut VecDeque<ClipItem> {
    unsafe { &mut *std::ptr::addr_of_mut!(CLIPS) }
}
pub fn clip_count() -> usize {
    clips().len()
}
pub fn clip_init() {
    unsafe {
        CF_EXCLUDE = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        CF_HISTORY = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        let _ = AddClipboardFormatListener(hwnd());
    }
}
// WM_CLIPBOARDUPDATE
pub fn clip_changed() {
    unsafe {
        if IsClipboardFormatAvailable(CF_EXCLUDE).is_ok() {
            return; // password managers ask not to be kept
        }
        if OpenClipboard(hwnd()).is_err() {
            return;
        }
        let mut skip = false;
        if let Ok(hh) = GetClipboardData(CF_HISTORY) {
            let g = HGLOBAL(hh.0);
            let d = GlobalLock(g) as *const u32;
            if !d.is_null() {
                skip = *d == 0;
                let _ = GlobalUnlock(g);
            }
        }
        let mut s = String::new();
        if !skip {
            if let Ok(h) = GetClipboardData(CF_UNICODETEXT.0 as u32) {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u16;
                if !p.is_null() {
                    let mut n = 0;
                    while *p.add(n) != 0 {
                        n += 1;
                    }
                    s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                    let _ = GlobalUnlock(g);
                }
            }
        }
        let _ = CloseClipboard();
        if skip || s.trim().is_empty() || s.len() > 100000 {
            return;
        }
        let c = clips();
        if let Some(i) = c.iter().position(|x| x.text == s) {
            c.remove(i);
        }
        c.push_front(ClipItem { text: s, at: calc::unix_now() });
        c.truncate(25);
        if st().mode == Mode::Panel && cur().map(|p| p.key == "clip").unwrap_or(false) {
            fill(false);
        }
    }
}
pub fn clip_fill(_: bool) {
    let a = arg_lower();
    let r = rows();
    r.clear();
    let t = calc::unix_now();
    for (i, c) in clips().iter().enumerate() {
        if !matches(&c.text, &a) {
            continue;
        }
        r.push(PRow {
            glyph: "\u{E8C8}".into(),
            title: one_line(&c.text, 120),
            detail: ago(t - c.at),
            action: "Copy  ↵".into(),
            tag: i as i32,
            ..Default::default()
        });
    }
}
pub fn clip_enter(sel: i32, _: bool) {
    let Some(r) = rows().get(sel.max(0) as usize) else { return };
    if sel < 0 {
        return;
    }
    if let Some(c) = clips().get(r.tag as usize) {
        let t = c.text.clone();
        if set_clipboard_text(&t) {
            flash("Copied");
        }
    }
}
pub fn clip_del(sel: i32) -> bool {
    if sel >= 0 && (sel as usize) < rows().len() {
        let i = rows()[sel as usize].tag as usize;
        if i < clips().len() {
            clips().remove(i);
        }
        fill(false);
    }
    true
}

// ---- > f / > files: names of the files in your folders, indexed in the background ----------
struct FileEntry {
    lower: String,
    path: String,
}
static FILES: RwLock<Vec<FileEntry>> = RwLock::new(Vec::new());
static FILES_LOADING: AtomicBool = AtomicBool::new(false);
static mut FILES_AT: f64 = -1e9;

fn index_dir(dir: &str, out: &mut Vec<FileEntry>, depth: i32) {
    if depth > 12 || out.len() > 300000 {
        return;
    }
    unsafe {
        let mut fd = WIN32_FIND_DATAW::default();
        let pat = HSTRING::from(format!("{}\\*", dir));
        let Ok(f) = FindFirstFileExW(
            &pat,
            FindExInfoBasic,
            &mut fd as *mut _ as *mut c_void,
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        ) else {
            return;
        };
        loop {
            let nlen = fd.cFileName.iter().position(|&c| c == 0).unwrap_or(260);
            let n = String::from_utf16_lossy(&fd.cFileName[..nlen]);
            let attr = fd.dwFileAttributes;
            if !n.starts_with('.') && attr & (FILE_ATTRIBUTE_HIDDEN.0 | FILE_ATTRIBUTE_SYSTEM.0) == 0 {
                let path = format!("{}\\{}", dir, n);
                let ln = n.to_lowercase();
                if attr & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
                    if attr & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0
                        && !matches!(ln.as_str(), "node_modules" | "__pycache__" | "target" | "venv" | ".venv")
                    {
                        out.push(FileEntry { lower: ln, path: path.clone() });
                        index_dir(&path, out, depth + 1);
                    }
                } else {
                    out.push(FileEntry { lower: ln, path });
                }
            }
            if FindNextFileW(f, &mut fd).is_err() || out.len() > 300000 {
                break;
            }
        }
        let _ = FindClose(f);
    }
}
fn files_refresh() {
    if now() - unsafe { FILES_AT } < 600.0 || FILES_LOADING.swap(true, SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let mut out = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for id in [&FOLDERID_Desktop, &FOLDERID_Documents, &FOLDERID_Downloads, &FOLDERID_Pictures, &FOLDERID_Music, &FOLDERID_Videos, &FOLDERID_SkyDrive] {
            let d = games::known_folder_path(id);
            let dl = d.to_lowercase();
            if d.is_empty() || seen.contains(&dl) {
                continue;
            }
            seen.push(dl.clone());
            // skip a folder already inside another one (Documents inside OneDrive)
            let inside = seen.iter().any(|s| *s != dl && dl.starts_with(&format!("{}\\", s)));
            if !inside {
                index_dir(&d, &mut out, 0);
            }
        }
        *FILES.write().unwrap() = out;
        unsafe { FILES_AT = now() };
        FILES_LOADING.store(false, SeqCst);
        unsafe {
            let _ = PostMessageW(hwnd(), WM_APP_PANEL, WPARAM(0), LPARAM(0));
        }
    });
}
fn short_dir(path: &str) -> String {
    let mut dir = match path.rfind('\\') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    };
    let home = games::expand_env("%USERPROFILE%");
    if !home.is_empty() && dir.to_lowercase().starts_with(&home.to_lowercase()) {
        dir = format!("~{}", &dir[home.len()..]);
    }
    dir
}
pub fn files_fill(_: bool) {
    files_refresh();
    let r = rows();
    r.clear();
    let a = arg_lower();
    if a.is_empty() {
        return;
    }
    let files = FILES.read().unwrap();
    let mut scored: Vec<(i32, usize)> = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let Some(p) = f.lower.find(&a) else { continue };
        let mut sc = if p == 0 { 3000 } else { 1000 };
        sc -= f.lower.encode_utf16().count() as i32;
        sc -= f.path.matches('\\').count() as i32 * 4;
        scored.push((sc, i));
    }
    scored.sort_by(|x, y| y.0.cmp(&x.0));
    for &(_, i) in scored.iter().take(60) {
        let f = &files[i];
        r.push(PRow {
            icon: f.path.clone(),
            glyph: "\u{E7C3}".into(),
            title: f.path[f.path.rfind('\\').map(|x| x + 1).unwrap_or(0)..].to_string(),
            detail: short_dir(&f.path),
            action: "Open  ↵   ⇧↵ folder".into(),
            tag: i as i32,
            ..Default::default()
        });
    }
}
pub fn files_enter(sel: i32, shift: bool) {
    if sel < 0 || sel as usize >= rows().len() {
        return;
    }
    let r = rows()[sel as usize].clone();
    if shift {
        show_in_explorer(r.icon);
        return;
    }
    let a = App { name: ws(&r.title), target: ws(&r.icon), ..Default::default() };
    launch(Some(&a), &[], false);
}

// ---- > kill: close every copy of an app ------------------------------------------------------
#[derive(Clone, Default)]
struct ProcGroup {
    name: String,
    path: String,
    pids: Vec<u32>,
    mem: f64,
}
static mut PROCS: Vec<ProcGroup> = Vec::new();
fn procs() -> &'static mut Vec<ProcGroup> {
    unsafe { &mut *std::ptr::addr_of_mut!(PROCS) }
}
fn protected(lower: &str) -> bool {
    const KEEP: [&str; 24] = [
        "system", "idle", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe", "svchost.exe",
        "dwm.exe", "fontdrvhost.exe", "memory compression", "secure system", "lsaiso.exe", "sihost.exe", "ctfmon.exe", "conhost.exe",
        "audiodg.exe", "spoolsv.exe", "msmpeng.exe", "searchindexer.exe", "wudfhost.exe", "system idle process",
    ];
    KEEP.contains(&lower)
}
pub fn kill_fill(first: bool) {
    let sel = st().sel;
    let keep_name = if !first && sel >= 0 && (sel as usize) < rows().len() { rows()[sel as usize].title.clone() } else { String::new() };
    let mut m: HashMap<String, ProcGroup> = HashMap::new();
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let me = GetCurrentProcessId();
            let mut ok = Process32FirstW(snap, &mut pe).is_ok();
            while ok {
                let pid = pe.th32ProcessID;
                let nlen = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
                let n = String::from_utf16_lossy(&pe.szExeFile[..nlen]);
                let ln = n.to_lowercase();
                if pid != 0 && pid != 4 && pid != me && !protected(&ln) {
                    let g = m.entry(ln.clone()).or_default();
                    g.name = if ln.ends_with(".exe") && n.len() > 4 { n[..n.len() - 4].to_string() } else { n.clone() };
                    g.pids.push(pid);
                    if let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) {
                        let mut pmc = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
                        if K32GetProcessMemoryInfo(p, &mut pmc, pmc.cb).as_bool() {
                            g.mem += pmc.WorkingSetSize as f64;
                        }
                        if g.path.is_empty() {
                            let mut buf = [0u16; 260];
                            let mut len = buf.len() as u32;
                            if QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                                g.path = String::from_utf16_lossy(&buf[..len as usize]);
                            }
                        }
                        let _ = CloseHandle(p);
                    }
                }
                ok = Process32NextW(snap, &mut pe).is_ok();
            }
            let _ = CloseHandle(snap);
        }
    }
    let pr = procs();
    *pr = m.into_values().collect();
    pr.sort_by(|a, b| b.mem.partial_cmp(&a.mem).unwrap_or(std::cmp::Ordering::Equal));
    let a = arg_lower();
    let r = rows();
    r.clear();
    for (i, g) in pr.iter().enumerate() {
        if !matches(&g.name, &a) {
            continue;
        }
        let detail = format!(
            "{}{}",
            if g.pids.len() > 1 { format!("{} running · ", g.pids.len()) } else { String::new() },
            format_bytes(g.mem)
        );
        let armed = armed(&format!("kill:{}", g.name.to_lowercase()));
        r.push(PRow {
            icon: g.path.clone(),
            glyph: "\u{E7EF}".into(),
            title: g.name.clone(),
            detail,
            action: if armed { "Press ↵ again to close".into() } else { "Close  ↵".into() },
            tag: i as i32,
            ..Default::default()
        });
        if !keep_name.is_empty() && g.name == keep_name {
            st().sel = r.len() as i32 - 1;
        }
    }
}
pub fn kill_enter(sel: i32, _: bool) {
    if sel < 0 || sel as usize >= rows().len() {
        return;
    }
    let i = rows()[sel as usize].tag as usize;
    let Some(g) = procs().get(i).cloned() else { return };
    if !confirmed(&format!("kill:{}", g.name.to_lowercase())) {
        fill(false);
        return;
    }
    let mut closed = 0;
    for pid in &g.pids {
        unsafe {
            if let Ok(p) = OpenProcess(PROCESS_TERMINATE, FALSE, *pid) {
                if TerminateProcess(p, 1).is_ok() {
                    closed += 1;
                }
                let _ = CloseHandle(p);
            }
        }
    }
    flash(&if closed > 0 { format!("Closed {}", g.name) } else { format!("Couldn't close {}", g.name) });
    std::thread::sleep(std::time::Duration::from_millis(120));
    fill(false);
}

// ---- > note / > notes --------------------------------------------------------------------------
struct Note {
    at: f64,
    text: String,
}
fn notes_file() -> String {
    format!("{}\\Notes.txt", app_dir())
}
fn load_notes() -> Vec<Note> {
    let mut out = Vec::new();
    let t = games::read_text_file(&notes_file());
    for line in t.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let Some(tab) = line.find('\t') else { continue };
        // stored with \n written as \\n
        let mut dec = String::new();
        let mut it = line[tab + 1..].chars().peekable();
        while let Some(c) = it.next() {
            if c == '\\' {
                if let Some(n) = it.next() {
                    dec.push(match n {
                        'n' => '\n',
                        't' => '\t',
                        o => o,
                    });
                    continue;
                }
            }
            dec.push(c);
        }
        out.push(Note { at: line[..tab].trim().parse().unwrap_or(0.0), text: dec });
    }
    out
}
fn save_notes(notes: &[Note]) {
    let mut t = String::from("\u{FEFF}");
    for n in notes {
        let mut enc = String::new();
        for c in n.text.chars() {
            match c {
                '\\' => enc.push_str("\\\\"),
                '\n' => enc.push_str("\\n"),
                '\t' => enc.push_str("\\t"),
                '\r' => {}
                o => enc.push(o),
            }
        }
        t.push_str(&format!("{:.0}\t{}\r\n", n.at, enc));
    }
    let _ = std::fs::write(notes_file(), t.as_bytes());
}
static mut NOTES: Vec<Note> = Vec::new();
fn notes() -> &'static mut Vec<Note> {
    unsafe { &mut *std::ptr::addr_of_mut!(NOTES) }
}
pub fn notes_count() -> usize {
    load_notes().len()
}
pub fn notes_fill(_: bool) {
    *notes() = load_notes();
    let r = rows();
    r.clear();
    let arg = ps().arg.trim().to_string();
    let writing = cur().map(|p| p.key == "note").unwrap_or(false) && !arg.is_empty();
    if writing {
        r.push(PRow {
            glyph: "\u{E70B}".into(),
            title: arg.clone(),
            detail: "New note".into(),
            action: "Save  ↵".into(),
            tag: -1,
            on: true,
            ..Default::default()
        });
    }
    let a = if writing { String::new() } else { arg_lower() };
    let t = calc::unix_now();
    for (i, n) in notes().iter().enumerate().rev() {
        if !matches(&n.text, &a) {
            continue;
        }
        r.push(PRow {
            glyph: "\u{E70B}".into(),
            title: one_line(&n.text, 120),
            detail: ago(t - n.at),
            action: "Copy  ↵   Del delete".into(),
            tag: i as i32,
            ..Default::default()
        });
    }
}
pub fn notes_enter(sel: i32, _: bool) {
    if sel < 0 || sel as usize >= rows().len() {
        return;
    }
    let i = rows()[sel as usize].tag;
    if i < 0 {
        let mut n = load_notes();
        n.push(Note { at: calc::unix_now(), text: ps().arg.trim().to_string() });
        save_notes(&n);
        flash("Note saved");
        set_text(&ws("> notes"));
        return;
    }
    if let Some(n) = notes().get(i as usize) {
        let t = n.text.clone();
        if set_clipboard_text(&t) {
            flash("Copied");
        }
    }
}
pub fn notes_del(sel: i32) -> bool {
    if sel < 0 || sel as usize >= rows().len() || rows()[sel as usize].tag < 0 {
        return true;
    }
    let i = rows()[sel as usize].tag as usize;
    let mut n = load_notes();
    if i < n.len() {
        n.remove(i);
        save_notes(&n);
        flash("Note deleted");
    }
    fill(false);
    true
}

// ---- power: lock, sleep, restart, shut down, sign out --------------------------------------------
fn enable_shutdown_privilege() -> bool {
    use windows::Win32::Security::*;
    unsafe {
        let mut tok = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut tok).is_err() {
            return false;
        }
        let mut tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, ..Default::default() };
        let _ = LookupPrivilegeValueW(PCWSTR::null(), SE_SHUTDOWN_NAME, &mut tp.Privileges[0].Luid);
        tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
        let ok = AdjustTokenPrivileges(tok, FALSE, Some(&tp), 0, None, None).is_ok() && GetLastError() == ERROR_SUCCESS;
        let _ = CloseHandle(tok);
        ok
    }
}
pub const POWER: [(&str, &str, &str, &str); 5] = [
    ("lock", "Lock", "\u{E72E}", "Lock this PC"),
    ("sleep", "Sleep", "\u{E708}", "Put this PC to sleep"),
    ("restart", "Restart", "\u{E777}", "Restart this PC"),
    ("shutdown", "Shut down", "\u{E7E8}", "Shut down this PC"),
    ("signout", "Sign out", "\u{F3B1}", "Sign out of Windows"),
];
pub fn power_fill(_: bool) {
    let which = cur().map(|p| p.tag).unwrap_or(0) as usize;
    let (key, title, glyph, verb) = POWER[which];
    let armed = armed(&format!("power:{}", key));
    let r = rows();
    r.clear();
    r.push(PRow {
        glyph: glyph.into(),
        title: verb.into(),
        detail: if armed { "Are you sure?".into() } else { String::new() },
        action: if armed { "Press ↵ again".into() } else { format!("{}  ↵", title) },
        on: armed,
        ..Default::default()
    });
}
pub fn power_enter(_: i32, _: bool) {
    use windows::Win32::System::Power::SetSuspendState;
    use windows::Win32::System::Shutdown::*;
    let which = cur().map(|p| p.tag).unwrap_or(0) as usize;
    if !confirmed(&format!("power:{}", POWER[which].0)) {
        fill(false);
        return;
    }
    st().prev_fg = HWND::default();
    hide();
    let reason = SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED;
    unsafe {
        match which {
            0 => {
                let _ = LockWorkStation();
            }
            1 => {
                let _ = SetSuspendState(false, false, false);
            }
            2 => {
                enable_shutdown_privilege();
                let _ = ExitWindowsEx(EWX_REBOOT, reason);
            }
            3 => {
                enable_shutdown_privilege();
                let _ = ExitWindowsEx(EWX_SHUTDOWN | EWX_POWEROFF, reason);
            }
            _ => {
                let _ = ExitWindowsEx(EWX_LOGOFF, SHUTDOWN_REASON(0));
            }
        }
    }
}

// ---- > dark: Windows' own dark mode -------------------------------------------------------------
const PERSONALIZE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
pub fn windows_dark() -> bool {
    let mut v = 1u32;
    let mut sz = 4u32;
    unsafe {
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            PERSONALIZE,
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut c_void),
            Some(&mut sz),
        );
    }
    v == 0
}
fn set_windows_dark(dark: bool) {
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PERSONALIZE, 0, KEY_SET_VALUE, &mut k) == ERROR_SUCCESS {
            let v: u32 = if dark { 0 } else { 1 };
            let b = v.to_le_bytes();
            let _ = RegSetValueExW(k, w!("AppsUseLightTheme"), 0, REG_DWORD, Some(&b));
            let _ = RegSetValueExW(k, w!("SystemUsesLightTheme"), 0, REG_DWORD, Some(&b));
            let _ = RegCloseKey(k);
        }
    }
    // tell the taskbar and the apps
    std::thread::spawn(|| unsafe {
        let name = wz("ImmersiveColorSet");
        let mut r = 0usize;
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(name.as_ptr() as isize), SMTO_ABORTIFHUNG, 3000, Some(&mut r));
    });
}
pub fn dark_fill(_: bool) {
    let dark = windows_dark();
    let r = rows();
    r.clear();
    r.push(PRow {
        glyph: if dark { "\u{E708}".into() } else { "\u{E706}".into() },
        title: "Dark mode for Windows and apps".into(),
        detail: if dark { "On".into() } else { "Off".into() },
        on: dark,
        action: if dark { "Turn off  ↵".into() } else { "Turn on  ↵".into() },
        ..Default::default()
    });
}
pub fn dark_enter(_: i32, _: bool) {
    let dark = !windows_dark();
    set_windows_dark(dark);
    flash(if dark { "Dark mode on" } else { "Light mode on" });
    fill(false);
}

// ---- > wifi / > bluetooth (Windows' radio switches, asked on a worker thread) -------------------
pub static RADIO: [AtomicI32; 2] = [AtomicI32::new(-2), AtomicI32::new(-2)]; // -2 checking, -1 none, 0 off, 1 on
static RADIO_BUSY: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
pub fn radio_refresh(kind: usize, set_to: i32) {
    if RADIO_BUSY[kind].swap(true, SeqCst) {
        return;
    }
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mut failed = false;
        if set_to >= 0 {
            failed = !widgets::radio_set(kind as i32, set_to == 1);
        }
        RADIO[kind].store(widgets::radio_state(kind as i32), SeqCst);
        RADIO_BUSY[kind].store(false, SeqCst);
        CoUninitialize();
        let _ = PostMessageW(hwnd(), WM_APP_PANEL, WPARAM(if failed { 2 } else { 0 }), LPARAM(0));
    });
}
pub fn radio_fill(kind: usize) {
    let s = RADIO[kind].load(SeqCst);
    let r = rows();
    r.clear();
    r.push(PRow {
        glyph: if kind == 0 { if s == 0 { "\u{EB55}".into() } else { "\u{E701}".into() } } else { "\u{E702}".into() },
        title: if kind == 0 { "Wi‑Fi".into() } else { "Bluetooth".into() },
        detail: if s == -2 || RADIO_BUSY[kind].load(SeqCst) {
            "…".into()
        } else if s == -1 {
            "Not on this PC".into()
        } else if s == 1 {
            "On".into()
        } else {
            "Off".into()
        },
        on: s == 1,
        action: if s >= 0 {
            if s == 1 {
                "Turn off  ↵".into()
            } else {
                "Turn on  ↵".into()
            }
        } else {
            String::new()
        },
        ..Default::default()
    });
}
pub fn radio_enter(kind: usize) {
    let s = RADIO[kind].load(SeqCst);
    if s < 0 {
        return;
    }
    let to = if s == 1 { 0 } else { 1 };
    RADIO[kind].store(to, SeqCst); // shown right away
    radio_refresh(kind, to);
    fill(false);
}

// ---- the user's own commands: files in the Commands folder become "> name" ----------------------
pub fn commands_dir() -> String {
    format!("{}\\Commands", app_dir())
}
static mut USER_CMDS_AT: f64 = -1e9;
pub fn scan_user_commands() {
    unsafe {
        if now() - USER_CMDS_AT < 3.0 {
            return;
        }
        USER_CMDS_AT = now();
    }
    let list = &mut ps().user_cmds;
    list.clear();
    let dir = commands_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        use std::os::windows::fs::MetadataExt;
        if md.is_dir() || md.file_attributes() & FILE_ATTRIBUTE_HIDDEN.0 != 0 {
            continue;
        }
        let n = e.file_name().to_string_lossy().into_owned();
        let name = match n.rfind('.') {
            Some(d) if d > 0 => n[..d].to_string(),
            _ => n.clone(),
        };
        let lower = name.to_lowercase().replace(' ', "-");
        if lower == "readme" {
            continue;
        }
        list.push(UserCmd { name, lower, path: format!("{}\\{}", dir, n) });
    }
    list.sort_by(|a, b| a.lower.cmp(&b.lower));
}
pub fn user_cmd_match(ql: &str, q: &str) -> Option<(usize, String)> {
    scan_user_commands();
    for (i, c) in ps().user_cmds.iter().enumerate() {
        let k = &c.lower;
        if ql == k || (ql.len() > k.len() && ql.starts_with(k.as_str()) && ql.as_bytes()[k.len()] == b' ') {
            return Some((i, q.get(k.len().min(q.len())..).unwrap_or("").trim().to_string()));
        }
    }
    None
}
pub fn user_cmd_fill(_: bool) {
    let p = ps();
    p.rows.clear();
    let Some(c) = p.user_cmds.get(p.user_cmd.max(0) as usize) else { return };
    if p.user_cmd < 0 {
        return;
    }
    let row = PRow {
        icon: c.path.clone(),
        glyph: "\u{E756}".into(),
        title: if p.arg.is_empty() { c.name.clone() } else { format!("{}  {}", c.name, p.arg) },
        detail: "Your command".into(),
        action: "Run  ↵   ⇧↵ edit".into(),
        ..Default::default()
    };
    p.rows.push(row);
}
pub fn user_cmd_enter(_: i32, shift: bool) {
    let p = ps();
    if p.user_cmd < 0 || p.user_cmd as usize >= p.user_cmds.len() {
        return;
    }
    let path = p.user_cmds[p.user_cmd as usize].path.clone();
    let arg = p.arg.clone();
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    st().prev_fg = HWND::default();
    hide();
    if shift {
        unsafe {
            ShellExecuteW(HWND::default(), w!("open"), w!("notepad.exe"), &HSTRING::from(format!("\"{}\"", path)), PCWSTR::null(), SW_SHOWNORMAL);
        }
        return;
    }
    let dir = commands_dir();
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let d = HSTRING::from(dir.as_str());
        if ext == "ps1" {
            let params = format!("-NoProfile -ExecutionPolicy Bypass -File \"{}\" {}", path, arg);
            ShellExecuteW(HWND::default(), w!("open"), w!("powershell.exe"), &HSTRING::from(params), &d, SW_SHOWNORMAL);
        } else {
            let a = HSTRING::from(arg.as_str());
            ShellExecuteW(
                HWND::default(),
                w!("open"),
                &HSTRING::from(path.as_str()),
                if arg.is_empty() { PCWSTR::null() } else { PCWSTR(a.as_ptr()) },
                &d,
                SW_SHOWNORMAL,
            );
        }
        CoUninitialize();
    });
}
pub fn open_commands_folder() {
    let d = commands_dir();
    let _ = std::fs::create_dir_all(&d);
    let readme = format!("{}\\README.txt", d);
    if !std::path::Path::new(&readme).exists() {
        let t = concat!(
            "Every file in this folder becomes a command in Custom Start.\r\n",
            "\r\n",
            "  hello.ps1      ->  > hello\r\n",
            "  backup.bat     ->  > backup\r\n",
            "  My Tool.exe    ->  > my-tool\r\n",
            "  docs.url       ->  > docs\r\n",
            "\r\n",
            "Anything typed after the name is passed along: \"> hello world\" runs hello.ps1 with \"world\".\r\n",
            "PowerShell scripts run with -ExecutionPolicy Bypass. Shift+Enter opens the file in Notepad.\r\n",
        );
        let _ = std::fs::write(&readme, t);
    }
    st().prev_fg = HWND::default();
    hide();
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(d.as_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        USER_CMDS_AT = -1e9;
    }
}
