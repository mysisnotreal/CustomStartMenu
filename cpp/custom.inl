// ---------------------------------------------------------------------------
// Making it yours: "> theme new" (your own theme), "> sounds", "> layout", "> hotkey".
// ---------------------------------------------------------------------------

// ---- your own theme -----------------------------------------------------------------------
static const struct { const wchar_t* name; UINT32 rgb; } kAccents[] = {
    {L"Sky", 0x60CDFF},  {L"Blue", 0x4C8DFF}, {L"Violet", 0xA78BFA}, {L"Pink", 0xFF7AC6}, {L"Red", 0xFF6B6B},  {L"Orange", 0xFF9E64},
    {L"Gold", 0xF5C451}, {L"Lime", 0xA3E635}, {L"Mint", 0x5EE6A8},   {L"Teal", 0x2DD4BF}, {L"Silver", 0xC0C4CC}, {L"Rose", 0xFDA4AF},
};
constexpr int kAccentCount = (int)(sizeof(kAccents) / sizeof(kAccents[0]));
static const wchar_t* kBases[] = {L"Dark", L"Light", L"Black", L"Navy"};
static const wchar_t* kMaterials[] = {L"Solid", L"Frosted", L"Clear"};
static int g_ctBase = 0, g_ctAccent = 0, g_ctMat = 1;  // being edited
static bool g_ctSaved = false;

static Theme BuildCustomTheme(int base, int accent, int mat) {
    Theme t = kThemes[0];
    t.name = L"Custom";
    static const float dTop[] = {.95f, .74f, .40f}, dBot[] = {.97f, .82f, .52f};
    static const float lTop[] = {.93f, .72f, .38f}, lBot[] = {.95f, .80f, .48f};
    D2D1_COLOR_F ac = C(kAccents[accent].rgb);
    if (base == 1) {
        t.tintTop = C(0xFAFAFA, lTop[mat]);
        t.tintBottom = C(0xF1F1F1, lBot[mat]);
        t.borderTop = C(0xFFFFFF, .80f);
        t.borderBottom = C(0x000000, .07f);
        t.text = C(0x1A1A1A);
        t.dim = C(0x6E6E6E);
        t.ghost = C(0xA3A3A3);
        t.highlight = C(0x000000, .055f);
        t.divider = C(0x000000, .07f);
        t.shadow = .30f;
        ac = LerpC(ac, C(0x000000), 0.38f);  // darker, so it reads on white
    } else {
        UINT32 top = base == 2 ? 0x050505 : base == 3 ? 0x141C33 : 0x2F2D2B, bot = base == 2 ? 0x000000 : base == 3 ? 0x0E1528 : 0x292725;
        t.tintTop = C(top, dTop[mat]);
        t.tintBottom = C(bot, dBot[mat]);
        t.borderTop = C(0xFFFFFF, .13f);
        t.borderBottom = C(0xFFFFFF, .04f);
        t.text = C(0xEDEBE8);
        t.dim = base == 3 ? C(0x7D879E) : C(0x8E8983);
        t.ghost = base == 3 ? C(0x5D667C) : C(0x726D67);
        t.highlight = C(0xFFFFFF, .07f);
        t.divider = C(0xFFFFFF, .07f);
        t.shadow = .62f;
        if (mat == 2) { t.text = C(0xFFFFFF); t.dim = C(0xD2D2D2); t.ghost = C(0xA8A8A8); }
    }
    t.accent = ac;
    return t;
}
constexpr int kCustomTheme = kThemeCount - 1;
static void LoadCustomTheme() {
    g_ctBase = std::clamp((int)ReadDword(L"CustomBase", 0), 0, 3);
    g_ctAccent = std::clamp((int)ReadDword(L"CustomAccent", 0), 0, kAccentCount - 1);
    g_ctMat = std::clamp((int)ReadDword(L"CustomMaterial", 1), 0, 2);
    kThemes[kCustomTheme] = BuildCustomTheme(g_ctBase, g_ctAccent, g_ctMat);
}
static void ThemeEditPreview() {
    kThemes[kCustomTheme] = BuildCustomTheme(g_ctBase, g_ctAccent, g_ctMat);
    g_thFrom = g_th;
    g_previewIdx = kCustomTheme;
    g_thStart = Now();
}
static std::vector<SettingDef> ThemeEditDefs() {
    std::vector<SettingDef> d;
    d.push_back({L"Base", L"\xE790", {kBases, kBases + 4}, [] { return g_ctBase; }, [](int v) { g_ctBase = v; ThemeEditPreview(); }});
    std::vector<std::wstring> acc;
    for (auto& a : kAccents) acc.push_back(a.name);
    d.push_back({L"Accent", L"\xE771", acc, [] { return g_ctAccent; }, [](int v) { g_ctAccent = v; ThemeEditPreview(); }});
    d.push_back({L"Material", L"\xF4A5", {kMaterials, kMaterials + 3}, [] { return g_ctMat; }, [](int v) { g_ctMat = v; ThemeEditPreview(); }});
    return d;
}
static void ThemeEditFill(bool first) {
    if (first) {
        g_ctSaved = false;
        ThemeEditPreview();
    }
    SettingsFill(ThemeEditDefs());
    g_prows[1].swatch = C(kAccents[g_ctAccent].rgb);
    PRow r;
    r.glyph = L"\xE73E";
    r.title = L"Use this theme";
    r.detail = g_themeIdx == kCustomTheme ? L"In use" : L"";
    r.action = L"Save  ↵";
    r.on = true;
    g_prows.push_back(r);
}
static void ThemeEditEnter(int sel, bool) {
    if (sel < 3) {
        SettingsEnter(ThemeEditDefs(), sel);
        g_prows[1].swatch = C(kAccents[g_ctAccent].rgb);
        return;
    }
    WriteDword(L"CustomBase", g_ctBase);
    WriteDword(L"CustomAccent", g_ctAccent);
    WriteDword(L"CustomMaterial", g_ctMat);
    g_ctSaved = true;
    ApplyTheme(kCustomTheme);
    Flash(L"Theme saved");
    PanelFill(false);
}
static bool ThemeEditKey(WPARAM vk, bool shift) {
    if (!SettingsKey(ThemeEditDefs(), vk, shift)) return false;
    ThemeEditFill(false);
    return true;
}
static void ThemeEditLeave() {
    if (!g_ctSaved) {
        LoadCustomTheme();  // back to what was saved
        SetPreview(g_themeIdx);
        if (g_previewIdx != g_themeIdx) g_previewIdx = g_themeIdx;
    }
}

// ---- sounds: made up on the spot (no files) -------------------------------------------------
static int g_sndType = 0, g_sndOpen = 0, g_sndVol = 2;
static const wchar_t* kTypeSounds[] = {L"Off", L"Soft", L"Typewriter", L"Bubble", L"Click"};
static const wchar_t* kOpenSounds[] = {L"Off", L"Whoosh", L"Chime", L"Pop"};
static const wchar_t* kVolumes[] = {L"25%", L"50%", L"75%", L"100%"};

static std::vector<char> MakeWav(const std::vector<float>& s) {
    const int rate = 44100;
    std::vector<char> w(44 + s.size() * 2);
    auto put32 = [&](size_t at, uint32_t v) { memcpy(&w[at], &v, 4); };
    auto put16 = [&](size_t at, uint16_t v) { memcpy(&w[at], &v, 2); };
    memcpy(&w[0], "RIFF", 4);
    put32(4, (uint32_t)(36 + s.size() * 2));
    memcpy(&w[8], "WAVEfmt ", 8);
    put32(16, 16);
    put16(20, 1);
    put16(22, 1);
    put32(24, rate);
    put32(28, rate * 2);
    put16(32, 2);
    put16(34, 16);
    memcpy(&w[36], "data", 4);
    put32(40, (uint32_t)(s.size() * 2));
    for (size_t i = 0; i < s.size(); i++) {
        int16_t v = (int16_t)std::clamp(s[i] * 32767.f, -32767.f, 32767.f);
        memcpy(&w[44 + i * 2], &v, 2);
    }
    return w;
}
static float Noise(uint32_t& seed) {
    seed = seed * 1664525u + 1013904223u;
    return ((seed >> 9) / 4194304.f) - 1.f;
}
// kind: 1-4 typing packs (variant 0-3), 11-13 open sounds, 20 timer chime, 30 delete
static std::vector<float> Synth(int kind, int variant) {
    const float R = 44100.f;
    std::vector<float> s;
    uint32_t seed = 1234 + variant * 977 + kind * 31;
    float pitch = 1.f + (variant - 1.5f) * 0.045f;
    auto len = [&](float sec) { s.assign((size_t)(sec * R), 0.f); };
    switch (kind) {
    case 1:  // soft: a muted tick
        len(0.035f);
        for (size_t i = 0; i < s.size(); i++) {
            float t = i / R;
            s[i] = 0.35f * std::sin(2 * kPI * 1650 * pitch * t) * std::exp(-t * 160) + 0.08f * Noise(seed) * std::exp(-t * 400);
        }
        break;
    case 2:  // typewriter: a clack and a thump
        len(0.06f);
        for (size_t i = 0; i < s.size(); i++) {
            float t = i / R;
            s[i] = 0.55f * Noise(seed) * std::exp(-t * 220) + 0.35f * std::sin(2 * kPI * 180 * pitch * t) * std::exp(-t * 70);
        }
        break;
    case 3:  // bubble: a quick upward blip
        len(0.07f);
        {
            float ph = 0;
            for (size_t i = 0; i < s.size(); i++) {
                float t = i / R, f = (520 + 1400 * t / 0.07f) * pitch;
                ph += 2 * kPI * f / R;
                s[i] = 0.4f * std::sin(ph) * std::sin(kPI * t / 0.07f);
            }
        }
        break;
    case 4:  // click: tiny and sharp
        len(0.012f);
        for (size_t i = 0; i < s.size(); i++) {
            float t = i / R;
            s[i] = 0.5f * (std::sin(2 * kPI * 3800 * pitch * t) > 0 ? 1.f : -1.f) * std::exp(-t * 600);
        }
        break;
    case 11: {  // whoosh: filtered noise swelling and fading
        len(0.32f);
        float lp = 0;
        for (size_t i = 0; i < s.size(); i++) {
            float t = i / R, env = std::sin(kPI * std::pow(t / 0.32f, 0.7f));
            float k = 0.02f + 0.25f * t / 0.32f;
            lp += (Noise(seed) - lp) * k;
            s[i] = 0.9f * lp * env;
        }
        break;
    }
    case 12:  // chime: two bell notes
    case 20: {
        float dur = kind == 20 ? 1.6f : 0.6f;
        len(dur);
        float notes[3] = {880, 1318.5f, 1760};
        int n = kind == 20 ? 3 : 2;
        for (int k = 0; k < n; k++) {
            size_t start = (size_t)(k * (kind == 20 ? 0.28f : 0.09f) * R);
            for (size_t i = start; i < s.size(); i++) {
                float t = (i - start) / R;
                float env = std::exp(-t * (kind == 20 ? 3.2f : 6.f)) * std::min(1.f, t * 400);
                s[i] += 0.22f * env * (std::sin(2 * kPI * notes[k] * t) + 0.3f * std::sin(2 * kPI * notes[k] * 2.01f * t));
            }
        }
        break;
    }
    case 13: {  // pop
        len(0.08f);
        float ph = 0;
        for (size_t i = 0; i < s.size(); i++) {
            float t = i / R, f = 900 * std::exp(-t * 30) + 140;
            ph += 2 * kPI * f / R;
            s[i] = 0.6f * std::sin(ph) * std::exp(-t * 45);
        }
        break;
    }
    }
    return s;
}
static std::unordered_map<int, std::vector<char>> g_sndCache;
static const std::vector<char>& SoundData(int kind, int variant) {
    int slot = kind * 4 + variant;
    if (g_sndCache[slot].empty()) {
        std::vector<float> s = Synth(kind, variant);
        float vol = (g_sndVol + 1) / 4.f;
        if (kind == 20) vol = std::max(vol, 0.75f);
        for (auto& x : s) x *= vol;
        g_sndCache[slot] = MakeWav(s);
    }
    return g_sndCache[slot];
}
static void SoundClearCache() {
    PlaySoundW(nullptr, nullptr, 0);
    g_sndCache.clear();
}
static void PlaySynth(int kind, int variant = 0) {
    const auto& d = SoundData(kind, variant);
    PlaySoundW((LPCWSTR)d.data(), nullptr, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
}
static int g_typeVariant = 0;
static void SoundTyped() {
    if (g_sndType > 0) PlaySynth(g_sndType, (g_typeVariant++) % 4);
}
static void SoundOpened() {
    if (g_sndOpen > 0) PlaySynth(10 + g_sndOpen);
}
static void PlayChime() { PlaySynth(20); }
static void LoadSoundSettings() {
    g_sndType = std::clamp((int)ReadDword(L"SoundTyping", 0), 0, 4);
    g_sndOpen = std::clamp((int)ReadDword(L"SoundOpen", 0), 0, 3);
    g_sndVol = std::clamp((int)ReadDword(L"SoundVolume", 2), 0, 3);
}
static std::vector<SettingDef> SoundDefs() {
    return {
        {L"Typing", L"\xE765", {kTypeSounds, kTypeSounds + 5}, [] { return g_sndType; },
         [](int v) { g_sndType = v; WriteDword(L"SoundTyping", v); if (v) PlaySynth(v, 1); }},
        {L"Opening the bar", L"\xE8A7", {kOpenSounds, kOpenSounds + 4}, [] { return g_sndOpen; },
         [](int v) { g_sndOpen = v; WriteDword(L"SoundOpen", v); if (v) PlaySynth(10 + v); }},
        {L"Volume", L"\xE767", {kVolumes, kVolumes + 4}, [] { return g_sndVol; },
         [](int v) {
             g_sndVol = v;
             WriteDword(L"SoundVolume", v);
             SoundClearCache();
             PlaySynth(g_sndType ? g_sndType : 12, 1);
         }},
    };
}

// ---- layout ---------------------------------------------------------------------------------
static int g_layPos = 0, g_layWidth = 1, g_layCorners = 0;
static const wchar_t* kLayPos[] = {L"Middle", L"Higher", L"Top"};
static const wchar_t* kLayWidth[] = {L"Compact", L"Normal", L"Wide"};
static const wchar_t* kLayCorners[] = {L"Round", L"Rounder", L"Square"};
static const wchar_t* kLayAnim[] = {L"Rise", L"Pop", L"Fade", L"Drop"};
static const float kWidths[] = {540.f, 620.f, 760.f};
static void ApplyLayout() {  // the size changes the next time the bar opens
    Lay::BarW = kWidths[g_layWidth];
    Lay::TileW = (Lay::BarW - (Lay::PinCols - 1) * Lay::TileGap) / Lay::PinCols;
    Lay::WinW = Lay::BarW + 2 * Lay::Margin;
    Lay::VerticalPos = g_layPos == 0 ? 0.40f : g_layPos == 1 ? 0.27f : 0.f;
}
static void ApplyCorners() { Lay::Radius = g_layCorners == 0 ? 14.f : g_layCorners == 1 ? 24.f : 5.f; }
static void LoadLayoutSettings() {
    g_layPos = std::clamp((int)ReadDword(L"LayoutPosition", 0), 0, 2);
    g_layWidth = std::clamp((int)ReadDword(L"LayoutWidth", 1), 0, 2);
    g_layCorners = std::clamp((int)ReadDword(L"LayoutCorners", 0), 0, 2);
    g_layAnim = std::clamp((int)ReadDword(L"LayoutAnimation", 0), 0, 3);
    g_stripOn = ReadDword(L"StatusStrip", 1) != 0;
    ApplyLayout();
    ApplyCorners();
}
static const wchar_t* kOnOff[] = {L"Off", L"On"};
static std::vector<SettingDef> LayoutDefs() {
    return {
        {L"Position on screen", L"\xE8A0", {kLayPos, kLayPos + 3}, [] { return g_layPos; },
         [](int v) { g_layPos = v; WriteDword(L"LayoutPosition", v); Flash(L"Moves the next time it opens"); }},
        {L"Width", L"\xE740", {kLayWidth, kLayWidth + 3}, [] { return g_layWidth; },
         [](int v) { g_layWidth = v; WriteDword(L"LayoutWidth", v); Flash(L"Resizes the next time it opens"); }},
        {L"Corners", L"\xF16B", {kLayCorners, kLayCorners + 3}, [] { return g_layCorners; },
         [](int v) { g_layCorners = v; WriteDword(L"LayoutCorners", v); ApplyCorners(); }},
        {L"Opening animation", L"\xE916", {kLayAnim, kLayAnim + 4}, [] { return g_layAnim; },
         [](int v) { g_layAnim = v; WriteDword(L"LayoutAnimation", v); }},
        {L"Clock and weather on the bar", L"\xE121", {kOnOff, kOnOff + 2}, [] { return g_stripOn ? 1 : 0; },
         [](int v) { g_stripOn = v != 0; WriteDword(L"StatusStrip", v); }},
    };
}

// ---- hotkey ---------------------------------------------------------------------------------
static int g_hotkey = 0;
static const wchar_t* kHotkeys[] = {L"None", L"Alt + Space", L"Ctrl + Space", L"Ctrl + Alt + Space", L"Ctrl + Shift + Space"};
static bool ApplyHotkey() {
    UnregisterHotKey(g_hwnd, 1);
    static const UINT mods[] = {0, MOD_ALT, MOD_CONTROL, MOD_CONTROL | MOD_ALT, MOD_CONTROL | MOD_SHIFT};
    if (g_hotkey == 0) return true;
    return RegisterHotKey(g_hwnd, 1, mods[g_hotkey] | MOD_NOREPEAT, VK_SPACE) != FALSE;
}
static void LoadHotkeySettings() {
    g_hotkey = std::clamp((int)ReadDword(L"Hotkey", 0), 0, 4);
    g_intercept = ReadDword(L"Intercept", 1) != 0;
}
static std::vector<SettingDef> HotkeyDefs() {
    return {
        {L"Windows key opens Custom Start", L"\xE782", {kOnOff, kOnOff + 2}, [] { return g_intercept ? 1 : 0; },
         [](int v) { g_intercept = v != 0; WriteDword(L"Intercept", v); }},
        {L"Also open with", L"\xE765", {kHotkeys, kHotkeys + 5}, [] { return g_hotkey; },
         [](int v) {
             g_hotkey = v;
             WriteDword(L"Hotkey", v);
             if (!ApplyHotkey()) Flash(L"Another app already uses that shortcut");
         }},
    };
}

static void TrayBalloon(const wchar_t* title, const std::wstring& text) {
    NOTIFYICONDATAW nid = {sizeof(nid)};
    nid.hWnd = g_hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    wcsncpy_s(nid.szInfoTitle, title, _TRUNCATE);
    wcsncpy_s(nid.szInfo, text.c_str(), _TRUNCATE);
    Shell_NotifyIconW(NIM_MODIFY, &nid);
}
