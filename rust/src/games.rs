// Games found on this PC: Steam, Epic, GOG, Ubisoft and Xbox libraries, game shortcuts in the
// Start menu, and the usual game folders on every fixed drive. Same rules as the C++ backend.

use super::*;
use std::collections::HashSet;
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;

pub static GAMES: RwLock<Vec<App>> = RwLock::new(Vec::new());
pub static GAMES_LOADING: AtomicBool = AtomicBool::new(false);

const DRIVE_FIXED: u32 = 3;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

// ---------------------------------------------------------------------------
// Small helpers shared with the pins code
// ---------------------------------------------------------------------------
pub fn make_app(name: &str, target: &str, icon: &str, detail: &str) -> App {
    let n = ws(name);
    let lw = lower(&n);
    App { initials: initials(&lw), lower: lw, name: n, target: ws(target), alias: Vec::new(), icon: ws(icon), detail: ws(detail) }
}

pub fn file_stem(path: &str) -> String {
    let f = match path.rfind(['\\', '/']) {
        Some(i) => &path[i + 1..],
        None => path,
    };
    match f.rfind('.') {
        Some(d) => f[..d].to_string(),
        None => f.to_string(),
    }
}

pub fn parent_dir(path: &str) -> String {
    match path.rfind(['\\', '/']) {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

pub fn dir_exists(p: &str) -> bool {
    !p.is_empty() && Path::new(p).is_dir()
}
pub fn file_exists(p: &str) -> bool {
    !p.is_empty() && Path::new(p).is_file()
}

pub fn known_folder_path(id: &windows::core::GUID) -> String {
    unsafe {
        match SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), HANDLE::default()) {
            Ok(p) => {
                let s = String::from_utf16_lossy(p.as_wide());
                CoTaskMemFree(Some(p.0 as *const c_void));
                s
            }
            Err(_) => String::new(),
        }
    }
}

pub fn expand_env(s: &str) -> String {
    let src = HSTRING::from(s);
    let mut buf = vec![0u16; 1024];
    let n = unsafe { ExpandEnvironmentStringsW(&src, Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return s.to_string();
    }
    String::from_utf16_lossy(&buf[..n.saturating_sub(1)])
}

// UTF-8 (with or without BOM) or UTF-16 LE text.
pub fn read_text_file(path: &str) -> String {
    let Ok(md) = std::fs::metadata(path) else { return String::new() };
    if md.len() == 0 || md.len() >= 8 * 1024 * 1024 {
        return String::new();
    }
    let Ok(d) = std::fs::read(path) else { return String::new() };
    if d.len() >= 2 && d[0] == 0xFF && d[1] == 0xFE {
        let w: Vec<u16> = d[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&w);
    }
    let skip = if d.len() >= 3 && d[0] == 0xEF && d[1] == 0xBB { 3 } else { 0 };
    String::from_utf8_lossy(&d[skip..]).into_owned()
}

pub fn reg_str(root: HKEY, key: &str, name: &str) -> String {
    let mut buf = vec![0u16; 1024];
    let mut sz = (buf.len() * 2) as u32;
    let (k, n) = (HSTRING::from(key), HSTRING::from(name));
    let r = unsafe {
        RegGetValueW(root, &k, &n, RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ, None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut sz))
    };
    if r != ERROR_SUCCESS {
        return String::new();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn reg_subkeys(root: HKEY, key: &str) -> Vec<String> {
    let mut r = Vec::new();
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(root, &HSTRING::from(key), 0, KEY_READ, &mut k) != ERROR_SUCCESS {
            return r;
        }
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut n = 256u32;
            if RegEnumKeyExW(k, i, PWSTR(name.as_mut_ptr()), &mut n, None, PWSTR::null(), None, None) != ERROR_SUCCESS {
                break;
            }
            r.push(String::from_utf16_lossy(&name[..n as usize]));
        }
        let _ = RegCloseKey(k);
    }
    r
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------
fn norm_name(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn skip_exe(lower_name: &str) -> bool {
    const BAD: [&str; 35] = [
        "unins", "crash", "report", "redist", "setup", "vc_", "vcredist", "dxsetup", "helper", "prereq", "cef", "update",
        "easyanticheat", "battleye", "beservice", "install", "launcherpatcher", "dotnet", "directx", "physx", "touchup",
        "cleanup", "diagnostic", "benchmark", "server", "editor", "sdk", "tool", "7z", "python", "java", "node",
        "bugsplat", "overlay", "webhelper",
    ];
    BAD.iter().any(|b| lower_name.contains(b))
}

fn collect_exes(dir: &Path, depth: i32, out: &mut Vec<(String, u64)>, budget: &mut i32) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        *budget -= 1;
        if *budget < 0 {
            break;
        }
        let Ok(md) = e.metadata() else { continue };
        let ln = e.file_name().to_string_lossy().to_lowercase();
        if md.is_dir() {
            if depth > 0
                && md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
                && !ln.contains("redist")
                && !ln.contains("support")
                && ln != "engine"
                && !ln.contains("directx")
                && ln != "_commonredist"
            {
                collect_exes(&e.path(), depth - 1, out, budget);
            }
        } else if md.is_file() && ln.len() > 4 && ln.ends_with(".exe") && !skip_exe(&ln) {
            out.push((e.path().to_string_lossy().into_owned(), md.len()));
        }
    }
}

// The game's own exe: prefer one named like its folder, then the biggest.
fn find_main_exe(dir: &str, title: &str) -> String {
    let mut exes = Vec::new();
    let mut budget = 4000;
    collect_exes(Path::new(dir), 2, &mut exes, &mut budget);
    let nt = norm_name(title);
    let nd = norm_name(&file_stem(dir));
    let mut best = String::new();
    let mut best_score = -1.0f64;
    for (path, size) in exes {
        let ns = norm_name(&file_stem(&path));
        let mut s = size as f64;
        if !ns.is_empty() && (ns == nt || ns == nd) {
            s += 1e12;
        } else if !ns.is_empty()
            && ((!nt.is_empty() && (nt.contains(&ns) || ns.contains(&nt))) || (!nd.is_empty() && (nd.contains(&ns) || ns.contains(&nd))))
        {
            s += 1e11;
        }
        if ns.contains("launcher") {
            s *= 0.5;
        }
        if s > best_score {
            best_score = s;
            best = path;
        }
    }
    best
}

#[derive(Default)]
struct GameScan {
    games: Vec<App>,
    names: HashSet<String>,
    dirs: HashSet<String>,
}

impl GameScan {
    fn add(&mut self, name: &str, target: &str, icon: &str, detail: &str, dir: &str) {
        let n = norm_name(name);
        if n.is_empty() || self.names.contains(&n) {
            return;
        }
        // launchers and their helpers live in the same folders as games
        const BAD: [&str; 16] = [
            "recovery", "launcher", "anticheat", "redist", "uninstall", "crashreport", "easyanticheat", "eadesktop", "eaapp",
            "originclient", "socialclub", "riotclient", "vanguard", "overwolf", "directx", "commonredist",
        ];
        if BAD.iter().any(|b| n.contains(b)) && !n.contains("minecraft") {
            return;
        }
        if !dir.is_empty() {
            let d = dir.to_lowercase();
            if self.dirs.contains(&d) {
                return;
            }
            self.dirs.insert(d);
        }
        self.names.insert(n);
        self.games.push(make_app(name, target, icon, detail));
    }
}

// Very small reader for Valve's KeyValues text: every "key" "value" pair, in order.
fn vdf_pairs(t: &str) -> Vec<(String, String)> {
    let mut r = Vec::new();
    let mut toks: Vec<String> = Vec::new();
    let cs: Vec<char> = t.chars().collect();
    let mut i = 0;
    let flush = |toks: &mut Vec<String>, r: &mut Vec<(String, String)>| {
        let mut k = 0;
        while k + 1 < toks.len() {
            r.push((toks[k].to_lowercase(), toks[k + 1].clone()));
            k += 2;
        }
        toks.clear();
    };
    while i < cs.len() {
        if cs[i] == '"' {
            let mut s = String::new();
            i += 1;
            while i < cs.len() && cs[i] != '"' {
                if cs[i] == '\\' && i + 1 < cs.len() {
                    i += 1;
                }
                s.push(cs[i]);
                i += 1;
            }
            toks.push(s);
        } else if cs[i] == '{' || cs[i] == '}' {
            if toks.len() % 2 == 1 {
                toks.pop(); // a section name
            }
            flush(&mut toks, &mut r);
        }
        i += 1;
    }
    flush(&mut toks, &mut r);
    r
}

fn json_str(j: &str, key: &str) -> String {
    let k = format!("\"{}\"", key);
    let Some(p) = j.find(&k) else { return String::new() };
    let rest = &j[p + k.len()..];
    let Some(c) = rest.find(':') else { return String::new() };
    let rest = rest[c + 1..].trim_start();
    let mut it = rest.chars();
    if it.next() != Some('"') {
        return String::new();
    }
    let mut s = String::new();
    while let Some(ch) = it.next() {
        match ch {
            '"' => break,
            '\\' => {
                if let Some(n) = it.next() {
                    s.push(n);
                }
            }
            _ => s.push(ch),
        }
    }
    s
}

fn slash_fix(s: &str) -> String {
    let mut s = s.replace('/', "\\");
    while s.ends_with('\\') {
        s.pop();
    }
    s
}

fn scan_steam(gs: &mut GameScan, extra_libs: &[String]) {
    let steam = slash_fix(&reg_str(HKEY_CURRENT_USER, "Software\\Valve\\Steam", "SteamPath"));
    let mut libs = Vec::new();
    if !steam.is_empty() {
        libs.push(steam.clone());
        for (k, v) in vdf_pairs(&read_text_file(&format!("{}\\steamapps\\libraryfolders.vdf", steam))) {
            if k == "path" {
                libs.push(slash_fix(&v));
            }
        }
    }
    libs.extend(extra_libs.iter().cloned());
    const TOOLS: [&str; 9] = ["228980", "1070560", "1391110", "1628350", "1493710", "2180100", "1826330", "2348590", "961940"];
    let mut seen = HashSet::new();
    for lib in libs {
        if !seen.insert(lib.to_lowercase()) {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(format!("{}\\steamapps", lib)) else { continue };
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().into_owned();
            let lf = fname.to_lowercase();
            if !lf.starts_with("appmanifest_") || !lf.ends_with(".acf") {
                continue;
            }
            let (mut id, mut name, mut dir) = (String::new(), String::new(), String::new());
            for (k, v) in vdf_pairs(&read_text_file(&format!("{}\\steamapps\\{}", lib, fname))) {
                if k == "appid" && id.is_empty() {
                    id = v;
                } else if k == "name" && name.is_empty() {
                    name = v;
                } else if k == "installdir" && dir.is_empty() {
                    dir = v;
                }
            }
            if id.is_empty() || name.is_empty() || TOOLS.contains(&id.as_str()) {
                continue;
            }
            let ln = name.to_lowercase();
            if ln.contains("redistributable")
                || ln.starts_with("proton")
                || ln.contains("steam linux runtime")
                || ln.contains("steamworks")
                || ln.contains("dedicated server")
            {
                continue;
            }
            let full = format!("{}\\steamapps\\common\\{}", lib, dir);
            if !dir_exists(&full) {
                continue;
            }
            let icon = find_main_exe(&full, &name);
            gs.add(&name, &format!("steam://rungameid/{}", id), &icon, "Steam", &full);
        }
    }
}

fn scan_epic(gs: &mut GameScan) {
    let dir = format!("{}\\Epic\\EpicGamesLauncher\\Data\\Manifests", known_folder_path(&FOLDERID_ProgramData));
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    for e in rd.flatten() {
        let fname = e.file_name().to_string_lossy().into_owned();
        if !fname.to_lowercase().ends_with(".item") {
            continue;
        }
        let j = read_text_file(&format!("{}\\{}", dir, fname));
        let name = json_str(&j, "DisplayName");
        let loc = slash_fix(&json_str(&j, "InstallLocation"));
        let exe = json_str(&j, "LaunchExecutable");
        let ns = json_str(&j, "CatalogNamespace");
        let item = json_str(&j, "CatalogItemId");
        let app_name = json_str(&j, "AppName");
        if name.is_empty() || loc.is_empty() || !dir_exists(&loc) || !j.contains("\"games\"") {
            continue;
        }
        let icon = if exe.is_empty() { find_main_exe(&loc, &name) } else { format!("{}\\{}", loc, slash_fix(&exe)) };
        let target = format!("com.epicgames.launcher://apps/{}%3A{}%3A{}?action=launch&silent=true", ns, item, app_name);
        gs.add(&name, &target, &icon, "Epic Games", &loc);
    }
}

fn scan_registry_launchers(gs: &mut GameScan) {
    let gog = "SOFTWARE\\WOW6432Node\\GOG.com\\Games";
    for id in reg_subkeys(HKEY_LOCAL_MACHINE, gog) {
        let k = format!("{}\\{}", gog, id);
        let name = reg_str(HKEY_LOCAL_MACHINE, &k, "gameName");
        let exe = reg_str(HKEY_LOCAL_MACHINE, &k, "exe");
        let path = slash_fix(&reg_str(HKEY_LOCAL_MACHINE, &k, "path"));
        if !name.is_empty() && file_exists(&exe) {
            gs.add(&name, &exe, "", "GOG", &path);
        }
    }
    let ubi = "SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs";
    for id in reg_subkeys(HKEY_LOCAL_MACHINE, ubi) {
        let dir = slash_fix(&reg_str(HKEY_LOCAL_MACHINE, &format!("{}\\{}", ubi, id), "InstallDir"));
        if dir.is_empty() || !dir_exists(&dir) {
            continue;
        }
        let name = file_stem(&format!("{}.x", dir));
        let icon = find_main_exe(&dir, &name);
        gs.add(&name, &format!("uplay://launch/{}/0", id), &icon, "Ubisoft", &dir);
    }
}

// Shortcuts the launchers put in the Start menu (.url to steam:// etc., and the folders of
// Riot, Roblox, Rockstar...).
fn scan_start_menu(gs: &mut GameScan, root: &str, folder: &str, depth: i32) {
    const GAME_FOLDERS: [&str; 14] = [
        "riot games", "roblox", "epic games", "ubisoft", "ea", "ea games", "rockstar games", "battle.net", "blizzard", "gog.com",
        "games", "steam", "minecraft", "xbox",
    ];
    const LAUNCHERS: [&str; 11] = [
        "riot client", "roblox studio", "ubisoft connect", "ea", "battle.net", "epic games launcher", "rockstar games launcher",
        "gog galaxy", "steam", "steam support center", "xbox",
    ];
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        let full = format!("{}\\{}", root, n);
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            if depth < 3 {
                scan_start_menu(gs, &full, if depth == 0 { &n } else { folder }, depth + 1);
            }
            continue;
        }
        let ln = n.to_lowercase();
        let stem = file_stem(&n);
        let ls = stem.to_lowercase();
        if ["uninstall", "readme", "website", "manual", "support"].iter().any(|b| ls.contains(b)) {
            continue;
        }
        if ln.len() > 4 && ln.ends_with(".url") {
            let t = read_text_file(&full).to_lowercase();
            let src = if t.contains("steam://rungameid/") {
                Some("Steam")
            } else if t.contains("com.epicgames.launcher://") {
                Some("Epic Games")
            } else if t.contains("uplay://") {
                Some("Ubisoft")
            } else if t.contains("origin2://") || t.contains("origin://") {
                Some("EA")
            } else {
                None
            };
            if let Some(src) = src {
                gs.add(&stem, &full, &full, src, "");
            }
        } else if ln.len() > 4 && ln.ends_with(".lnk") && !folder.is_empty() {
            let lf = folder.to_lowercase();
            let game_folder = GAME_FOLDERS.contains(&lf.as_str());
            let launcher = LAUNCHERS.contains(&ls.as_str());
            if game_folder && !launcher {
                gs.add(&stem, &full, &full, folder, "");
            }
        }
    }
}

fn scan_game_folder(gs: &mut GameScan, dir: &str, label: &str) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        if !md.is_dir() {
            continue;
        }
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with('$') {
            continue;
        }
        let ln = n.to_lowercase();
        if ln.contains("launcher")
            || ln == "riot client"
            || ln == "social club"
            || ln == "directxredist"
            || ln == "steamlibrary"
            || ln == "steam"
        {
            continue;
        }
        let full = format!("{}\\{}", dir, n);
        let exe = find_main_exe(&full, &n);
        if !exe.is_empty() {
            gs.add(&n, &exe, "", label, &full);
        }
    }
}

fn scan_all() -> Vec<App> {
    let mut gs = GameScan::default();
    let mut drives = Vec::new();
    let mut steam_libs = Vec::new();
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:", (b'A' + i as u8) as char);
        if unsafe { GetDriveTypeW(&HSTRING::from(format!("{}\\", root))) } != DRIVE_FIXED {
            continue;
        }
        for s in ["\\SteamLibrary", "\\Steam", "\\Games\\Steam", "\\Games\\SteamLibrary", "\\Program Files (x86)\\Steam", "\\Program Files\\Steam"] {
            if dir_exists(&format!("{}{}\\steamapps", root, s)) {
                steam_libs.push(format!("{}{}", root, s));
            }
        }
        drives.push(root);
    }

    scan_steam(&mut gs, &steam_libs);
    scan_epic(&mut gs);
    scan_registry_launchers(&mut gs);
    scan_start_menu(&mut gs, &known_folder_path(&FOLDERID_Programs), "", 0);
    scan_start_menu(&mut gs, &known_folder_path(&FOLDERID_CommonPrograms), "", 0);

    // Xbox app / Microsoft Store games: started through their Start entry
    let apps: Vec<(String, String)> =
        APPS.read().unwrap().iter().map(|a| (String::from_utf16_lossy(&a.name), String::from_utf16_lossy(&a.target))).collect();
    for d in &drives {
        let Ok(rd) = std::fs::read_dir(format!("{}\\XboxGames", d)) else { continue };
        for e in rd.flatten() {
            if !e.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                continue;
            }
            let n = e.file_name().to_string_lossy().into_owned();
            let nn = norm_name(&n);
            if let Some((name, target)) = apps.iter().find(|(name, _)| norm_name(name) == nn) {
                gs.add(name, target, "", "Xbox", &format!("{}\\XboxGames\\{}", d, n));
            }
        }
    }

    // Folders people keep games in, on every drive
    const FOLDERS: [(&str, &str); 15] = [
        ("\\Games", "Games"),
        ("\\Epic Games", "Epic Games"),
        ("\\Program Files\\Epic Games", "Epic Games"),
        ("\\GOG Games", "GOG"),
        ("\\Program Files (x86)\\GOG Galaxy\\Games", "GOG"),
        ("\\Riot Games", "Riot Games"),
        ("\\Program Files\\EA Games", "EA"),
        ("\\EA Games", "EA"),
        ("\\Program Files (x86)\\Origin Games", "EA"),
        ("\\Program Files (x86)\\Ubisoft\\Ubisoft Game Launcher\\games", "Ubisoft"),
        ("\\Ubisoft Games", "Ubisoft"),
        ("\\Program Files\\Rockstar Games", "Rockstar Games"),
        ("\\Rockstar Games", "Rockstar Games"),
        ("\\Battle.net", "Battle.net"),
        ("\\Program Files (x86)\\Battle.net", "Battle.net"),
    ];
    for d in &drives {
        for (f, label) in FOLDERS {
            let p = format!("{}{}", d, f);
            if dir_exists(&p) {
                scan_game_folder(&mut gs, &p, label);
            }
        }
    }

    gs.games.sort_by(|a, b| a.lower.cmp(&b.lower));
    gs.games
}

// Rescans at most once a minute, on a background thread; posts WM_APP_GAMES when done.
pub fn refresh_games() {
    let s = st();
    let t = now();
    if t - s.games_at < 60.0 {
        return;
    }
    if GAMES_LOADING.compare_exchange(false, true, SeqCst, SeqCst).is_err() {
        return;
    }
    s.games_at = t;
    std::thread::spawn(|| unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let games = scan_all();
        *GAMES.write().unwrap() = games;
        GAMES_LOADING.store(false, SeqCst);
        CoUninitialize();
        let h = hwnd();
        if !h.is_invalid() {
            let _ = PostMessageW(h, WM_APP_GAMES, WPARAM(0), LPARAM(0));
        }
    });
}
