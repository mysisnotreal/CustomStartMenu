// ---------------------------------------------------------------------------
// Tools: switch windows, clipboard history, find files, close apps, notes, power, dark mode,
// Wi-Fi / Bluetooth, and the user's own commands (the Commands folder).
// ---------------------------------------------------------------------------

static std::wstring ProcessPath(DWORD pid) {
    std::wstring out;
    if (HANDLE p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid)) {
        wchar_t buf[MAX_PATH];
        DWORD n = MAX_PATH;
        if (QueryFullProcessImageNameW(p, 0, buf, &n)) out.assign(buf, n);
        CloseHandle(p);
    }
    return out;
}
static bool Matches(const std::wstring& hay, const std::wstring& needleLower) {
    return needleLower.empty() || Lower(hay).find(needleLower) != std::wstring::npos;
}

// ---- > windows --------------------------------------------------------------------------------
struct WinItem {
    HWND h;
    std::wstring title, exe, path;
};
static std::vector<WinItem> g_winItems;
static BOOL CALLBACK EnumWinProc(HWND h, LPARAM) {
    if (!IsWindowVisible(h) || h == g_hwnd || GetWindow(h, GW_OWNER)) return TRUE;
    LONG ex = GetWindowLongW(h, GWL_EXSTYLE);
    if ((ex & WS_EX_TOOLWINDOW) && !(ex & WS_EX_APPWINDOW)) return TRUE;
    BOOL cloaked = FALSE;
    DwmGetWindowAttribute(h, DWMWA_CLOAKED, &cloaked, sizeof(cloaked));
    if (cloaked) return TRUE;
    wchar_t t[512];
    if (!GetWindowTextW(h, t, 512) || !t[0]) return TRUE;
    wchar_t cls[64] = {};
    GetClassNameW(h, cls, 64);
    if (!wcscmp(cls, L"Progman") || !wcscmp(cls, L"Shell_TrayWnd")) return TRUE;
    DWORD pid = 0;
    GetWindowThreadProcessId(h, &pid);
    WinItem w;
    w.h = h;
    w.title = t;
    w.path = ProcessPath(pid);
    w.exe = FileStem(w.path);
    g_winItems.push_back(w);
    return TRUE;
}
static void WindowsFill(bool first) {
    HWND keep = (!first && g_sel >= 0 && g_sel < (int)g_prows.size()) ? (HWND)(intptr_t)g_prows[g_sel].tag : nullptr;
    g_winItems.clear();
    EnumWindows(EnumWinProc, 0);
    std::wstring a = Lower(g_panelArg);
    g_prows.clear();
    for (auto& w : g_winItems) {
        if (!Matches(w.title, a) && !Matches(w.exe, a)) continue;
        PRow r;
        r.icon = w.path;
        r.glyph = L"\xE737";
        r.title = w.title;
        r.detail = w.exe;
        r.action = IsIconic(w.h) ? L"Restore  ↵" : L"Switch  ↵";
        r.tag = (int)(intptr_t)w.h;
        g_prows.push_back(r);
        if (keep && w.h == keep) g_sel = (int)g_prows.size() - 1;
    }
}
static void WindowsEnter(int sel, bool) {
    if (sel < 0 || sel >= (int)g_prows.size()) return;
    HWND h = (HWND)(intptr_t)g_prows[sel].tag;
    if (!IsWindow(h)) return;
    g_prevFg = nullptr;
    Hide();
    if (IsIconic(h)) ShowWindow(h, SW_RESTORE);
    SwitchToThisWindow(h, TRUE);
    SetForegroundWindow(h);
}
static bool WindowsDel(int sel) {
    if (sel < 0 || sel >= (int)g_prows.size()) return true;
    PostMessageW((HWND)(intptr_t)g_prows[sel].tag, WM_CLOSE, 0, 0);
    Flash(L"Closing " + g_prows[sel].detail);
    g_panelFillAt = Now() - 10;  // look again soon
    return true;
}

// ---- > clip: clipboard history (kept in memory only, never saved) -------------------------
struct ClipItem {
    std::wstring text;
    double at;
};
static std::deque<ClipItem> g_clips;
static UINT g_cfExclude, g_cfHistory;
static void ClipInit() {
    g_cfExclude = RegisterClipboardFormatW(L"ExcludeClipboardContentFromMonitorProcessing");
    g_cfHistory = RegisterClipboardFormatW(L"CanIncludeInClipboardHistory");
    AddClipboardFormatListener(g_hwnd);
}
// WM_CLIPBOARDUPDATE
static void ClipChanged() {
    if (IsClipboardFormatAvailable(g_cfExclude)) return;  // password managers ask not to be kept
    if (!OpenClipboard(g_hwnd)) return;
    bool skip = false;
    if (HANDLE hh = GetClipboardData(g_cfHistory)) {
        if (auto* d = (DWORD*)GlobalLock(hh)) {
            skip = *d == 0;
            GlobalUnlock(hh);
        }
    }
    std::wstring s;
    if (!skip)
        if (HANDLE h = GetClipboardData(CF_UNICODETEXT)) {
            if (auto* p = (const wchar_t*)GlobalLock(h)) {
                s = p;
                GlobalUnlock(h);
            }
        }
    CloseClipboard();
    if (skip || Trim(s).empty() || s.size() > 100000) return;
    for (size_t i = 0; i < g_clips.size(); i++)
        if (g_clips[i].text == s) {
            g_clips.erase(g_clips.begin() + i);
            break;
        }
    g_clips.push_front({s, UnixNow()});
    while (g_clips.size() > 25) g_clips.pop_back();
    if (g_mode == Mode::Panel && CurPanel() && CurPanel()->key == L"clip") PanelFill(false);
}
static void ClipFill(bool) {
    std::wstring a = Lower(g_panelArg);
    g_prows.clear();
    double now = UnixNow();
    for (int i = 0; i < (int)g_clips.size(); i++) {
        if (!Matches(g_clips[i].text, a)) continue;
        PRow r;
        r.glyph = L"\xE8C8";
        r.title = OneLine(g_clips[i].text);
        r.detail = Ago(now - g_clips[i].at);
        r.action = L"Copy  ↵";
        r.tag = i;
        g_prows.push_back(r);
    }
}
static void ClipEnter(int sel, bool) {
    if (sel < 0 || sel >= (int)g_prows.size()) return;
    int i = g_prows[sel].tag;
    if (i < 0 || i >= (int)g_clips.size()) return;
    if (SetClipboardText(g_clips[i].text)) Flash(L"Copied");
}
static bool ClipDel(int sel) {
    if (sel >= 0 && sel < (int)g_prows.size()) {
        int i = g_prows[sel].tag;
        if (i >= 0 && i < (int)g_clips.size()) g_clips.erase(g_clips.begin() + i);
        PanelFill(false);
    }
    return true;
}

// ---- > f / > files: names of the files in your folders, indexed in the background --------
struct FileEntry {
    std::wstring lower, path;
};
static SRWLOCK g_filesLock = SRWLOCK_INIT;
static std::vector<FileEntry> g_files;
static std::atomic<bool> g_filesLoading{false};
static double g_filesAt = -1e9;

static void IndexDir(const std::wstring& dir, std::vector<FileEntry>& out, int depth) {
    if (depth > 12 || out.size() > 300000) return;
    WIN32_FIND_DATAW fd;
    HANDLE f = FindFirstFileExW((dir + L"\\*").c_str(), FindExInfoBasic, &fd, FindExSearchNameMatch, nullptr, FIND_FIRST_EX_LARGE_FETCH);
    if (f == INVALID_HANDLE_VALUE) return;
    do {
        const wchar_t* n = fd.cFileName;
        if (n[0] == L'.' || (fd.dwFileAttributes & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM))) continue;
        std::wstring path = dir + L"\\" + n;
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
            if (fd.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) continue;
            std::wstring ln = Lower(n);
            if (ln == L"node_modules" || ln == L"__pycache__" || ln == L"target" || ln == L"venv" || ln == L".venv") continue;
            out.push_back({ln, path});
            IndexDir(path, out, depth + 1);
        } else out.push_back({Lower(n), path});
    } while (FindNextFileW(f, &fd) && out.size() <= 300000);
    FindClose(f);
}
static void FilesRefresh() {
    if (Now() - g_filesAt < 600 || g_filesLoading.exchange(true)) return;
    std::thread([] {
        std::vector<FileEntry> out;
        std::set<std::wstring> seen;
        for (REFKNOWNFOLDERID id : {FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Pictures, FOLDERID_Music,
                                    FOLDERID_Videos, FOLDERID_SkyDrive}) {
            std::wstring d = KnownFolderPath(id);
            if (d.empty() || !seen.insert(Lower(d)).second) continue;
            // skip a folder already inside another one (Documents inside OneDrive)
            bool inside = false;
            for (auto& s : seen)
                if (s != Lower(d) && StartsWith(Lower(d), s + L"\\")) inside = true;
            if (!inside) IndexDir(d, out, 0);
        }
        AcquireSRWLockExclusive(&g_filesLock);
        g_files.swap(out);
        ReleaseSRWLockExclusive(&g_filesLock);
        g_filesAt = Now();
        g_filesLoading = false;
        PostMessageW(g_hwnd, WM_APP_PANEL, 0, 0);
    }).detach();
}
static std::wstring ShortDir(const std::wstring& path) {
    std::wstring dir = ParentDir(path);
    std::wstring home = ExpandEnv(L"%USERPROFILE%");
    if (StartsWith(Lower(dir), Lower(home))) dir = L"~" + dir.substr(home.size());
    return dir;
}
static void FilesFill(bool) {
    FilesRefresh();
    g_prows.clear();
    std::wstring a = Lower(g_panelArg);
    if (a.empty()) return;
    std::vector<std::pair<int, int>> scored;
    AcquireSRWLockShared(&g_filesLock);
    for (int i = 0; i < (int)g_files.size(); i++) {
        size_t p = g_files[i].lower.find(a);
        if (p == std::wstring::npos) continue;
        int s = p == 0 ? 3000 : 1000;
        s -= (int)g_files[i].lower.size();
        s -= (int)std::count(g_files[i].path.begin(), g_files[i].path.end(), L'\\') * 4;
        scored.push_back({s, i});
    }
    size_t n = std::min<size_t>(scored.size(), 60);
    std::partial_sort(scored.begin(), scored.begin() + n, scored.end(),
                      [](const std::pair<int, int>& x, const std::pair<int, int>& y) { return x.first > y.first; });
    for (size_t k = 0; k < n; k++) {
        const FileEntry& f = g_files[scored[k].second];
        PRow r;
        r.icon = f.path;
        r.glyph = L"\xE7C3";
        r.title = f.path.substr(f.path.find_last_of(L'\\') + 1);
        r.detail = ShortDir(f.path);
        r.action = L"Open  ↵   ⇧↵ folder";
        r.tag = scored[k].second;
        g_prows.push_back(r);
    }
    ReleaseSRWLockShared(&g_filesLock);
}
static void FilesEnter(int sel, bool shift) {
    if (sel < 0 || sel >= (int)g_prows.size()) return;
    std::wstring path = g_prows[sel].icon;
    if (shift) {
        ShowInExplorer(path);
        return;
    }
    App a;
    a.name = g_prows[sel].title;
    a.target = path;
    Launch(&a, L"", false);
}

// ---- > kill: close every copy of an app -------------------------------------------------
struct ProcGroup {
    std::wstring name, path;
    std::vector<DWORD> pids;
    double mem = 0;
};
static std::vector<ProcGroup> g_procs;
static bool Protected(const std::wstring& lower) {
    static const wchar_t* keep[] = {L"system", L"idle", L"registry", L"smss.exe", L"csrss.exe", L"wininit.exe", L"winlogon.exe",
                                    L"services.exe", L"lsass.exe", L"svchost.exe", L"dwm.exe", L"fontdrvhost.exe", L"memory compression",
                                    L"secure system", L"lsaiso.exe", L"sihost.exe", L"ctfmon.exe", L"conhost.exe", L"audiodg.exe",
                                    L"spoolsv.exe", L"msmpeng.exe", L"searchindexer.exe", L"wudfhost.exe", L"system idle process"};
    for (auto* k : keep)
        if (lower == k) return true;
    return false;
}
static void KillFill(bool first) {
    std::wstring keepName = (!first && g_sel >= 0 && g_sel < (int)g_prows.size()) ? g_prows[g_sel].title : L"";
    std::unordered_map<std::wstring, ProcGroup> m;
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snap != INVALID_HANDLE_VALUE) {
        PROCESSENTRY32W pe = {sizeof(pe)};
        DWORD me = GetCurrentProcessId();
        for (BOOL ok = Process32FirstW(snap, &pe); ok; ok = Process32NextW(snap, &pe)) {
            if (pe.th32ProcessID == 0 || pe.th32ProcessID == 4 || pe.th32ProcessID == me) continue;
            std::wstring n = pe.szExeFile, ln = Lower(n);
            if (Protected(ln)) continue;
            ProcGroup& g = m[ln];
            g.name = n.size() > 4 && Lower(n).compare(n.size() - 4, 4, L".exe") == 0 ? n.substr(0, n.size() - 4) : n;
            g.pids.push_back(pe.th32ProcessID);
            if (HANDLE p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pe.th32ProcessID)) {
                PROCESS_MEMORY_COUNTERS pmc = {sizeof(pmc)};
                if (K32GetProcessMemoryInfo(p, &pmc, sizeof(pmc))) g.mem += (double)pmc.WorkingSetSize;
                if (g.path.empty()) {
                    wchar_t buf[MAX_PATH];
                    DWORD len = MAX_PATH;
                    if (QueryFullProcessImageNameW(p, 0, buf, &len)) g.path.assign(buf, len);
                }
                CloseHandle(p);
            }
        }
        CloseHandle(snap);
    }
    g_procs.clear();
    for (auto& kv : m) g_procs.push_back(kv.second);
    std::sort(g_procs.begin(), g_procs.end(), [](const ProcGroup& a, const ProcGroup& b) { return a.mem > b.mem; });
    std::wstring a = Lower(g_panelArg);
    g_prows.clear();
    for (int i = 0; i < (int)g_procs.size(); i++) {
        auto& g = g_procs[i];
        if (!Matches(g.name, a)) continue;
        PRow r;
        r.icon = g.path;
        r.glyph = L"\xE7EF";
        r.title = g.name;
        r.detail = (g.pids.size() > 1 ? std::to_wstring(g.pids.size()) + L" running · " : L"") + FormatBytes(g.mem);
        r.action = Armed(L"kill:" + Lower(g.name)) ? L"Press ↵ again to close" : L"Close  ↵";
        r.tag = i;
        g_prows.push_back(r);
        if (!keepName.empty() && g.name == keepName) g_sel = (int)g_prows.size() - 1;
    }
}
static void KillEnter(int sel, bool) {
    if (sel < 0 || sel >= (int)g_prows.size()) return;
    int i = g_prows[sel].tag;
    if (i < 0 || i >= (int)g_procs.size()) return;
    ProcGroup g = g_procs[i];
    if (!Confirmed(L"kill:" + Lower(g.name))) {
        PanelFill(false);
        return;
    }
    int closed = 0;
    for (DWORD pid : g.pids)
        if (HANDLE p = OpenProcess(PROCESS_TERMINATE, FALSE, pid)) {
            closed += TerminateProcess(p, 1) != FALSE;
            CloseHandle(p);
        }
    Flash(closed ? L"Closed " + g.name : L"Couldn't close " + g.name);
    Sleep(120);
    PanelFill(false);
}

// ---- > note / > notes -----------------------------------------------------------------------
struct Note {
    double at;
    std::wstring text;
};
static std::wstring NotesFile() { return AppDir() + L"\\Notes.txt"; }
static std::vector<Note> LoadNotes() {
    std::vector<Note> out;
    std::wstring t = ReadTextFile(NotesFile());
    size_t p = 0;
    while (p < t.size()) {
        size_t e = t.find(L'\n', p);
        if (e == std::wstring::npos) e = t.size();
        std::wstring line = t.substr(p, e - p);
        p = e + 1;
        if (!line.empty() && line.back() == L'\r') line.pop_back();
        size_t tab = line.find(L'\t');
        if (tab == std::wstring::npos || line.empty()) continue;
        std::wstring txt = line.substr(tab + 1);
        // stored with \n written as \\n
        std::wstring dec;
        for (size_t i = 0; i < txt.size(); i++) {
            if (txt[i] == L'\\' && i + 1 < txt.size()) {
                wchar_t c = txt[++i];
                dec.push_back(c == L'n' ? L'\n' : c == L't' ? L'\t' : c);
            } else dec.push_back(txt[i]);
        }
        out.push_back({_wtof(line.substr(0, tab).c_str()), dec});
    }
    return out;
}
static void SaveNotes(const std::vector<Note>& notes) {
    std::wstring t;
    for (auto& n : notes) {
        wchar_t ts[32];
        swprintf(ts, 32, L"%.0f", n.at);
        std::wstring enc;
        for (wchar_t c : n.text) {
            if (c == L'\\') enc += L"\\\\";
            else if (c == L'\n') enc += L"\\n";
            else if (c == L'\t') enc += L"\\t";
            else if (c != L'\r') enc.push_back(c);
        }
        t += ts + std::wstring(L"\t") + enc + L"\r\n";
    }
    std::string u = "\xEF\xBB\xBF" + WToUtf8(t);
    std::wstring f = NotesFile();
    HANDLE h = CreateFileW(f.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
    if (h != INVALID_HANDLE_VALUE) {
        DWORD w = 0;
        WriteFile(h, u.data(), (DWORD)u.size(), &w, nullptr);
        CloseHandle(h);
    }
}
static std::vector<Note> g_notes;
static void NotesFill(bool) {
    g_notes = LoadNotes();
    g_prows.clear();
    bool writing = CurPanel() && CurPanel()->key == L"note" && !Trim(g_panelArg).empty();
    if (writing) {
        PRow r;
        r.glyph = L"\xE70B";
        r.title = Trim(g_panelArg);
        r.detail = L"New note";
        r.action = L"Save  ↵";
        r.tag = -1;
        r.on = true;
        g_prows.push_back(r);
    }
    std::wstring a = writing ? L"" : Lower(g_panelArg);
    double now = UnixNow();
    for (int i = (int)g_notes.size() - 1; i >= 0; i--) {
        if (!Matches(g_notes[i].text, a)) continue;
        PRow r;
        r.glyph = L"\xE70B";
        r.title = OneLine(g_notes[i].text);
        r.detail = Ago(now - g_notes[i].at);
        r.action = L"Copy  ↵   Del delete";
        r.tag = i;
        g_prows.push_back(r);
    }
}
static void NotesEnter(int sel, bool) {
    if (sel < 0 || sel >= (int)g_prows.size()) return;
    int i = g_prows[sel].tag;
    if (i < 0) {
        auto notes = LoadNotes();
        notes.push_back({UnixNow(), Trim(g_panelArg)});
        SaveNotes(notes);
        Flash(L"Note saved");
        SetText(L"> notes");
        return;
    }
    if (i < (int)g_notes.size() && SetClipboardText(g_notes[i].text)) Flash(L"Copied");
}
static bool NotesDel(int sel) {
    if (sel < 0 || sel >= (int)g_prows.size() || g_prows[sel].tag < 0) return true;
    int i = g_prows[sel].tag;
    auto notes = LoadNotes();
    if (i < (int)notes.size()) {
        notes.erase(notes.begin() + i);
        SaveNotes(notes);
        Flash(L"Note deleted");
    }
    PanelFill(false);
    return true;
}

// ---- power: lock, sleep, restart, shut down, sign out ---------------------------------------
static bool EnableShutdownPrivilege() {
    HANDLE tok;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &tok)) return false;
    TOKEN_PRIVILEGES tp = {1};
    LookupPrivilegeValueW(nullptr, SE_SHUTDOWN_NAME, &tp.Privileges[0].Luid);
    tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
    AdjustTokenPrivileges(tok, FALSE, &tp, 0, nullptr, nullptr);
    bool ok = GetLastError() == ERROR_SUCCESS;
    CloseHandle(tok);
    return ok;
}
enum { PW_LOCK, PW_SLEEP, PW_RESTART, PW_SHUTDOWN, PW_SIGNOUT };
static const struct { const wchar_t* key; const wchar_t* title; const wchar_t* glyph; const wchar_t* verb; } kPower[] = {
    {L"lock", L"Lock", L"\xE72E", L"Lock this PC"},
    {L"sleep", L"Sleep", L"\xE708", L"Put this PC to sleep"},
    {L"restart", L"Restart", L"\xE777", L"Restart this PC"},
    {L"shutdown", L"Shut down", L"\xE7E8", L"Shut down this PC"},
    {L"signout", L"Sign out", L"\xF3B1", L"Sign out of Windows"},
};
static void PowerFill(int which) {
    g_prows.clear();
    PRow r;
    r.glyph = kPower[which].glyph;
    r.title = kPower[which].verb;
    bool armed = Armed(std::wstring(L"power:") + kPower[which].key);
    r.detail = armed ? L"Are you sure?" : L"";
    r.action = armed ? L"Press ↵ again" : std::wstring(kPower[which].title) + L"  ↵";
    r.on = armed;
    g_prows.push_back(r);
}
static void PowerEnter(int which) {
    if (!Confirmed(std::wstring(L"power:") + kPower[which].key)) {
        PanelFill(false);
        return;
    }
    g_prevFg = nullptr;
    Hide();
    switch (which) {
    case PW_LOCK: LockWorkStation(); break;
    case PW_SLEEP: SetSuspendState(FALSE, FALSE, FALSE); break;
    case PW_RESTART:
        EnableShutdownPrivilege();
        ExitWindowsEx(EWX_REBOOT, SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED);
        break;
    case PW_SHUTDOWN:
        EnableShutdownPrivilege();
        ExitWindowsEx(EWX_SHUTDOWN | EWX_POWEROFF, SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED);
        break;
    case PW_SIGNOUT: ExitWindowsEx(EWX_LOGOFF, 0); break;
    }
}

// ---- > dark: Windows' own dark mode ---------------------------------------------------------
static const wchar_t* kPersonalize = L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
static bool WindowsDark() {
    DWORD v = 1, sz = sizeof(v);
    RegGetValueW(HKEY_CURRENT_USER, kPersonalize, L"AppsUseLightTheme", RRF_RT_REG_DWORD, nullptr, &v, &sz);
    return v == 0;
}
static void SetWindowsDark(bool dark) {
    HKEY k;
    if (RegOpenKeyExW(HKEY_CURRENT_USER, kPersonalize, 0, KEY_SET_VALUE, &k) == ERROR_SUCCESS) {
        DWORD v = dark ? 0 : 1;
        RegSetValueExW(k, L"AppsUseLightTheme", 0, REG_DWORD, (BYTE*)&v, sizeof(v));
        RegSetValueExW(k, L"SystemUsesLightTheme", 0, REG_DWORD, (BYTE*)&v, sizeof(v));
        RegCloseKey(k);
    }
    // tell the taskbar and the apps
    std::thread([] {
        DWORD_PTR r;
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, (LPARAM)L"ImmersiveColorSet", SMTO_ABORTIFHUNG, 3000, &r);
    }).detach();
}
static void DarkFill(bool) {
    g_prows.clear();
    bool dark = WindowsDark();
    PRow r;
    r.glyph = dark ? L"\xE708" : L"\xE706";
    r.title = L"Dark mode for Windows and apps";
    r.detail = dark ? L"On" : L"Off";
    r.on = dark;
    r.action = dark ? L"Turn off  ↵" : L"Turn on  ↵";
    g_prows.push_back(r);
}
static void DarkEnter(int, bool) {
    bool dark = !WindowsDark();
    SetWindowsDark(dark);
    Flash(dark ? L"Dark mode on" : L"Light mode on");
    PanelFill(false);
}

// ---- > wifi / > bluetooth (Windows' radio switches, asked on a worker thread) --------------
static std::atomic<int> g_radio[2] = {{-2}, {-2}};  // -2 checking, -1 none, 0 off, 1 on
static std::atomic<bool> g_radioBusy[2] = {{false}, {false}};
static void RadioRefresh(int kind, int setTo = -1) {
    if (g_radioBusy[kind].exchange(true)) return;
    std::thread([kind, setTo] {
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
        bool failed = false;
        if (setTo >= 0) failed = !wb::RadioSet(kind, setTo == 1);
        g_radio[kind] = wb::RadioState(kind);
        g_radioBusy[kind] = false;
        CoUninitialize();
        PostMessageW(g_hwnd, WM_APP_PANEL, failed ? 2 : 0, 0);
    }).detach();
}
static void RadioFill(int kind) {
    g_prows.clear();
    int s = g_radio[kind];
    PRow r;
    r.glyph = kind == 0 ? (s == 0 ? L"\xEB55" : L"\xE701") : L"\xE702";
    r.title = kind == 0 ? L"Wi‑Fi" : L"Bluetooth";
    r.detail = s == -2 || g_radioBusy[kind] ? L"…" : s == -1 ? L"Not on this PC" : s ? L"On" : L"Off";
    r.on = s == 1;
    r.action = s >= 0 ? (s ? L"Turn off  ↵" : L"Turn on  ↵") : L"";
    g_prows.push_back(r);
}
static void RadioEnter(int kind) {
    int s = g_radio[kind];
    if (s < 0) return;
    g_radio[kind] = !s;  // shown right away
    RadioRefresh(kind, !s);
    PanelFill(false);
}

// ---- the user's own commands: files in the Commands folder become "> name" -----------------
static std::wstring CommandsDir() { return AppDir() + L"\\Commands"; }
static double g_userCmdsAt = -1e9;
static void ScanUserCommands() {
    if (Now() - g_userCmdsAt < 3) return;
    g_userCmdsAt = Now();
    g_userCmds.clear();
    WIN32_FIND_DATAW fd;
    HANDLE f = FindFirstFileW((CommandsDir() + L"\\*").c_str(), &fd);
    if (f == INVALID_HANDLE_VALUE) return;
    do {
        if (fd.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_HIDDEN)) continue;
        std::wstring n = fd.cFileName;
        UserCmd c;
        c.path = CommandsDir() + L"\\" + n;
        size_t dot = n.find_last_of(L'.');
        c.name = dot == std::wstring::npos || dot == 0 ? n : n.substr(0, dot);
        c.lower = Lower(c.name);
        for (auto& ch : c.lower)
            if (ch == L' ') ch = L'-';
        if (c.lower == L"readme") continue;
        g_userCmds.push_back(c);
    } while (FindNextFileW(f, &fd));
    FindClose(f);
    std::sort(g_userCmds.begin(), g_userCmds.end(), [](const UserCmd& a, const UserCmd& b) { return a.lower < b.lower; });
}
static int UserCmdMatch(const std::wstring& ql, const std::wstring& q, std::wstring& arg) {
    ScanUserCommands();
    for (int i = 0; i < (int)g_userCmds.size(); i++) {
        const std::wstring& k = g_userCmds[i].lower;
        if (ql == k || (ql.size() > k.size() && ql.compare(0, k.size(), k) == 0 && ql[k.size()] == L' ')) {
            arg = Trim(q.substr(std::min(k.size(), q.size())));
            return i;
        }
    }
    return -1;
}
static void UserCmdFill(bool) {
    g_prows.clear();
    if (g_userCmd < 0 || g_userCmd >= (int)g_userCmds.size()) return;
    auto& c = g_userCmds[g_userCmd];
    PRow r;
    r.icon = c.path;
    r.glyph = L"\xE756";
    r.title = c.name + (g_panelArg.empty() ? L"" : L"  " + g_panelArg);
    r.detail = L"Your command";
    r.action = L"Run  ↵   ⇧↵ edit";
    g_prows.push_back(r);
}
static void UserCmdEnter(int, bool shift) {
    if (g_userCmd < 0 || g_userCmd >= (int)g_userCmds.size()) return;
    UserCmd c = g_userCmds[g_userCmd];
    std::wstring arg = g_panelArg, ext = Lower(c.path.substr(c.path.find_last_of(L'.') + 1));
    g_prevFg = nullptr;
    Hide();
    if (shift) {
        ShellExecuteW(nullptr, L"open", L"notepad.exe", (L"\"" + c.path + L"\"").c_str(), nullptr, SW_SHOWNORMAL);
        return;
    }
    std::wstring dir = CommandsDir();
    std::thread([c, arg, ext, dir] {
        CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        if (ext == L"ps1") {
            std::wstring params = L"-NoProfile -ExecutionPolicy Bypass -File \"" + c.path + L"\" " + arg;
            ShellExecuteW(nullptr, L"open", L"powershell.exe", params.c_str(), dir.c_str(), SW_SHOWNORMAL);
        } else {
            ShellExecuteW(nullptr, L"open", c.path.c_str(), arg.empty() ? nullptr : arg.c_str(), dir.c_str(), SW_SHOWNORMAL);
        }
        CoUninitialize();
    }).detach();
}
static void OpenCommandsFolder() {
    std::wstring d = CommandsDir();
    CreateDirectoryW(d.c_str(), nullptr);
    std::wstring readme = d + L"\\README.txt";
    if (!FileExists(readme)) {
        std::string t =
            "Every file in this folder becomes a command in Custom Start.\r\n"
            "\r\n"
            "  hello.ps1      ->  > hello\r\n"
            "  backup.bat     ->  > backup\r\n"
            "  My Tool.exe    ->  > my-tool\r\n"
            "  docs.url       ->  > docs\r\n"
            "\r\n"
            "Anything typed after the name is passed along: \"> hello world\" runs hello.ps1 with \"world\".\r\n"
            "PowerShell scripts run with -ExecutionPolicy Bypass. Shift+Enter opens the file in Notepad.\r\n";
        HANDLE h = CreateFileW(readme.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_NEW, 0, nullptr);
        if (h != INVALID_HANDLE_VALUE) {
            DWORD w = 0;
            WriteFile(h, t.data(), (DWORD)t.size(), &w, nullptr);
            CloseHandle(h);
        }
    }
    g_prevFg = nullptr;
    Hide();
    ShellExecuteW(nullptr, L"open", d.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
    g_userCmdsAt = -1e9;
}
