// CustomStartMenu
// A tray application that replaces the Windows Start menu with a minimal,
// animated command bar. Press the Windows key (or click Start) to open it.
//
// Rendering: Direct2D + DirectWrite into a per-pixel-alpha layered window,
// presented once per vsync (DwmFlush) while visible for smooth animation.
// The frosted background is a blurred snapshot of the desktop behind the bar,
// and the shadow is a true Gaussian blur drawn as a nine-slice.

#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif
#define NOMINMAX

#include <windows.h>
#include <windowsx.h>
#include <shellapi.h>
#include <shlobj.h>
#include <shobjidl.h>
#include <knownfolders.h>
#include <shellscalingapi.h>
#include <dwmapi.h>
#include <d2d1_1.h>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <dcomp.h>
#include <dwrite.h>
#include <UIAutomation.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <condition_variable>
#include <cwctype>
#include <deque>
#include <mutex>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>

#pragma comment(lib, "user32.lib")
#pragma comment(lib, "gdi32.lib")
#pragma comment(lib, "shell32.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "oleaut32.lib")
#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "uuid.lib")
#pragma comment(lib, "d2d1.lib")
#pragma comment(lib, "dwrite.lib")
#pragma comment(lib, "dwmapi.lib")
#pragma comment(lib, "shcore.lib")
#pragma comment(lib, "d3d11.lib")
#pragma comment(lib, "dxgi.lib")
#pragma comment(lib, "dcomp.lib")

using D2D1::ColorF;
using D2D1::Matrix3x2F;

// ---------------------------------------------------------------------------
// Layout (device independent pixels)
// ---------------------------------------------------------------------------
namespace Lay {
constexpr float BarW = 620.f;
constexpr float BarH = 56.f;
constexpr float Margin = 52.f;  // room around the container for the shadow / animation
constexpr float Radius = 14.f;
constexpr float VerticalPos = 0.40f;  // bar center, as a fraction of work-area height
constexpr float LogoX = 20.f;
constexpr float LogoSize = 18.f;
constexpr float TextX = 54.f;
constexpr float RightPad = 18.f;
constexpr float FontSize = 16.f;
constexpr float HintSize = 13.f;
constexpr float HintMaxW = 240.f;
constexpr float CaretW = 2.f;
constexpr float CaretH = 20.f;
constexpr float ShadowSigma = 15.f;
constexpr float ShadowOffsetY = 8.f;

// Expanding panel
constexpr float RowH = 42.f;
constexpr float PanelPad = 6.f;
constexpr int MaxRows = 8;
constexpr int GridCols = 4;
constexpr float GridPad = 12.f;
constexpr float GridHeaderH = 28.f;
constexpr float CardGap = 10.f;
constexpr float CardPreviewH = 70.f;
constexpr float CardNameH = 28.f;
constexpr float MaxPanelH = PanelPad * 2 + RowH * MaxRows;

constexpr float WinW = BarW + 2 * Margin;
constexpr float WinMaxH = BarH + MaxPanelH + 2 * Margin;
}  // namespace Lay

// ---------------------------------------------------------------------------
// Themes
// ---------------------------------------------------------------------------
struct Theme {
    const wchar_t* name;
    D2D1_COLOR_F tintTop, tintBottom;      // drawn over the blurred backdrop (alpha = opacity)
    D2D1_COLOR_F borderTop, borderBottom;  // 1px edge highlight
    D2D1_COLOR_F text, dim, ghost, accent;
    D2D1_COLOR_F highlight;  // row / hover background
    D2D1_COLOR_F divider;
    float shadow;
};
static D2D1_COLOR_F C(UINT32 rgb, float a = 1.f) { return ColorF(rgb, a); }

static const Theme kThemes[] = {
    {L"Dark", C(0x2F2D2B, .95f), C(0x292725, .97f), C(0xFFFFFF, .13f), C(0xFFFFFF, .04f), C(0xEDEBE8),
     C(0x8E8983), C(0x726D67), C(0x60CDFF), C(0xFFFFFF, .07f), C(0xFFFFFF, .07f), .60f},
    {L"Light", C(0xFAFAFA, .93f), C(0xF1F1F1, .95f), C(0xFFFFFF, .80f), C(0x000000, .07f), C(0x1A1A1A),
     C(0x6E6E6E), C(0xA3A3A3), C(0x005FB8), C(0x000000, .055f), C(0x000000, .07f), .30f},
    {L"Translucent", C(0x1E1E1E, .38f), C(0x181818, .50f), C(0xFFFFFF, .22f), C(0xFFFFFF, .06f), C(0xFFFFFF),
     C(0xD2D2D2), C(0xA8A8A8), C(0x60CDFF), C(0xFFFFFF, .11f), C(0xFFFFFF, .12f), .45f},
    {L"Glass", C(0xFFFFFF, .38f), C(0xF4F4F4, .48f), C(0xFFFFFF, .60f), C(0xFFFFFF, .18f), C(0x101010),
     C(0x3C3C3C), C(0x6A6A6A), C(0x005FB8), C(0x000000, .07f), C(0x000000, .09f), .28f},
    {L"Midnight", C(0x171E31, .96f), C(0x10172A, .97f), C(0x9DB4FF, .16f), C(0x9DB4FF, .04f), C(0xE6EAF5),
     C(0x7D879E), C(0x5D667C), C(0x7AA2FF), C(0x9DB4FF, .09f), C(0x9DB4FF, .08f), .65f},
    {L"Sunset", C(0x3B2422, .96f), C(0x2C1B24, .97f), C(0xFFC9A8, .16f), C(0xFFC9A8, .04f), C(0xFBEDE6),
     C(0xB08E86), C(0x8A6C66), C(0xFF9E64), C(0xFFC9A8, .09f), C(0xFFC9A8, .08f), .60f},
    {L"Mint", C(0x1D2A25, .96f), C(0x16211D, .97f), C(0xB4FFDD, .14f), C(0xB4FFDD, .04f), C(0xE6F2EC),
     C(0x7FA394), C(0x5F7E72), C(0x5EE6A8), C(0xB4FFDD, .08f), C(0xB4FFDD, .07f), .60f},
    {L"OLED", C(0x000000, .985f), C(0x000000, .99f), C(0xFFFFFF, .12f), C(0xFFFFFF, .05f), C(0xFFFFFF),
     C(0x8A8A8A), C(0x5E5E5E), C(0x60CDFF), C(0xFFFFFF, .08f), C(0xFFFFFF, .09f), .70f},
};
constexpr int kThemeCount = (int)(sizeof(kThemes) / sizeof(kThemes[0]));

static D2D1_COLOR_F LerpC(const D2D1_COLOR_F& a, const D2D1_COLOR_F& b, float t) {
    return {a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t};
}
static Theme LerpTheme(const Theme& a, const Theme& b, float t) {
    Theme r = b;
    r.tintTop = LerpC(a.tintTop, b.tintTop, t);
    r.tintBottom = LerpC(a.tintBottom, b.tintBottom, t);
    r.borderTop = LerpC(a.borderTop, b.borderTop, t);
    r.borderBottom = LerpC(a.borderBottom, b.borderBottom, t);
    r.text = LerpC(a.text, b.text, t);
    r.dim = LerpC(a.dim, b.dim, t);
    r.ghost = LerpC(a.ghost, b.ghost, t);
    r.accent = LerpC(a.accent, b.accent, t);
    r.highlight = LerpC(a.highlight, b.highlight, t);
    r.divider = LerpC(a.divider, b.divider, t);
    r.shadow = a.shadow + (b.shadow - a.shadow) * t;
    return r;
}
static D2D1_COLOR_F WithA(D2D1_COLOR_F c, float a) {
    c.a *= a;
    return c;
}

// ---------------------------------------------------------------------------
// Commands ("> ...")
// ---------------------------------------------------------------------------
struct CmdDef {
    const wchar_t* key;
    const wchar_t* title;
    const wchar_t* glyph;  // Segoe Fluent Icons / MDL2
};
static const CmdDef kCmds[] = {
    {L"theme", L"Theme", L""},
    {L"expanded", L"Expanded results", L""},
    {L"startup", L"Start with Windows", L""},
    {L"quit", L"Quit Custom Start", L""},
};
constexpr int kCmdCount = (int)(sizeof(kCmds) / sizeof(kCmds[0]));
enum { CMD_THEME, CMD_EXPANDED, CMD_STARTUP, CMD_QUIT };

static const wchar_t* kClassName = L"CustomStartMenu.CommandBar";
// Placeholder in the language chosen in Setup (HKCU\Software\CustomStartMenu\Language).
static const struct { const wchar_t* code; const wchar_t* text; } kPlaceholders[] = {
    {L"en", L"Search apps, or type > for commands"},
    {L"es", L"Busca apps o escribe > para comandos"},
    {L"fr", L"Rechercher des applis, ou tapez > pour les commandes"},
    {L"de", L"Apps suchen oder > für Befehle eingeben"},
    {L"it", L"Cerca app o digita > per i comandi"},
    {L"pt", L"Pesquise apps ou digite > para comandos"},
    {L"ru", L"Поиск приложений или > для команд"},
    {L"ja", L"アプリを検索、または > でコマンド"},
    {L"zh", L"搜索应用，或输入 > 使用命令"},
    {L"ko", L"앱 검색 또는 > 를 입력해 명령 사용"},
};
static std::wstring g_placeholderText = kPlaceholders[0].text;
static const wchar_t* kRunKey = L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
static const wchar_t* kRunName = L"CustomStartMenu";
static const wchar_t* kSettingsKey = L"Software\\CustomStartMenu";
static const ULONG_PTR kInjectMagic = 0x43534D31;  // marks our own injected keys
constexpr int kBackdropScale = 4;                   // backdrop is blurred at 1/4 resolution

#define WM_APP_TOGGLE (WM_APP + 1)
#define WM_APP_TRAY (WM_APP + 2)
#define WM_APP_APPS (WM_APP + 3)
#define WM_APP_HIDE (WM_APP + 4)
#define WM_APP_QUIT (WM_APP + 5)  // sent by CSMInstaller before replacing the exe
#define TIMER_UPDATE_CHECK 1

enum { ID_OPEN = 1, ID_AUTOSTART, ID_INTERCEPT, ID_EXPANDED, ID_EXIT, ID_THEME0 = 100 };

enum class Mode { None, Results, Commands, Themes };

template <class T>
static void SafeRelease(T*& p) {
    if (p) { p->Release(); p = nullptr; }
}

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------
static HWND g_hwnd;
static UINT g_wmTaskbarCreated;
static HHOOK g_kbHook, g_msHook;
static HANDLE g_quitEvent;
static HICON g_trayIcon;

static ID2D1Factory1* g_d2d;
static IDWriteFactory* g_dw;
static IDWriteTextFormat *g_fmtText, *g_fmtHint, *g_fmtRow, *g_fmtRowRight, *g_fmtGlyph, *g_fmtHeader, *g_fmtCard;
static IDWriteTextLayout* g_placeholderLayout;

// Rendering backends. Hardware acceleration (the default, chosen in Setup) draws on the GPU
// into a DirectComposition swap chain, so nothing is copied back to the CPU. Without it,
// Direct2D renders in software into a layered window and never touches the GPU.
static bool g_hw = true;
static ID2D1RenderTarget* g_rt;     // the active target (one of the two below)
static ID2D1DCRenderTarget* g_dcrt;  // software
static ID3D11Device* g_d3d;          // hardware
static ID2D1Device* g_d2dDev;
static ID2D1DeviceContext* g_ctx;
static IDXGISwapChain1* g_swap;
static ID2D1Bitmap1* g_target;
static IDCompositionDevice* g_dcomp;
static IDCompositionTarget* g_dcTarget;
static IDCompositionVisual* g_dcVisual;
static int g_winH = -1;  // current window height (hardware path)
static ID2D1SolidColorBrush* g_solid;
static ID2D1LinearGradientBrush* g_logoBrush;
static ID2D1Bitmap* g_shadowBmp;
static float g_shadowDpi = 0;
static int g_shadowC = 0, g_shadowN = 0, g_shadowPad = 0;

// backdrop (blurred desktop snapshot)
static std::vector<UINT32> g_bdPx;
static int g_bdW = 0, g_bdH = 0;
static bool g_bdDirty = false;
static ID2D1Bitmap* g_bdBmp;
static ID2D1BitmapBrush* g_bdBrush;

static HDC g_memDC;
static HBITMAP g_dib, g_oldBmp;
static void* g_bits;
static int g_pxW, g_pxH;
static float g_dpi = 96.f;
static POINT g_winPos;

static LARGE_INTEGER g_qpf;

// settings
static int g_themeIdx = 0;
static bool g_expanded = true;
static bool g_autostart = false;

// theme animation
static int g_previewIdx = 0;
static Theme g_th, g_thFrom;
static double g_thStart = -10;

// show/hide animation
static bool g_visible = false;
static double g_anim = 0, g_animFrom = 0, g_animTo = 0, g_animStart = 0, g_animDur = 0.001;
static double g_showTime = 0, g_lastFrame = 0, g_lastHideTime = 0;

// text editing
static std::wstring g_text;
static std::vector<double> g_birth;  // per-character insertion time (fade-in)
static int g_caret = 0, g_anchor = 0;
static double g_lastInput = 0;
static float g_caretX = 0, g_trailX = 0, g_scroll = 0, g_scrollTarget = 0, g_placeholderA = 1;
static bool g_caretInit = false;

// panel
static Mode g_mode = Mode::None, g_drawMode = Mode::None;
static int g_sel = 0;
static float g_panelH = 0, g_contentA = 0, g_selY = 0;
static bool g_selYInit = false;
static float g_cardSel[kThemeCount];
struct HitItem {
    D2D1_RECT_F r;
    int index;
};
static std::vector<HitItem> g_hits;
static POINT g_lastMouse = {-1, -1};

// hooks: they run on their own high-priority thread so that input never waits on the UI
// thread, and only read the atomics below. The mouse hook is removed entirely while a game
// or other fullscreen app is in front (unless the bar is open).
#define WM_HOOK_SYNC (WM_APP + 20)
static DWORD g_hookTid;
static std::atomic<bool> g_intercept{true};
static std::atomic<bool> g_barOpen{false};
static std::atomic<bool> g_fullscreen{false};
static std::atomic<LONG> g_boxL{0}, g_boxT{0}, g_boxR{0}, g_boxB{0};  // bar rectangle on screen
static bool g_winDown = false, g_winEmitted = false, g_swallowEscUp = false, g_swallowClickUp = false;
static DWORD g_winVk = 0, g_winFlags = 0, g_winLastTime = 0;

// start button rectangles (found via UI Automation on a worker thread)
static SRWLOCK g_startLock = SRWLOCK_INIT;
static std::vector<RECT> g_startRects;

// app index
struct App {
    std::wstring name, lower, initials, target;
};
static SRWLOCK g_appsLock = SRWLOCK_INIT;
static std::vector<App> g_apps;
static std::atomic<bool> g_appsLoading{false};
static double g_appsLoadedAt = -1e9;

static std::vector<App> g_results;
static std::vector<int> g_cmdList;
static bool g_hasMatch = false;
static App g_match;
static std::wstring g_ghost, g_hintName;
static double g_hintTime = 0;

// app icons (loaded on a worker thread)
struct IconEntry {
    bool ready = false, failed = false;
    std::vector<UINT32> px;
    int w = 0, h = 0;
    ID2D1Bitmap* bmp = nullptr;
    double shownAt = 0;
};
static std::mutex g_iconMx;
static std::condition_variable g_iconCv;
static std::deque<std::wstring> g_iconQueue;
static std::unordered_map<std::wstring, IconEntry> g_icons;
static bool g_iconQuit = false;
static int g_iconPx = 32;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
static double Now() {
    LARGE_INTEGER t;
    QueryPerformanceCounter(&t);
    return double(t.QuadPart) / double(g_qpf.QuadPart);
}
static float Clamp01(double v) { return (float)std::clamp(v, 0.0, 1.0); }
static double EaseOutCubic(double t) { return 1 - std::pow(1 - t, 3); }
static double EaseOutQuart(double t) { return 1 - std::pow(1 - t, 4); }
static double EaseOutBack(double t) {
    const double c1 = 1.45, c3 = c1 + 1;
    return 1 + c3 * std::pow(t - 1, 3) + c1 * std::pow(t - 1, 2);
}
static float Damp(float cur, float target, float speed, float dt) {
    return cur + (target - cur) * (1.f - std::exp(-speed * dt));
}
static std::wstring Lower(const std::wstring& s) {
    std::wstring r = s;
    for (auto& c : r) c = (wchar_t)std::towlower(c);
    return r;
}
static std::wstring Trim(const std::wstring& s) {
    size_t a = 0, b = s.size();
    while (a < b && std::iswspace(s[a])) a++;
    while (b > a && std::iswspace(s[b - 1])) b--;
    return s.substr(a, b - a);
}
static std::wstring Initials(const std::wstring& lower) {
    std::wstring r;
    bool start = true;
    for (wchar_t c : lower) {
        bool sep = std::iswspace(c) || c == L'-' || c == L'_' || c == L'.' || c == L'(';
        if (sep) { start = true; continue; }
        if (start) r.push_back(c);
        start = false;
    }
    return r;
}
static bool StartsWith(const std::wstring& s, const std::wstring& p) { return s.compare(0, p.size(), p) == 0; }

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------
static DWORD ReadDword(const wchar_t* name, DWORD def) {
    DWORD v = 0, sz = sizeof(v);
    if (RegGetValueW(HKEY_CURRENT_USER, kSettingsKey, name, RRF_RT_REG_DWORD, nullptr, &v, &sz) == ERROR_SUCCESS)
        return v;
    return def;
}
static void WriteDword(const wchar_t* name, DWORD v) {
    HKEY k;
    if (RegCreateKeyExW(HKEY_CURRENT_USER, kSettingsKey, 0, nullptr, 0, KEY_SET_VALUE, nullptr, &k, nullptr) ==
        ERROR_SUCCESS) {
        RegSetValueExW(k, name, 0, REG_DWORD, (const BYTE*)&v, sizeof(v));
        RegCloseKey(k);
    }
}
static bool IsAutoStart() {
    return RegGetValueW(HKEY_CURRENT_USER, kRunKey, kRunName, RRF_RT_REG_SZ, nullptr, nullptr, nullptr) ==
           ERROR_SUCCESS;
}
static void SetAutoStart(bool on) {
    HKEY k;
    if (RegOpenKeyExW(HKEY_CURRENT_USER, kRunKey, 0, KEY_SET_VALUE, &k) != ERROR_SUCCESS) return;
    if (on) {
        wchar_t path[MAX_PATH];
        GetModuleFileNameW(nullptr, path, MAX_PATH);
        std::wstring v = L"\"" + std::wstring(path) + L"\"";
        RegSetValueExW(k, kRunName, 0, REG_SZ, (const BYTE*)v.c_str(), DWORD((v.size() + 1) * sizeof(wchar_t)));
    } else {
        RegDeleteValueW(k, kRunName);
    }
    RegCloseKey(k);
    g_autostart = IsAutoStart();
}

static void SetPreview(int idx) {
    idx = std::clamp(idx, 0, kThemeCount - 1);
    if (idx == g_previewIdx) return;
    g_thFrom = g_th;
    g_previewIdx = idx;
    g_thStart = Now();
}
static void ApplyTheme(int idx) {
    g_themeIdx = std::clamp(idx, 0, kThemeCount - 1);
    WriteDword(L"Theme", (DWORD)g_themeIdx);
    if (g_visible) SetPreview(g_themeIdx);
    else { g_previewIdx = g_themeIdx; g_th = g_thFrom = kThemes[g_themeIdx]; }
}

// ---------------------------------------------------------------------------
// App index (shell:AppsFolder covers both Win32 and Store apps)
// ---------------------------------------------------------------------------
static void UpdateState();

static DWORD WINAPI AppsThread(LPVOID) {
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    std::vector<App> apps;
    IShellItem* folder = nullptr;
    if (SUCCEEDED(SHGetKnownFolderItem(FOLDERID_AppsFolder, KF_FLAG_DEFAULT, nullptr, IID_PPV_ARGS(&folder)))) {
        IEnumShellItems* en = nullptr;
        if (SUCCEEDED(folder->BindToHandler(nullptr, BHID_EnumItems, IID_PPV_ARGS(&en)))) {
            IShellItem* it = nullptr;
            while (en->Next(1, &it, nullptr) == S_OK) {
                PWSTR name = nullptr, parse = nullptr;
                if (SUCCEEDED(it->GetDisplayName(SIGDN_NORMALDISPLAY, &name)) &&
                    SUCCEEDED(it->GetDisplayName(SIGDN_PARENTRELATIVEPARSING, &parse)) && name && *name) {
                    App a;
                    a.name = name;
                    a.target = parse;
                    a.lower = Lower(a.name);
                    a.initials = Initials(a.lower);
                    apps.push_back(std::move(a));
                }
                CoTaskMemFree(name);
                CoTaskMemFree(parse);
                it->Release();
            }
            en->Release();
        }
        folder->Release();
    }
    std::stable_sort(apps.begin(), apps.end(), [](const App& a, const App& b) { return a.lower < b.lower; });
    apps.erase(std::unique(apps.begin(), apps.end(), [](const App& a, const App& b) { return a.lower == b.lower; }),
               apps.end());

    AcquireSRWLockExclusive(&g_appsLock);
    g_apps.swap(apps);
    ReleaseSRWLockExclusive(&g_appsLock);

    g_appsLoading = false;
    if (g_hwnd) PostMessageW(g_hwnd, WM_APP_APPS, 0, 0);
    CoUninitialize();
    return 0;
}

static void RefreshApps(bool force) {
    double now = Now();
    if (!force && now - g_appsLoadedAt < 30.0) return;
    bool expected = false;
    if (!g_appsLoading.compare_exchange_strong(expected, true)) return;
    g_appsLoadedAt = now;
    HANDLE h = CreateThread(nullptr, 0, AppsThread, nullptr, 0, nullptr);
    if (h) CloseHandle(h); else g_appsLoading = false;
}

static int Score(const App& a, const std::wstring& q) {
    int base;
    size_t p = a.lower.find(q);
    if (p == 0) base = 3000;
    else if (p != std::wstring::npos) {
        wchar_t c = a.lower[p - 1];
        base = (std::iswspace(c) || c == L'-' || c == L'(' || c == L'.' || c == L'_') ? 2000 : 1000;
    } else if (q.size() >= 2 && a.initials.compare(0, q.size(), q) == 0) base = 1500;
    else return 0;
    int s = base - (int)a.lower.size();
    if (a.lower.find(L"uninstall") != std::wstring::npos || a.lower.find(L"readme") != std::wstring::npos ||
        a.lower.find(L"documentation") != std::wstring::npos)
        s -= 800;
    return std::max(s, 1);
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------
static bool LoadIconPixels(const std::wstring& target, int size, std::vector<UINT32>& px, int& w, int& h) {
    IShellItemImageFactory* f = nullptr;
    std::wstring path = L"shell:AppsFolder\\" + target;
    if (FAILED(SHCreateItemFromParsingName(path.c_str(), nullptr, IID_PPV_ARGS(&f))) || !f) return false;
    HBITMAP hb = nullptr;
    HRESULT hr = f->GetImage({size, size}, SIIGBF_ICONONLY, &hb);
    f->Release();
    if (FAILED(hr) || !hb) return false;
    BITMAP bm = {};
    GetObjectW(hb, sizeof(bm), &bm);
    w = bm.bmWidth;
    h = std::abs(bm.bmHeight);
    if (w <= 0 || h <= 0) { DeleteObject(hb); return false; }
    BITMAPINFO bi = {};
    bi.bmiHeader.biSize = sizeof(bi.bmiHeader);
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    px.assign((size_t)w * h, 0);
    HDC dc = GetDC(nullptr);
    int lines = GetDIBits(dc, hb, 0, h, px.data(), &bi, DIB_RGB_COLORS);
    ReleaseDC(nullptr, dc);
    DeleteObject(hb);
    if (!lines) return false;
    bool anyAlpha = false, straight = false;
    for (UINT32 p : px) {
        UINT32 a = p >> 24, r = (p >> 16) & 255, g = (p >> 8) & 255, b = p & 255;
        if (a) anyAlpha = true;
        if (r > a || g > a || b > a) straight = true;
    }
    for (UINT32& p : px) {
        if (!anyAlpha) { p |= 0xFF000000; continue; }
        if (straight) {
            UINT32 a = p >> 24, r = (p >> 16) & 255, g = (p >> 8) & 255, b = p & 255;
            p = (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) | (b * a / 255);
        }
    }
    return true;
}

static void IconWorker() {
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    for (;;) {
        std::wstring key;
        {
            std::unique_lock<std::mutex> lk(g_iconMx);
            g_iconCv.wait(lk, [] { return g_iconQuit || !g_iconQueue.empty(); });
            if (g_iconQuit) break;
            key = g_iconQueue.front();
            g_iconQueue.pop_front();
        }
        size_t bar = key.rfind(L'|');
        std::wstring target = key.substr(0, bar);
        int size = _wtoi(key.c_str() + bar + 1);
        std::vector<UINT32> px;
        int w = 0, h = 0;
        bool ok = LoadIconPixels(target, size, px, w, h);
        std::lock_guard<std::mutex> lk(g_iconMx);
        IconEntry& e = g_icons[key];
        e.px.swap(px);
        e.w = w;
        e.h = h;
        e.ready = ok;
        e.failed = !ok;
    }
    CoUninitialize();
}

// Returns the icon bitmap if loaded (and when it first appeared), else queues a load.
static ID2D1Bitmap* GetIcon(const std::wstring& target, double* shownAt) {
    std::wstring key = target + L"|" + std::to_wstring(g_iconPx);
    std::lock_guard<std::mutex> lk(g_iconMx);
    auto it = g_icons.find(key);
    if (it == g_icons.end()) {
        g_icons.emplace(key, IconEntry());
        g_iconQueue.push_back(key);
        g_iconCv.notify_one();
        return nullptr;
    }
    IconEntry& e = it->second;
    if (!e.ready) return nullptr;
    if (!e.bmp && g_rt) {
        D2D1_BITMAP_PROPERTIES bp =
            D2D1::BitmapProperties(D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
        g_rt->CreateBitmap(D2D1::SizeU(e.w, e.h), e.px.data(), e.w * 4, bp, &e.bmp);
        if (e.shownAt == 0) e.shownAt = Now();
    }
    if (shownAt) *shownAt = e.shownAt;
    return e.bmp;
}

// ---------------------------------------------------------------------------
// State: what the panel shows for the current text
// ---------------------------------------------------------------------------
static std::wstring CommandDetail(int c) {
    switch (c) {
    case CMD_THEME: return kThemes[g_themeIdx].name;
    case CMD_EXPANDED: return g_expanded ? L"On" : L"Off";
    case CMD_STARTUP: return g_autostart ? L"On" : L"Off";
    }
    return L"";
}

static void RecomputeGhost() {
    g_ghost.clear();
    std::wstring lt = Lower(g_text);
    if (g_mode == Mode::Commands) {
        if (g_sel >= 0 && g_sel < (int)g_cmdList.size()) {
            size_t gt = lt.find(L'>');
            std::wstring q = Lower(Trim(g_text.substr(gt + 1)));
            std::wstring key = kCmds[g_cmdList[g_sel]].key;
            if (!q.empty() && StartsWith(key, q) && !lt.empty() && !std::iswspace(lt.back()))
                g_ghost = key.substr(q.size());
        }
        return;
    }
    const App* a = nullptr;
    if (g_mode == Mode::Results && g_sel >= 0 && g_sel < (int)g_results.size()) a = &g_results[g_sel];
    else if (g_mode == Mode::None && g_hasMatch) a = &g_match;
    if (a && !lt.empty() && StartsWith(a->lower, lt) && lt.size() < a->name.size()) g_ghost = a->name.substr(g_text.size());
}

static void UpdateState() {
    Mode old = g_mode;
    g_results.clear();
    g_cmdList.clear();
    g_hasMatch = false;

    std::wstring tl = Lower(Trim(g_text));
    if (!tl.empty() && tl[0] == L'>') {
        std::wstring q = Trim(tl.substr(1));
        if (q == L"theme" || q == L"themes" || StartsWith(q, L"theme ")) {
            g_mode = Mode::Themes;
        } else {
            g_mode = Mode::Commands;
            for (int i = 0; i < kCmdCount; i++)
                if (q.empty() || StartsWith(kCmds[i].key, q) || Lower(kCmds[i].title).find(q) != std::wstring::npos)
                    g_cmdList.push_back(i);
        }
    } else if (!tl.empty()) {
        std::vector<std::pair<int, int>> scored;
        AcquireSRWLockShared(&g_appsLock);
        for (size_t i = 0; i < g_apps.size(); i++) {
            int s = Score(g_apps[i], tl);
            if (s > 0) scored.push_back({s, (int)i});
        }
        size_t n = std::min<size_t>(scored.size(), Lay::MaxRows);
        std::partial_sort(scored.begin(), scored.begin() + n, scored.end(),
                          [](const std::pair<int, int>& a, const std::pair<int, int>& b) { return a.first > b.first; });
        for (size_t i = 0; i < n; i++) g_results.push_back(g_apps[scored[i].second]);
        ReleaseSRWLockShared(&g_appsLock);
        g_hasMatch = !g_results.empty();
        if (g_hasMatch) g_match = g_results[0];
        g_mode = (g_expanded && g_hasMatch) ? Mode::Results : Mode::None;
    } else {
        g_mode = Mode::None;
    }

    if (g_mode != old) {
        if (old == Mode::Themes) SetPreview(g_themeIdx);  // leaving without applying reverts the preview
        g_sel = (g_mode == Mode::Themes) ? g_themeIdx : 0;
        if (g_mode != Mode::None) {
            if (old == Mode::None) g_contentA = 0;
            else g_contentA = std::min(g_contentA, 0.35f);
            g_drawMode = g_mode;
            g_selYInit = false;
        }
    } else if (g_mode == Mode::Results || g_mode == Mode::Commands) {
        g_sel = 0;
    }
    RecomputeGhost();

    std::wstring hint;
    if (g_mode == Mode::None && g_hasMatch && !StartsWith(g_match.lower, Lower(g_text))) hint = g_match.name;
    if (hint != g_hintName) { g_hintName = hint; g_hintTime = Now(); }
}

static int ItemCount() {
    switch (g_mode) {
    case Mode::Results: return (int)g_results.size();
    case Mode::Commands: return (int)g_cmdList.size();
    case Mode::Themes: return kThemeCount;
    default: return 0;
    }
}

static float ThemeGridHeight() {
    using namespace Lay;
    int rows = (kThemeCount + GridCols - 1) / GridCols;
    return GridPad + GridHeaderH + rows * (CardPreviewH + CardNameH) + (rows - 1) * 4.f + GridPad - 4.f;
}

static float PanelTargetH() {
    using namespace Lay;
    switch (g_mode) {
    case Mode::Results: return PanelPad * 2 + RowH * g_results.size();
    case Mode::Commands: return g_cmdList.empty() ? 0.f : PanelPad * 2 + RowH * g_cmdList.size();
    case Mode::Themes: return ThemeGridHeight();
    default: return 0.f;
    }
}

// ---------------------------------------------------------------------------
// Start button location (UI Automation, polled on a worker thread)
// ---------------------------------------------------------------------------
static BOOL CALLBACK EnumTaskbars(HWND h, LPARAM lp) {
    wchar_t cls[64];
    if (GetClassNameW(h, cls, 64) && (!wcscmp(cls, L"Shell_TrayWnd") || !wcscmp(cls, L"Shell_SecondaryTrayWnd")))
        reinterpret_cast<std::vector<HWND>*>(lp)->push_back(h);
    return TRUE;
}

// A game or other fullscreen app is in front (exclusive or borderless fullscreen).
static bool ForegroundIsFullscreen() {
    QUERY_USER_NOTIFICATION_STATE qs;
    if (SUCCEEDED(SHQueryUserNotificationState(&qs)) &&
        (qs == QUNS_RUNNING_D3D_FULL_SCREEN || qs == QUNS_BUSY || qs == QUNS_PRESENTATION_MODE))
        return true;
    HWND fg = GetForegroundWindow();
    if (!fg || fg == g_hwnd || fg == GetShellWindow() || fg == GetDesktopWindow()) return false;
    wchar_t cls[64] = {};
    GetClassNameW(fg, cls, 64);
    if (!wcscmp(cls, L"Progman") || !wcscmp(cls, L"WorkerW") || !wcscmp(cls, L"Shell_TrayWnd") ||
        !wcscmp(cls, L"Shell_SecondaryTrayWnd"))
        return false;
    LONG style = GetWindowLongW(fg, GWL_STYLE);
    if ((style & WS_CAPTION) == WS_CAPTION) return false;  // a normal maximized window
    RECT r;
    MONITORINFO mi = {sizeof(mi)};
    if (!GetWindowRect(fg, &r) || !GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mi))
        return false;
    return r.left <= mi.rcMonitor.left && r.top <= mi.rcMonitor.top && r.right >= mi.rcMonitor.right &&
           r.bottom >= mi.rcMonitor.bottom;
}

// Background watcher: notices games / fullscreen apps once a second, and (only when none is
// running) refreshes where the Start button is.
static DWORD WINAPI StartButtonThread(LPVOID) {
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    IUIAutomation* uia = nullptr;
    CoCreateInstance(__uuidof(CUIAutomation), nullptr, CLSCTX_INPROC_SERVER, __uuidof(IUIAutomation), (void**)&uia);
    IUIAutomationCondition* cond = nullptr;
    if (uia) {
        VARIANT v;
        VariantInit(&v);
        v.vt = VT_BSTR;
        v.bstrVal = SysAllocString(L"StartButton");
        uia->CreatePropertyCondition(UIA_AutomationIdPropertyId, v, &cond);
        VariantClear(&v);
    }
    int sinceQuery = 1000;
    do {
        bool fs = ForegroundIsFullscreen();
        if (fs != g_fullscreen.load()) {
            g_fullscreen = fs;
            if (g_hookTid) PostThreadMessageW(g_hookTid, WM_HOOK_SYNC, 0, 0);
            if (!fs) sinceQuery = 1000;  // the taskbar may have moved meanwhile
        }
        if (fs || !uia || !cond || ++sinceQuery < 3) continue;
        sinceQuery = 0;
        {
            std::vector<HWND> bars;
            EnumWindows(EnumTaskbars, (LPARAM)&bars);
            std::vector<RECT> rects;
            for (HWND h : bars) {
                IUIAutomationElement* root = nullptr;
                if (SUCCEEDED(uia->ElementFromHandle(h, &root)) && root) {
                    IUIAutomationElement* el = nullptr;
                    if (SUCCEEDED(root->FindFirst(TreeScope_Descendants, cond, &el)) && el) {
                        RECT r{};
                        if (SUCCEEDED(el->get_CurrentBoundingRectangle(&r)) && r.right > r.left && r.bottom > r.top)
                            rects.push_back(r);
                        el->Release();
                    }
                    root->Release();
                }
            }
            AcquireSRWLockExclusive(&g_startLock);
            g_startRects.swap(rects);
            ReleaseSRWLockExclusive(&g_startLock);
        }
    } while (WaitForSingleObject(g_quitEvent, 1000) == WAIT_TIMEOUT);
    SafeRelease(cond);
    SafeRelease(uia);
    CoUninitialize();
    return 0;
}

static bool PointOnStartButton(POINT pt) {
    bool hit = false;
    AcquireSRWLockShared(&g_startLock);
    for (const RECT& r : g_startRects)
        if (PtInRect(&r, pt)) { hit = true; break; }
    ReleaseSRWLockShared(&g_startLock);
    return hit;
}

// ---------------------------------------------------------------------------
// Low level hooks
// ---------------------------------------------------------------------------
static LRESULT CALLBACK KbdProc(int code, WPARAM wp, LPARAM lp) {
    if (code == HC_ACTION && g_intercept) {
        auto* k = reinterpret_cast<KBDLLHOOKSTRUCT*>(lp);
        if (k->dwExtraInfo != kInjectMagic) {
            bool down = (wp == WM_KEYDOWN || wp == WM_SYSKEYDOWN);
            DWORD vk = k->vkCode;
            if (vk == VK_LWIN || vk == VK_RWIN) {
                if (down) {
                    // New press (not auto-repeat)? Also recovers if a release was never seen
                    // (e.g. Win+L switches to the secure desktop before the key comes up).
                    if (!g_winDown || k->time - g_winLastTime > 600) {
                        g_winDown = true;
                        g_winEmitted = false;
                        g_winVk = vk;
                        g_winFlags = k->flags;
                    }
                    g_winLastTime = k->time;
                    // Hold the Win key back; the shell never sees a lone Win press, so the
                    // Start menu never opens. It is replayed if a shortcut key follows.
                    if (!g_winEmitted) return 1;
                } else if (g_winDown) {
                    bool emitted = g_winEmitted;
                    g_winDown = false;
                    if (!emitted) {
                        PostMessageW(g_hwnd, WM_APP_TOGGLE, 0, 0);
                        return 1;
                    }
                }
            } else if (down && g_winDown && !g_winEmitted) {
                // Win + <key> shortcut (Win+E, Win+R, Win+Shift+S...): replay Win-down, then this key.
                INPUT in[2] = {};
                in[0].type = INPUT_KEYBOARD;
                in[0].ki.wVk = (WORD)g_winVk;
                in[0].ki.dwFlags = (g_winFlags & LLKHF_EXTENDED) ? KEYEVENTF_EXTENDEDKEY : 0;
                in[0].ki.dwExtraInfo = kInjectMagic;
                in[1].type = INPUT_KEYBOARD;
                in[1].ki.wVk = (WORD)vk;
                in[1].ki.wScan = (WORD)k->scanCode;
                in[1].ki.dwFlags = (k->flags & LLKHF_EXTENDED) ? KEYEVENTF_EXTENDEDKEY : 0;
                in[1].ki.dwExtraInfo = kInjectMagic;
                g_winEmitted = true;
                if (SendInput(2, in, sizeof(INPUT)) == 2) return 1;
            } else if (down) {
                if (vk == VK_ESCAPE && (GetAsyncKeyState(VK_CONTROL) & 0x8000) &&
                    !(GetAsyncKeyState(VK_MENU) & 0x8000) && !(GetAsyncKeyState(VK_SHIFT) & 0x8000)) {
                    g_swallowEscUp = true;  // Ctrl+Esc also opens Start
                    PostMessageW(g_hwnd, WM_APP_TOGGLE, 0, 0);
                    return 1;
                }
            } else if (vk == VK_ESCAPE && g_swallowEscUp) {
                g_swallowEscUp = false;
                return 1;
            }
        }
    }
    return CallNextHookEx(nullptr, code, wp, lp);
}

// Called on the UI thread whenever the bar moves or resizes.
static void PublishBox() {
    float s = g_dpi / 96.f;
    LONG l = g_winPos.x + (LONG)(Lay::Margin * s), t = g_winPos.y + (LONG)(Lay::Margin * s);
    g_boxL = l;
    g_boxT = t;
    g_boxR = l + (LONG)(Lay::BarW * s);
    g_boxB = t + (LONG)((Lay::BarH + g_panelH) * s);
}

static void SetBarOpen(bool open) {
    if (g_barOpen.exchange(open) != open && g_hookTid) PostThreadMessageW(g_hookTid, WM_HOOK_SYNC, 0, 0);
}

static LRESULT CALLBACK MouseProc(int code, WPARAM wp, LPARAM lp) {
    if (code == HC_ACTION && wp != WM_MOUSEMOVE) {  // moves are the hot path: let them straight through
        auto* m = reinterpret_cast<MSLLHOOKSTRUCT*>(lp);
        if (!(m->flags & LLMHF_INJECTED)) {
            if (g_intercept && wp == WM_LBUTTONDOWN && PointOnStartButton(m->pt)) {
                g_swallowClickUp = true;
                PostMessageW(g_hwnd, WM_APP_TOGGLE, 0, 0);
                return 1;
            }
            if (wp == WM_LBUTTONUP && g_swallowClickUp) {
                g_swallowClickUp = false;
                return 1;
            }
            // Clicking anywhere outside the bar closes it (even if focus could not be taken).
            if ((wp == WM_LBUTTONDOWN || wp == WM_RBUTTONDOWN || wp == WM_MBUTTONDOWN) && g_barOpen) {
                POINT p = m->pt;
                if (p.x < g_boxL || p.x >= g_boxR || p.y < g_boxT || p.y >= g_boxB)
                    PostMessageW(g_hwnd, WM_APP_HIDE, 0, 0);
            }
        }
    }
    return CallNextHookEx(nullptr, code, wp, lp);
}

// Owns both low-level hooks. Runs at time-critical priority and does almost nothing else, so
// a busy CPU (a game, a compile) never makes the mouse or keyboard wait on Custom Start.
static void SyncMouseHook() {
    bool want = !g_fullscreen || g_barOpen;
    if (want && !g_msHook) {
        g_msHook = SetWindowsHookExW(WH_MOUSE_LL, MouseProc, GetModuleHandleW(nullptr), 0);
    } else if (!want && g_msHook) {
        UnhookWindowsHookEx(g_msHook);
        g_msHook = nullptr;
        g_swallowClickUp = false;
    }
}
static DWORD WINAPI HookThread(LPVOID ready) {
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
    MSG msg;
    PeekMessageW(&msg, nullptr, 0, 0, PM_NOREMOVE);  // create the message queue
    g_kbHook = SetWindowsHookExW(WH_KEYBOARD_LL, KbdProc, GetModuleHandleW(nullptr), 0);
    SyncMouseHook();
    SetEvent((HANDLE)ready);
    while (GetMessageW(&msg, nullptr, 0, 0) > 0)
        if (msg.message == WM_HOOK_SYNC) SyncMouseHook();
    if (g_msHook) UnhookWindowsHookEx(g_msHook);
    if (g_kbHook) UnhookWindowsHookEx(g_kbHook);
    g_msHook = g_kbHook = nullptr;
    return 0;
}

// ---------------------------------------------------------------------------
// Graphics resources
// ---------------------------------------------------------------------------
static bool FontExists(const wchar_t* family) {
    IDWriteFontCollection* c = nullptr;
    BOOL exists = FALSE;
    UINT32 idx = 0;
    if (SUCCEEDED(g_dw->GetSystemFontCollection(&c))) {
        c->FindFamilyName(family, &idx, &exists);
        c->Release();
    }
    return exists != FALSE;
}

static IDWriteTextFormat* MakeFormat(const wchar_t* fam, float size, DWRITE_FONT_WEIGHT weight,
                                     DWRITE_TEXT_ALIGNMENT align, bool ellipsis) {
    IDWriteTextFormat* f = nullptr;
    g_dw->CreateTextFormat(fam, nullptr, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, L"en-us",
                           &f);
    if (!f) return nullptr;
    f->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    f->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    f->SetTextAlignment(align);
    if (ellipsis) {
        DWRITE_TRIMMING trim{DWRITE_TRIMMING_GRANULARITY_CHARACTER, 0, 0};
        IDWriteInlineObject* sign = nullptr;
        if (SUCCEEDED(g_dw->CreateEllipsisTrimmingSign(f, &sign))) {
            f->SetTrimming(&trim, sign);
            sign->Release();
        }
    }
    return f;
}

static void CreateTextResources() {
    const wchar_t* fam = FontExists(L"Segoe UI Variable Text") ? L"Segoe UI Variable Text" : L"Segoe UI";
    const wchar_t* icons = FontExists(L"Segoe Fluent Icons") ? L"Segoe Fluent Icons" : L"Segoe MDL2 Assets";
    g_fmtText = MakeFormat(fam, Lay::FontSize, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING, false);
    g_fmtHint = MakeFormat(fam, Lay::HintSize, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING, true);
    g_fmtRow = MakeFormat(fam, 14.f, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING, true);
    g_fmtRowRight = MakeFormat(fam, 13.f, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING, false);
    g_fmtGlyph = MakeFormat(icons, 16.f, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, false);
    g_fmtHeader = MakeFormat(fam, 12.f, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, false);
    g_fmtCard = MakeFormat(fam, 12.5f, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER, true);
    wchar_t lang[16] = {};
    DWORD sz = sizeof(lang);
    if (RegGetValueW(HKEY_CURRENT_USER, kSettingsKey, L"Language", RRF_RT_REG_SZ, nullptr, lang, &sz) == ERROR_SUCCESS)
        for (auto& p : kPlaceholders)
            if (wcsncmp(lang, p.code, 2) == 0) g_placeholderText = p.text;
    g_dw->CreateTextLayout(g_placeholderText.c_str(), (UINT32)g_placeholderText.size(), g_fmtText, 10000.f, Lay::BarH,
                           &g_placeholderLayout);
}

static ID2D1LinearGradientBrush* MakeGradient(D2D1_POINT_2F a, D2D1_POINT_2F b, D2D1_COLOR_F ca, D2D1_COLOR_F cb) {
    D2D1_GRADIENT_STOP stops[2] = {{0.f, ca}, {1.f, cb}};
    ID2D1GradientStopCollection* coll = nullptr;
    ID2D1LinearGradientBrush* br = nullptr;
    if (SUCCEEDED(g_rt->CreateGradientStopCollection(stops, 2, &coll))) {
        g_rt->CreateLinearGradientBrush(D2D1::LinearGradientBrushProperties(a, b), coll, &br);
        coll->Release();
    }
    return br;
}

static void DiscardDeviceResources() {
    SafeRelease(g_solid);
    SafeRelease(g_logoBrush);
    SafeRelease(g_shadowBmp);
    SafeRelease(g_bdBrush);
    SafeRelease(g_bdBmp);
    g_bdDirty = !g_bdPx.empty();
    g_shadowDpi = 0;
    {
        std::lock_guard<std::mutex> lk(g_iconMx);
        for (auto& kv : g_icons) SafeRelease(kv.second.bmp);
    }
    g_rt = nullptr;
    SafeRelease(g_dcrt);
    if (g_ctx) g_ctx->SetTarget(nullptr);
    SafeRelease(g_target);
    SafeRelease(g_ctx);
    SafeRelease(g_d2dDev);
    SafeRelease(g_dcVisual);
    SafeRelease(g_dcTarget);
    SafeRelease(g_dcomp);
    SafeRelease(g_swap);
    SafeRelease(g_d3d);
}

static ID3D11Device* CreateD3DDevice() {
    ID3D11Device* dev = nullptr;
    for (D3D_DRIVER_TYPE t : {D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP})
        if (SUCCEEDED(D3D11CreateDevice(nullptr, t, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT, nullptr, 0,
                                        D3D11_SDK_VERSION, &dev, nullptr, nullptr)))
            return dev;
    return nullptr;
}

static bool BindSwapTarget() {
    IDXGISurface* surf = nullptr;
    if (FAILED(g_swap->GetBuffer(0, IID_PPV_ARGS(&surf)))) return false;
    D2D1_BITMAP_PROPERTIES1 bp = D2D1::BitmapProperties1(
        D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED), 96.f, 96.f);
    HRESULT hr = g_ctx->CreateBitmapFromDxgiSurface(surf, &bp, &g_target);
    surf->Release();
    if (FAILED(hr)) return false;
    g_ctx->SetTarget(g_target);
    g_ctx->SetDpi(g_dpi, g_dpi);
    return true;
}

static bool CreateGpuTarget() {
    g_d3d = CreateD3DDevice();
    if (!g_d3d) return false;
    IDXGIDevice1* dxgi = nullptr;
    IDXGIAdapter* adapter = nullptr;
    IDXGIFactory2* factory = nullptr;
    bool ok = SUCCEEDED(g_d3d->QueryInterface(IID_PPV_ARGS(&dxgi)));
    if (ok) {
        dxgi->SetMaximumFrameLatency(1);  // Present(1) then paces us at the display's refresh rate
        ok = SUCCEEDED(g_d2d->CreateDevice(dxgi, &g_d2dDev)) &&
             SUCCEEDED(g_d2dDev->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, &g_ctx)) &&
             SUCCEEDED(dxgi->GetAdapter(&adapter)) && SUCCEEDED(adapter->GetParent(IID_PPV_ARGS(&factory)));
    }
    if (ok) {
        DXGI_SWAP_CHAIN_DESC1 d = {};
        d.Width = g_pxW;
        d.Height = g_pxH;
        d.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        d.SampleDesc.Count = 1;
        d.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
        d.BufferCount = 2;
        d.Scaling = DXGI_SCALING_STRETCH;
        d.SwapEffect = DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL;
        d.AlphaMode = DXGI_ALPHA_MODE_PREMULTIPLIED;
        ok = SUCCEEDED(factory->CreateSwapChainForComposition(g_d3d, &d, nullptr, &g_swap)) &&
             SUCCEEDED(DCompositionCreateDevice(dxgi, IID_PPV_ARGS(&g_dcomp))) &&
             SUCCEEDED(g_dcomp->CreateTargetForHwnd(g_hwnd, TRUE, &g_dcTarget)) &&
             SUCCEEDED(g_dcomp->CreateVisual(&g_dcVisual)) && SUCCEEDED(g_dcVisual->SetContent(g_swap)) &&
             SUCCEEDED(g_dcTarget->SetRoot(g_dcVisual)) && SUCCEEDED(g_dcomp->Commit()) && BindSwapTarget();
    }
    SafeRelease(factory);
    SafeRelease(adapter);
    SafeRelease(dxgi);
    if (!ok) return false;
    g_rt = g_ctx;
    return true;
}

static bool CreateDeviceResources() {
    if (g_rt) return true;
    if (g_hw) {
        if (!CreateGpuTarget()) {
            DiscardDeviceResources();
            return false;
        }
    } else {
        D2D1_RENDER_TARGET_PROPERTIES props = D2D1::RenderTargetProperties(
            D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
        if (FAILED(g_d2d->CreateDCRenderTarget(&props, &g_dcrt))) return false;
        g_rt = g_dcrt;
    }
    g_rt->SetDpi(g_dpi, g_dpi);
    g_rt->SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
    g_rt->CreateSolidColorBrush(ColorF(0, 1.f), &g_solid);
    using namespace Lay;
    float lx = Margin + LogoX, ly = Margin + (BarH - LogoSize) / 2;
    g_logoBrush = MakeGradient({lx, ly}, {lx + LogoSize, ly + LogoSize}, C(0x4CC2FF), C(0x0067C0));
    return g_solid && g_logoBrush;
}

// Gaussian-blurred rounded rectangle, drawn later as a nine-slice so it fits any size.
static void BuildShadow() {
    if (g_shadowBmp && g_shadowDpi == g_dpi) return;
    SafeRelease(g_shadowBmp);
    float s = g_dpi / 96.f;
    float sigma = Lay::ShadowSigma * s;
    int pad = (int)std::ceil(sigma * 3);
    float rr = Lay::Radius * s;
    int c = pad + (int)std::ceil(rr) + 2;
    int N = 2 * c + 1;
    std::vector<float> a((size_t)N * N), tmp((size_t)N * N);
    float half = (N - 2.f * pad) / 2.f, ctr = N / 2.f;
    for (int y = 0; y < N; y++)
        for (int x = 0; x < N; x++) {
            float qx = std::fabs(x + 0.5f - ctr) - (half - rr), qy = std::fabs(y + 0.5f - ctr) - (half - rr);
            float d = std::hypot(std::max(qx, 0.f), std::max(qy, 0.f)) + std::min(std::max(qx, qy), 0.f) - rr;
            a[(size_t)y * N + x] = std::clamp(0.5f - d, 0.f, 1.f);
        }
    int kr = pad;
    std::vector<float> k(2 * kr + 1);
    float ks = 0;
    for (int i = -kr; i <= kr; i++) ks += k[i + kr] = std::exp(-(i * i) / (2 * sigma * sigma));
    for (float& v : k) v /= ks;
    for (int y = 0; y < N; y++)
        for (int x = 0; x < N; x++) {
            float acc = 0;
            for (int i = -kr; i <= kr; i++) {
                int xx = x + i;
                if (xx >= 0 && xx < N) acc += a[(size_t)y * N + xx] * k[i + kr];
            }
            tmp[(size_t)y * N + x] = acc;
        }
    std::vector<UINT32> px((size_t)N * N);
    for (int y = 0; y < N; y++)
        for (int x = 0; x < N; x++) {
            float acc = 0;
            for (int i = -kr; i <= kr; i++) {
                int yy = y + i;
                if (yy >= 0 && yy < N) acc += tmp[(size_t)yy * N + x] * k[i + kr];
            }
            px[(size_t)y * N + x] = UINT32(std::clamp(acc, 0.f, 1.f) * 255.f + 0.5f) << 24;
        }
    D2D1_BITMAP_PROPERTIES bp = D2D1::BitmapProperties(
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED), 96.f, 96.f);
    g_rt->CreateBitmap(D2D1::SizeU(N, N), px.data(), N * 4, bp, &g_shadowBmp);
    g_shadowC = c;
    g_shadowN = N;
    g_shadowPad = pad;
    g_shadowDpi = g_dpi;
}

static void EnsureSurface(int w, int h) {
    if (g_hw) {
        if (w == g_pxW && h == g_pxH) return;
        g_pxW = w;
        g_pxH = h;
        if (g_swap) {
            g_ctx->SetTarget(nullptr);
            SafeRelease(g_target);
            if (FAILED(g_swap->ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, 0)) || !BindSwapTarget())
                DiscardDeviceResources();
        }
        return;
    }
    if (g_dib && w == g_pxW && h == g_pxH) return;
    if (g_dib) {
        SelectObject(g_memDC, g_oldBmp);
        DeleteObject(g_dib);
        g_dib = nullptr;
    }
    BITMAPINFO bi = {};
    bi.bmiHeader.biSize = sizeof(bi.bmiHeader);
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    g_dib = CreateDIBSection(g_memDC, &bi, DIB_RGB_COLORS, &g_bits, nullptr, 0);
    g_oldBmp = (HBITMAP)SelectObject(g_memDC, g_dib);
    g_pxW = w;
    g_pxH = h;
}

static void PositionOnMonitor() {
    POINT pt;
    GetCursorPos(&pt);
    HMONITOR mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
    MONITORINFO mi = {sizeof(mi)};
    GetMonitorInfoW(mon, &mi);
    UINT dx = 96, dy = 96;
    if (FAILED(GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &dx, &dy))) dx = 96;
    g_dpi = (float)dx;
    float s = g_dpi / 96.f;
    int w = (int)std::ceil(Lay::WinW * s), h = (int)std::ceil(Lay::WinMaxH * s);
    EnsureSurface(w, h);
    if (g_rt) g_rt->SetDpi(g_dpi, g_dpi);
    g_iconPx = (int)std::lround(24 * s);

    // Centered horizontally, sitting a little above the vertical middle of the screen.
    const RECT& wa = mi.rcWork;
    int barCenterY = wa.top + (int)std::lround((wa.bottom - wa.top) * Lay::VerticalPos);
    g_winPos.x = (wa.left + wa.right) / 2 - w / 2;
    g_winPos.y = barCenterY - (int)std::lround((Lay::Margin + Lay::BarH / 2) * s);
    g_winH = -1;  // hardware path: move/resize the window on the next frame
}

// Snapshot the desktop behind the (hidden) window and blur it: the frosted backdrop.
static void BoxBlur(std::vector<float>& ch, std::vector<float>& tmp, int w, int h, int r) {
    const float inv = 1.f / (2 * r + 1);
    for (int y = 0; y < h; y++) {
        const float* src = &ch[(size_t)y * w];
        float* dst = &tmp[(size_t)y * w];
        float acc = 0;
        for (int i = -r; i <= r; i++) acc += src[std::clamp(i, 0, w - 1)];
        for (int x = 0; x < w; x++) {
            dst[x] = acc * inv;
            acc += src[std::min(x + r + 1, w - 1)] - src[std::max(x - r, 0)];
        }
    }
    for (int x = 0; x < w; x++) {
        float acc = 0;
        for (int i = -r; i <= r; i++) acc += tmp[(size_t)std::clamp(i, 0, h - 1) * w + x];
        for (int y = 0; y < h; y++) {
            ch[(size_t)y * w + x] = acc * inv;
            acc += tmp[(size_t)std::min(y + r + 1, h - 1) * w + x] - tmp[(size_t)std::max(y - r, 0) * w + x];
        }
    }
}

static void CaptureBackdrop() {
    int w = g_pxW, h = g_pxH;
    HDC scr = GetDC(nullptr);
    HDC mem = CreateCompatibleDC(scr);
    BITMAPINFO bi = {};
    bi.bmiHeader.biSize = sizeof(bi.bmiHeader);
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    HBITMAP bmp = CreateDIBSection(mem, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
    if (!bmp) { DeleteDC(mem); ReleaseDC(nullptr, scr); return; }
    HGDIOBJ old = SelectObject(mem, bmp);
    BitBlt(mem, 0, 0, w, h, scr, g_winPos.x, g_winPos.y, SRCCOPY);
    GdiFlush();

    const int F = kBackdropScale;
    int bw = (w + F - 1) / F, bh = (h + F - 1) / F;
    std::vector<float> ch[3], tmp((size_t)bw * bh);
    for (auto& c : ch) c.assign((size_t)bw * bh, 0.f);
    const UINT32* src = static_cast<const UINT32*>(bits);
    for (int by = 0; by < bh; by++)
        for (int bx = 0; bx < bw; bx++) {
            float r = 0, g = 0, b = 0;
            int n = 0;
            for (int y = by * F; y < std::min(h, by * F + F); y++)
                for (int x = bx * F; x < std::min(w, bx * F + F); x++) {
                    UINT32 p = src[(size_t)y * w + x];
                    r += (p >> 16) & 255;
                    g += (p >> 8) & 255;
                    b += p & 255;
                    n++;
                }
            size_t i = (size_t)by * bw + bx;
            ch[0][i] = r / n;
            ch[1][i] = g / n;
            ch[2][i] = b / n;
        }
    SelectObject(mem, old);
    DeleteObject(bmp);
    DeleteDC(mem);
    ReleaseDC(nullptr, scr);

    for (auto& c : ch)
        for (int pass = 0; pass < 3; pass++) BoxBlur(c, tmp, bw, bh, 5);

    g_bdPx.resize((size_t)bw * bh);
    for (size_t i = 0; i < g_bdPx.size(); i++) {
        float r = ch[0][i], g = ch[1][i], b = ch[2][i];
        float l = 0.2126f * r + 0.7152f * g + 0.0722f * b;
        const float sat = 1.35f;  // acrylic-style saturation boost
        r = std::clamp(l + (r - l) * sat, 0.f, 255.f);
        g = std::clamp(l + (g - l) * sat, 0.f, 255.f);
        b = std::clamp(l + (b - l) * sat, 0.f, 255.f);
        g_bdPx[i] = 0xFF000000u | (UINT32(r) << 16) | (UINT32(g) << 8) | UINT32(b);
    }
    g_bdW = bw;
    g_bdH = bh;
    g_bdDirty = true;
}

static void EnsureBackdropBrush() {
    if (!g_bdDirty || g_bdPx.empty() || !g_rt) return;
    SafeRelease(g_bdBrush);
    SafeRelease(g_bdBmp);
    float bdpi = g_dpi / kBackdropScale;  // makes the bitmap's DIP size equal the window's
    D2D1_BITMAP_PROPERTIES bp = D2D1::BitmapProperties(
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED), bdpi, bdpi);
    if (SUCCEEDED(g_rt->CreateBitmap(D2D1::SizeU(g_bdW, g_bdH), g_bdPx.data(), g_bdW * 4, bp, &g_bdBmp)))
        g_rt->CreateBitmapBrush(g_bdBmp,
                                D2D1::BitmapBrushProperties(D2D1_EXTEND_MODE_CLAMP, D2D1_EXTEND_MODE_CLAMP,
                                                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR),
                                &g_bdBrush);
    g_bdDirty = false;
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------
static float CaretXFor(IDWriteTextLayout* lay, int pos, int len) {
    if (len == 0) return 0.f;
    DWRITE_HIT_TEST_METRICS m;
    float x = 0, y = 0;
    if (pos >= len) lay->HitTestTextPosition(len - 1, TRUE, &x, &y, &m);
    else lay->HitTestTextPosition(pos, FALSE, &x, &y, &m);
    return x;
}

static void DrawShadow(const D2D1_RECT_F& box, float opacity) {
    if (!g_shadowBmp || opacity <= 0.001f) return;
    float s = g_dpi / 96.f;
    float pad = g_shadowPad / s, cd = g_shadowC / s;
    D2D1_RECT_F o = {box.left - pad, box.top - pad + Lay::ShadowOffsetY, box.right + pad,
                     box.bottom + pad + Lay::ShadowOffsetY};
    float dx[4] = {o.left, o.left + cd, o.right - cd, o.right};
    float dy[4] = {o.top, o.top + cd, o.bottom - cd, o.bottom};
    float sx[4] = {0, (float)g_shadowC, (float)g_shadowC + 1, (float)g_shadowN};
    g_rt->SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);  // avoid seams between slices
    for (int j = 0; j < 3; j++)
        for (int i = 0; i < 3; i++) {
            if (dx[i + 1] <= dx[i] || dy[j + 1] <= dy[j]) continue;
            D2D1_RECT_F d = {dx[i], dy[j], dx[i + 1], dy[j + 1]};
            D2D1_RECT_F src = {sx[i], sx[j], sx[i + 1], sx[j + 1]};
            g_rt->DrawBitmap(g_shadowBmp, d, opacity, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, src);
        }
    g_rt->SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
}

// Fills with the blurred desktop (kept locked to the screen regardless of the current transform),
// then the theme tint on top.
static void FillAcrylic(const D2D1_ROUNDED_RECT& rr, const Theme& t, float gradTop, float gradBottom) {
    if (g_bdBrush) {
        D2D1_MATRIX_3X2_F cur;
        g_rt->GetTransform(&cur);
        Matrix3x2F inv = Matrix3x2F::ReinterpretBaseType(&cur)[0];
        if (inv.Invert()) g_bdBrush->SetTransform(inv);
        g_rt->FillRoundedRectangle(rr, g_bdBrush);
    } else {
        g_solid->SetColor(ColorF(0x202020));
        g_rt->FillRoundedRectangle(rr, g_solid);
    }
    if (ID2D1LinearGradientBrush* g = MakeGradient({0, gradTop}, {0, gradBottom}, t.tintTop, t.tintBottom)) {
        g_rt->FillRoundedRectangle(rr, g);
        g->Release();
    }
}

static void DrawLogoAt(float lx, float ly, float size, ID2D1Brush* brush, const Matrix3x2F& base, double now,
                       bool animate) {
    float gap = size * 0.09f;
    float q = (size - gap) / 2;
    for (int i = 0; i < 4; i++) {
        float x0 = lx + (i % 2) * (q + gap), y0 = ly + (i / 2) * (q + gap);
        float k = 1.f;
        if (animate) {
            double t = (now - g_showTime - 0.04 - i * 0.045) / 0.42;
            k = (float)EaseOutBack(std::clamp(t, 0.0, 1.0));
        }
        float sk = 0.3f + 0.7f * k;
        D2D1_POINT_2F c = {x0 + q / 2, y0 + q / 2};
        g_rt->SetTransform(Matrix3x2F::Scale(sk, sk, c) * base);
        brush->SetOpacity(Clamp01(k * 1.6));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x0, y0, x0 + q, y0 + q), size * 0.08f, size * 0.08f),
                                   brush);
    }
    brush->SetOpacity(1.f);
    g_rt->SetTransform(base);
}

static void DrawTextArea(double now, float dt) {
    using namespace Lay;
    const Theme& th = g_th;
    const float ox = Margin + TextX;
    const float rightEdge = Margin + BarW - RightPad;
    const float cy = Margin + BarH / 2;

    // Right-hand hint: shows which app Enter will open when it isn't an inline completion.
    float hintW = 0;
    if (!g_hintName.empty()) {
        std::wstring hint = g_hintName + L"  ↵";
        IDWriteTextLayout* hl = nullptr;
        if (SUCCEEDED(g_dw->CreateTextLayout(hint.c_str(), (UINT32)hint.size(), g_fmtHint, HintMaxW, BarH, &hl))) {
            DWRITE_TEXT_METRICS tm;
            hl->GetMetrics(&tm);
            hintW = std::min(tm.widthIncludingTrailingWhitespace, HintMaxW);
            float o = Clamp01((now - g_hintTime) / 0.16);
            g_solid->SetColor(WithA(th.dim, (float)EaseOutCubic(o)));
            g_rt->DrawTextLayout({rightEdge - HintMaxW + (1 - o) * 6.f, Margin}, hl, g_solid);
            hl->Release();
        }
    }
    const float avail = rightEdge - ox - (hintW > 0 ? hintW + 20.f : 0.f);

    const bool hasSel = g_anchor != g_caret;
    const int len = (int)g_text.size();
    std::wstring ghost = (!hasSel && g_caret == len) ? g_ghost : L"";
    std::wstring full = g_text + ghost;

    IDWriteTextLayout* lay = nullptr;
    if (FAILED(g_dw->CreateTextLayout(full.c_str(), (UINT32)full.size(), g_fmtText, 100000.f, BarH, &lay))) return;

    // Per-character fade-in for freshly typed text (grouped by insertion time).
    std::vector<ID2D1SolidColorBrush*> temp;
    const double kFade = 0.14;
    for (int i = 0; i < len;) {
        double age = now - g_birth[i];
        if (age >= kFade) { i++; continue; }
        int j = i + 1;
        while (j < len && g_birth[j] == g_birth[i]) j++;
        float o = 0.15f + 0.85f * (float)EaseOutCubic(age / kFade);
        ID2D1SolidColorBrush* b = nullptr;
        if (SUCCEEDED(g_rt->CreateSolidColorBrush(WithA(th.text, o), &b))) {
            lay->SetDrawingEffect(b, {(UINT32)i, (UINT32)(j - i)});
            temp.push_back(b);
        }
        i = j;
    }
    if (!ghost.empty()) {
        ID2D1SolidColorBrush* b = nullptr;
        float o = (float)EaseOutCubic(Clamp01((now - g_lastInput) / 0.12));
        if (SUCCEEDED(g_rt->CreateSolidColorBrush(WithA(th.ghost, o), &b))) {
            lay->SetDrawingEffect(b, {(UINT32)len, (UINT32)ghost.size()});
            temp.push_back(b);
        }
    }

    // Caret target, smoothing, and horizontal scrolling.
    float target = CaretXFor(lay, g_caret, (int)full.size());
    if (!g_caretInit) { g_caretX = g_trailX = target; g_caretInit = true; }
    g_caretX = Damp(g_caretX, target, 32.f, dt);
    g_trailX = Damp(g_trailX, target, 13.f, dt);

    DWRITE_TEXT_METRICS tm;
    lay->GetMetrics(&tm);
    float st = g_scrollTarget;
    if (target - st > avail - 4) st = target - avail + 4;
    if (target - st < 0) st = target;
    st = std::clamp(st, 0.f, std::max(0.f, tm.widthIncludingTrailingWhitespace - avail + 4));
    g_scrollTarget = st;
    g_scroll = Damp(g_scroll, st, 22.f, dt);

    g_placeholderA = Damp(g_placeholderA, g_text.empty() ? 1.f : 0.f, 18.f, dt);

    g_rt->PushAxisAlignedClip(D2D1::RectF(ox - 4, Margin, ox + avail + 4, Margin + BarH),
                              D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    const float tx = ox - g_scroll;

    if (hasSel) {
        float a = CaretXFor(lay, std::min(g_anchor, g_caret), (int)full.size());
        float b = CaretXFor(lay, std::max(g_anchor, g_caret), (int)full.size());
        g_solid->SetColor(WithA(th.accent, 0.30f));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(tx + a, cy - 12, tx + b, cy + 12), 3, 3), g_solid);
    }

    if (g_placeholderA > 0.01f && g_placeholderLayout) {
        g_solid->SetColor(WithA(th.dim, g_placeholderA));
        g_rt->DrawTextLayout({ox + (1 - g_placeholderA) * 10.f, Margin}, g_placeholderLayout, g_solid);
    }

    g_solid->SetColor(th.text);
    g_rt->DrawTextLayout({tx, Margin}, lay, g_solid);

    // Caret: smooth glide + soft trailing smear + eased blink + small "pop" on input.
    double idle = now - g_lastInput;
    float blink = idle < 0.5 ? 1.f : (float)(0.5 + 0.5 * std::cos((idle - 0.5) * 2 * 3.14159265 / 1.05));
    float intro = Clamp01((now - g_showTime - 0.10) / 0.22);
    float op = blink * (float)EaseOutCubic(intro);
    float h = CaretH * (1.f + 0.14f * (float)std::exp(-idle * 16.0));
    if (op > 0.003f) {
        float x0 = tx + std::min(g_caretX, g_trailX) - CaretW / 2;
        float x1 = tx + std::max(g_caretX, g_trailX) + CaretW / 2;
        if (x1 - x0 > CaretW + 0.6f) {
            g_solid->SetColor(WithA(th.accent, op * 0.28f));
            g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x0, cy - h * 0.38f, x1, cy + h * 0.38f), 1, 1),
                                       g_solid);
        }
        g_solid->SetColor(WithA(th.accent, op));
        g_rt->FillRoundedRectangle(
            D2D1::RoundedRect(D2D1::RectF(tx + g_caretX - CaretW / 2, cy - h / 2, tx + g_caretX + CaretW / 2, cy + h / 2),
                              1, 1),
            g_solid);
    }

    g_rt->PopAxisAlignedClip();
    lay->Release();
    for (auto* b : temp) b->Release();
}

static void DrawLayout(const std::wstring& s, IDWriteTextFormat* fmt, float x, float y, float w, float h,
                       D2D1_COLOR_F col, IDWriteTextLayout** keep = nullptr) {
    IDWriteTextLayout* l = nullptr;
    if (FAILED(g_dw->CreateTextLayout(s.c_str(), (UINT32)s.size(), fmt, w, h, &l))) return;
    g_solid->SetColor(col);
    g_rt->DrawTextLayout({x, y}, l, g_solid);
    if (keep) *keep = l;
    else l->Release();
}

// List rows (results / commands) with a sliding selection highlight.
static void DrawRows(float top, float dt, float alpha) {
    using namespace Lay;
    const Theme& th = g_th;
    int n = (g_drawMode == Mode::Results) ? (int)g_results.size() : (int)g_cmdList.size();
    if (n == 0) return;
    int sel = std::clamp(g_sel, 0, n - 1);
    float selTarget = top + PanelPad + sel * RowH;
    if (!g_selYInit) { g_selY = selTarget; g_selYInit = true; }
    g_selY = Damp(g_selY, selTarget, 26.f, dt);

    // highlight + accent pill
    g_solid->SetColor(WithA(th.highlight, alpha));
    g_rt->FillRoundedRectangle(
        D2D1::RoundedRect(D2D1::RectF(Margin + 6, g_selY + 2, Margin + BarW - 6, g_selY + RowH - 2), 8, 8), g_solid);
    g_solid->SetColor(WithA(th.accent, alpha));
    g_rt->FillRoundedRectangle(
        D2D1::RoundedRect(D2D1::RectF(Margin + 8, g_selY + RowH / 2 - 8, Margin + 11, g_selY + RowH / 2 + 8), 1.5f,
                          1.5f),
        g_solid);

    std::wstring q = Lower(Trim(g_text));
    double now = Now();
    for (int i = 0; i < n; i++) {
        float slide = (1.f - g_contentA) * (6.f + i * 3.f);
        float y = top + PanelPad + i * RowH + slide;
        float rowA = alpha * Clamp01(g_contentA * 1.4 - i * 0.06);
        g_hits.push_back({D2D1::RectF(Margin + 6, y, Margin + BarW - 6, y + RowH), i});
        bool isSel = i == sel;
        float ix = Margin + 20, iy = y + (RowH - 24) / 2;

        if (g_drawMode == Mode::Results) {
            const App& a = g_results[i];
            double shownAt = 0;
            ID2D1Bitmap* icon = GetIcon(a.target, &shownAt);
            float io = icon ? (float)EaseOutCubic(Clamp01((now - shownAt) / 0.18)) : 0.f;
            if (io < 1.f) {
                g_solid->SetColor(WithA(th.highlight, rowA * (1 - io) * 1.5f));
                g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(ix, iy, ix + 24, iy + 24), 6, 6), g_solid);
            }
            if (icon) g_rt->DrawBitmap(icon, D2D1::RectF(ix, iy, ix + 24, iy + 24), rowA * io);

            IDWriteTextLayout* l = nullptr;
            if (SUCCEEDED(g_dw->CreateTextLayout(a.name.c_str(), (UINT32)a.name.size(), g_fmtRow, BarW - 120, RowH,
                                                 &l))) {
                size_t p = q.empty() ? std::wstring::npos : a.lower.find(q);
                if (p != std::wstring::npos)
                    l->SetFontWeight(DWRITE_FONT_WEIGHT_SEMI_BOLD, {(UINT32)p, (UINT32)q.size()});
                g_solid->SetColor(WithA(isSel ? th.text : LerpC(th.dim, th.text, 0.55f), rowA));
                g_rt->DrawTextLayout({Margin + 56, y}, l, g_solid);
                l->Release();
            }
            if (isSel) DrawLayout(L"Open  ↵", g_fmtRowRight, Margin + BarW - 140, y, 120, RowH, WithA(th.dim, rowA));
        } else {
            int c = g_cmdList[i];
            DrawLayout(kCmds[c].glyph, g_fmtGlyph, ix, y, 24, RowH, WithA(isSel ? th.accent : th.dim, rowA));
            DrawLayout(kCmds[c].title, g_fmtRow, Margin + 56, y, BarW - 200, RowH,
                       WithA(isSel ? th.text : LerpC(th.dim, th.text, 0.55f), rowA));
            std::wstring d = CommandDetail(c);
            bool on = d == L"On";
            DrawLayout(d, g_fmtRowRight, Margin + BarW - 180, y, 160, RowH, WithA(on ? th.accent : th.dim, rowA));
        }
    }
}

// Theme picker: a grid of live preview cards.
static void DrawThemeGrid(float top, float dt, float alpha, double now) {
    using namespace Lay;
    const Theme& th = g_th;
    DrawLayout(L"Themes", g_fmtHeader, Margin + GridPad + 2, top + GridPad - 4, 200, GridHeaderH,
               WithA(th.dim, alpha));
    DrawLayout(L"← → preview   ↵ apply", g_fmtRowRight, Margin + BarW - GridPad - 260,
               top + GridPad - 4, 258, GridHeaderH, WithA(th.dim, alpha * 0.85f));

    D2D1_MATRIX_3X2_F baseRaw;
    g_rt->GetTransform(&baseRaw);
    Matrix3x2F base = Matrix3x2F::ReinterpretBaseType(&baseRaw)[0];

    float cw = (BarW - 2 * GridPad - (GridCols - 1) * CardGap) / GridCols;
    float y0 = top + GridPad + GridHeaderH - 4;
    for (int i = 0; i < kThemeCount; i++) {
        int col = i % GridCols, row = i / GridCols;
        float x = Margin + GridPad + col * (cw + CardGap);
        float slide = (1.f - g_contentA) * (8.f + row * 6.f);
        float y = y0 + row * (CardPreviewH + CardNameH + 4.f) + slide;
        float cardA = alpha * Clamp01(g_contentA * 1.3 - (row * GridCols + col) * 0.04);
        g_cardSel[i] = Damp(g_cardSel[i], i == g_sel ? 1.f : 0.f, 16.f, dt);
        float sk = 1.f + 0.035f * g_cardSel[i];
        D2D1_RECT_F card = D2D1::RectF(x, y, x + cw, y + CardPreviewH);
        g_hits.push_back({D2D1::RectF(x, y, x + cw, y + CardPreviewH + CardNameH), i});

        D2D1_POINT_2F cc = {x + cw / 2, y + CardPreviewH / 2};
        Matrix3x2F m = Matrix3x2F::Scale(sk, sk, cc) * base;
        g_rt->SetTransform(m);

        // Preview: the theme's real material on the real blurred desktop.
        const Theme& t = kThemes[i];
        D2D1_ROUNDED_RECT rr = D2D1::RoundedRect(card, 9, 9);
        g_rt->PushLayer(D2D1::LayerParameters(D2D1::InfiniteRect(), nullptr, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                                              D2D1::IdentityMatrix(), cardA),
                        nullptr);
        FillAcrylic(rr, t, card.top, card.bottom);
        if (ID2D1LinearGradientBrush* b = MakeGradient({0, card.top}, {0, card.bottom}, t.borderTop, t.borderBottom)) {
            g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + .5f, y + .5f, x + cw - .5f, y + CardPreviewH - .5f), 8.5f, 8.5f), b, 1.f);
            b->Release();
        }
        // mini bar contents: logo, faux text, caret, and a faux results row
        float ly = y + 14;
        if (ID2D1LinearGradientBrush* lb = MakeGradient({x + 12, ly}, {x + 24, ly + 12}, C(0x4CC2FF), C(0x0067C0))) {
            DrawLogoAt(x + 12, ly, 12, lb, m, now, false);
            lb->Release();
        }
        g_solid->SetColor(WithA(t.text, 0.85f));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + 32, ly + 3.5f, x + 32 + cw * 0.28f, ly + 8.5f), 2.5f, 2.5f), g_solid);
        g_solid->SetColor(t.ghost);
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + 34 + cw * 0.28f, ly + 3.5f, x + 32 + cw * 0.52f, ly + 8.5f), 2.5f, 2.5f), g_solid);
        g_solid->SetColor(t.accent);
        g_rt->FillRectangle(D2D1::RectF(x + 32 + cw * 0.28f, ly - 1, x + 33.5f + cw * 0.28f, ly + 13), g_solid);
        g_solid->SetColor(t.divider);
        g_rt->FillRectangle(D2D1::RectF(x + 1, y + 34, x + cw - 1, y + 35), g_solid);
        g_solid->SetColor(t.highlight);
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + 6, y + 41, x + cw - 6, y + 61), 5, 5), g_solid);
        g_solid->SetColor(t.accent);
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + 8, y + 46, x + 10, y + 56), 1, 1), g_solid);
        g_solid->SetColor(WithA(t.dim, 0.9f));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + 16, y + 48.5f, x + 16 + cw * 0.38f, y + 53.5f), 2.5f, 2.5f), g_solid);
        g_rt->PopLayer();

        // selection ring
        if (g_cardSel[i] > 0.01f) {
            g_solid->SetColor(WithA(th.accent, cardA * g_cardSel[i]));
            g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x - 3, y - 3, x + cw + 3, y + CardPreviewH + 3), 12, 12),
                                       g_solid, 2.f);
        }
        // active-theme badge
        if (i == g_themeIdx) {
            D2D1_ELLIPSE e = D2D1::Ellipse({x + cw - 13, y + 13}, 8, 8);
            g_solid->SetColor(WithA(th.accent, cardA));
            g_rt->FillEllipse(e, g_solid);
            DrawLayout(L"", g_fmtGlyph, x + cw - 21, y + 5, 16, 16, WithA(C(0xFFFFFF), cardA));
        }
        g_rt->SetTransform(base);

        // name
        D2D1_COLOR_F nc = LerpC(th.dim, th.text, g_cardSel[i]);
        DrawLayout(t.name, g_fmtCard, x, y + CardPreviewH + 2, cw, CardNameH - 4, WithA(nc, cardA));
    }
}

static void DrawPanel(double now, float dt) {
    using namespace Lay;
    if (g_panelH < 0.5f || g_drawMode == Mode::None) return;
    const Theme& th = g_th;
    float top = Margin + BarH;
    float alpha = (float)EaseOutCubic(g_contentA);

    // divider between the bar and the panel
    g_solid->SetColor(WithA(th.divider, Clamp01(g_panelH / 20.f)));
    g_rt->FillRectangle(D2D1::RectF(Margin + 1, top, Margin + BarW - 1, top + 1), g_solid);

    g_rt->PushAxisAlignedClip(D2D1::RectF(Margin, top + 1, Margin + BarW, top + g_panelH),
                              D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    if (g_drawMode == Mode::Themes) DrawThemeGrid(top, dt, alpha, now);
    else DrawRows(top, dt, alpha);
    g_rt->PopAxisAlignedClip();
}

static void Render() {
    if (!g_pxW || !CreateDeviceResources()) return;
    double now = Now();
    float dt = (float)std::clamp(now - g_lastFrame, 0.0, 0.05);
    g_lastFrame = now;

    double s = std::clamp((now - g_animStart) / g_animDur, 0.0, 1.0);
    double e = g_animTo > 0.5 ? EaseOutQuart(s) : EaseOutCubic(s);
    g_anim = g_animFrom + (g_animTo - g_animFrom) * e;
    if (s >= 1.0 && g_animTo < 0.5) {
        g_anim = 0;
        g_visible = false;
        SetBarOpen(false);
        ShowWindow(g_hwnd, SW_HIDE);
        g_previewIdx = g_themeIdx;
        g_th = g_thFrom = kThemes[g_themeIdx];
        return;
    }

    // theme cross-fade
    g_th = LerpTheme(g_thFrom, kThemes[g_previewIdx], (float)EaseOutCubic(Clamp01((now - g_thStart) / 0.35)));

    // panel expansion
    float target = PanelTargetH();
    g_panelH = Damp(g_panelH, target, 20.f, dt);
    if (std::fabs(g_panelH - target) < 0.25f) g_panelH = target;
    if (g_mode != Mode::None) g_contentA = Damp(g_contentA, 1.f, 12.f, dt);
    else if (g_panelH < 0.5f) g_drawMode = Mode::None;
    g_hits.clear();

    BuildShadow();
    EnsureBackdropBrush();

    using namespace Lay;
    if (!g_hw) {
        RECT rc = {0, 0, g_pxW, g_pxH};
        g_dcrt->BindDC(g_memDC, &rc);
    }
    g_rt->BeginDraw();
    g_rt->SetTransform(Matrix3x2F::Identity());
    g_rt->Clear(ColorF(0, 0.f));
    // The layered window fades through its constant alpha; the swap chain fades through a layer.
    bool fadeLayer = g_hw && g_anim < 0.999;
    if (fadeLayer)
        g_rt->PushLayer(D2D1::LayerParameters(D2D1::InfiniteRect(), nullptr, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                                              D2D1::IdentityMatrix(), (float)std::clamp(g_anim, 0.0, 1.0)),
                        nullptr);

    float a = (float)g_anim;
    float sc = 0.94f + 0.06f * a;
    float ty = (1.f - a) * 16.f;
    Matrix3x2F base = Matrix3x2F::Scale(sc, sc, {Margin + BarW / 2, Margin + BarH / 2}) * Matrix3x2F::Translation(0, ty);
    g_rt->SetTransform(base);

    D2D1_RECT_F box = D2D1::RectF(Margin, Margin, Margin + BarW, Margin + BarH + g_panelH);
    D2D1_ROUNDED_RECT rr = D2D1::RoundedRect(box, Radius, Radius);
    DrawShadow(box, g_th.shadow);
    FillAcrylic(rr, g_th, Margin, Margin + BarH);

    DrawPanel(now, dt);

    if (ID2D1LinearGradientBrush* b = MakeGradient({0, Margin}, {0, Margin + BarH}, g_th.borderTop, g_th.borderBottom)) {
        D2D1_RECT_F inner = D2D1::RectF(box.left + .5f, box.top + .5f, box.right - .5f, box.bottom - .5f);
        g_rt->DrawRoundedRectangle(D2D1::RoundedRect(inner, Radius - .5f, Radius - .5f), b, 1.f);
        b->Release();
    }

    DrawLogoAt(Margin + LogoX, Margin + (BarH - LogoSize) / 2, LogoSize, g_logoBrush, base, now, true);
    DrawTextArea(now, dt);

    if (fadeLayer) {
        g_rt->SetTransform(Matrix3x2F::Identity());
        g_rt->PopLayer();
    }
    HRESULT hr = g_rt->EndDraw();
    if (hr == D2DERR_RECREATE_TARGET) {
        DiscardDeviceResources();
        return;
    }

    float scale = g_dpi / 96.f;
    int curH = std::min(g_pxH, (int)std::ceil((Margin * 2 + BarH + g_panelH) * scale));
    PublishBox();
    if (g_hw) {
        // The window follows the panel's height so the empty space below never catches clicks.
        if (curH != g_winH) {
            SetWindowPos(g_hwnd, nullptr, g_winPos.x, g_winPos.y, g_pxW, curH, SWP_NOZORDER | SWP_NOACTIVATE);
            g_winH = curH;
        }
        hr = g_swap->Present(1, 0);
        if (hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET) DiscardDeviceResources();
        return;
    }
    BLENDFUNCTION bf = {AC_SRC_OVER, 0, (BYTE)std::lround(255.0 * std::clamp(g_anim, 0.0, 1.0)), AC_SRC_ALPHA};
    POINT src = {0, 0};
    SIZE sz = {g_pxW, curH};
    UpdateLayeredWindow(g_hwnd, nullptr, &g_winPos, &sz, g_memDC, &src, 0, &bf, ULW_ALPHA);
}

// ---------------------------------------------------------------------------
// Show / hide
// ---------------------------------------------------------------------------
static void StartAnim(double to, double dur) {
    g_animFrom = g_anim;
    g_animTo = to;
    g_animStart = Now();
    g_animDur = dur;
}

static void ForceForeground(HWND h) {
    HWND fg = GetForegroundWindow();
    DWORD me = GetCurrentThreadId();
    DWORD other = fg ? GetWindowThreadProcessId(fg, nullptr) : 0;
    if (other && other != me && AttachThreadInput(me, other, TRUE)) {
        BringWindowToTop(h);
        SetForegroundWindow(h);
        AttachThreadInput(me, other, FALSE);
    } else {
        SetForegroundWindow(h);
    }
    SetFocus(h);
}

static void Show() {
    if (g_visible && g_animTo > 0.5) return;
    if (!g_visible) {
        PositionOnMonitor();
        CaptureBackdrop();
        g_text.clear();
        g_birth.clear();
        g_caret = g_anchor = 0;
        g_caretInit = false;
        g_scroll = g_scrollTarget = 0;
        g_placeholderA = 1;
        g_panelH = 0;
        g_contentA = 0;
        g_mode = g_drawMode = Mode::None;
        g_showTime = Now();
        g_autostart = IsAutoStart();
        g_previewIdx = g_themeIdx;
        g_th = g_thFrom = kThemes[g_themeIdx];
        UpdateState();
    }
    StartAnim(1.0, 0.32);
    g_visible = true;
    PublishBox();
    SetBarOpen(true);
    g_lastFrame = g_lastInput = Now();
    Render();
    SetWindowPos(g_hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
    ForceForeground(g_hwnd);
    RefreshApps(false);
}

static void Hide() {
    if (!g_visible || g_animTo < 0.5) return;
    StartAnim(0.0, 0.18);
    SetBarOpen(false);
    g_lastHideTime = Now();
    if (GetCapture() == g_hwnd) ReleaseCapture();
}

static void Toggle() {
    if (g_visible && g_animTo > 0.5) Hide();
    else Show();
}

// ---------------------------------------------------------------------------
// Running commands
// ---------------------------------------------------------------------------
static std::wstring UrlEncode(const std::wstring& s) {
    int n = WideCharToMultiByte(CP_UTF8, 0, s.c_str(), (int)s.size(), nullptr, 0, nullptr, nullptr);
    std::string u(n, '\0');
    WideCharToMultiByte(CP_UTF8, 0, s.c_str(), (int)s.size(), &u[0], n, nullptr, nullptr);
    std::wstring out;
    const wchar_t* hex = L"0123456789ABCDEF";
    for (unsigned char c : u) {
        if (isalnum(c) || c == '-' || c == '_' || c == '.' || c == '~') out.push_back((wchar_t)c);
        else if (c == ' ') out.push_back(L'+');
        else { out.push_back(L'%'); out.push_back(hex[c >> 4]); out.push_back(hex[c & 15]); }
    }
    return out;
}

static bool LooksLikeUrl(const std::wstring& s) {
    if (s.find(L"://") != std::wstring::npos || s.rfind(L"www.", 0) == 0) return true;
    if (s.find(L' ') != std::wstring::npos) return false;
    size_t dot = s.rfind(L'.');
    if (dot == std::wstring::npos || dot == 0) return false;
    std::wstring tld = s.substr(dot + 1);
    size_t slash = tld.find(L'/');
    if (slash != std::wstring::npos) tld = tld.substr(0, slash);
    if (tld.size() < 2 || tld.size() > 6) return false;
    for (wchar_t c : tld) if (!std::iswalpha(c)) return false;
    return true;
}

static void SplitCommand(const std::wstring& q, std::wstring& file, std::wstring& params) {
    if (GetFileAttributesW(q.c_str()) != INVALID_FILE_ATTRIBUTES) { file = q; params.clear(); return; }
    size_t i = 0;
    if (q[0] == L'"') {
        size_t e = q.find(L'"', 1);
        if (e == std::wstring::npos) e = q.size();
        file = q.substr(1, e - 1);
        i = std::min(e + 1, q.size());
    } else {
        size_t e = q.find(L' ');
        if (e == std::wstring::npos) e = q.size();
        file = q.substr(0, e);
        i = e;
    }
    params = Trim(q.substr(i));
}

static void Launch(const App* app, const std::wstring& q, bool admin) {
    bool isApp = app != nullptr;
    std::wstring file, params;
    if (isApp) file = L"shell:AppsFolder\\" + app->target;
    else SplitCommand(q, file, params);
    std::wstring verb = admin ? L"runas" : L"open";

    AllowSetForegroundWindow(ASFW_ANY);
    Hide();
    std::thread([file, params, verb, isApp, q]() {
        CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        SHELLEXECUTEINFOW sei = {sizeof(sei)};
        sei.fMask = SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
        sei.lpVerb = verb.c_str();
        sei.lpFile = file.c_str();
        sei.lpParameters = params.empty() ? nullptr : params.c_str();
        sei.nShow = SW_SHOWNORMAL;
        if (!ShellExecuteExW(&sei) && !isApp && GetLastError() != ERROR_CANCELLED) {
            std::wstring url;
            if (LooksLikeUrl(q)) url = (q.find(L"://") == std::wstring::npos ? L"https://" : L"") + q;
            else url = L"https://www.bing.com/search?q=" + UrlEncode(q);
            ShellExecuteW(nullptr, L"open", url.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
        }
        CoUninitialize();
    }).detach();
}

static void SetText(const std::wstring& s);

static void RunCommand(int c) {
    switch (c) {
    case CMD_THEME: SetText(L"> theme"); break;
    case CMD_EXPANDED:
        g_expanded = !g_expanded;
        WriteDword(L"Expanded", g_expanded ? 1 : 0);
        break;
    case CMD_STARTUP: SetAutoStart(!g_autostart); break;
    case CMD_QUIT: DestroyWindow(g_hwnd); break;
    }
}

// Enter / click on the current selection.
static void Activate(bool forceRaw, bool admin) {
    std::wstring q = Trim(g_text);
    switch (g_mode) {
    case Mode::Themes: ApplyTheme(g_sel); return;
    case Mode::Commands:
        if (g_sel >= 0 && g_sel < (int)g_cmdList.size()) RunCommand(g_cmdList[g_sel]);
        return;
    case Mode::Results:
        if (q.empty()) return;
        if (!forceRaw && g_sel >= 0 && g_sel < (int)g_results.size()) { Launch(&g_results[g_sel], q, admin); return; }
        Launch(nullptr, q, admin);
        return;
    default:
        if (q.empty()) return;
        Launch((g_hasMatch && !forceRaw) ? &g_match : nullptr, q, admin);
    }
}

static void Select(int idx) {
    int n = ItemCount();
    if (n == 0) return;
    idx = std::clamp(idx, 0, n - 1);
    if (idx == g_sel) return;
    g_sel = idx;
    if (g_mode == Mode::Themes) SetPreview(idx);
    RecomputeGhost();
}

// ---------------------------------------------------------------------------
// Text editing
// ---------------------------------------------------------------------------
static bool HasSel() { return g_anchor != g_caret; }

static void Touched() { g_lastInput = Now(); }

static void Edited() {
    Touched();
    UpdateState();
}

static void SetText(const std::wstring& s) {
    g_text = s;
    g_birth.assign(s.size(), 0.0);
    g_caret = g_anchor = (int)s.size();
    Edited();
}

static void DeleteRange(int a, int b) {
    if (a > b) std::swap(a, b);
    g_text.erase(a, b - a);
    g_birth.erase(g_birth.begin() + a, g_birth.begin() + b);
    g_caret = g_anchor = a;
}

static void DeleteSel() { DeleteRange(g_anchor, g_caret); }

static void InsertText(std::wstring s) {
    for (auto& c : s)
        if (c == L'\r' || c == L'\n' || c == L'\t') c = L' ';
    if (HasSel()) DeleteSel();
    double t = Now();
    g_text.insert(g_caret, s);
    g_birth.insert(g_birth.begin() + g_caret, s.size(), t);
    g_caret += (int)s.size();
    g_anchor = g_caret;
    Edited();
}

static int WordLeft(int i) {
    while (i > 0 && std::iswspace(g_text[i - 1])) i--;
    while (i > 0 && !std::iswspace(g_text[i - 1])) i--;
    return i;
}
static int WordRight(int i) {
    int n = (int)g_text.size();
    while (i < n && !std::iswspace(g_text[i])) i++;
    while (i < n && std::iswspace(g_text[i])) i++;
    return i;
}
static int PrevChar(int i) {
    if (i <= 0) return 0;
    i--;
    if (i > 0 && IS_LOW_SURROGATE(g_text[i]) && IS_HIGH_SURROGATE(g_text[i - 1])) i--;
    return i;
}
static int NextChar(int i) {
    int n = (int)g_text.size();
    if (i >= n) return n;
    i++;
    if (i < n && IS_LOW_SURROGATE(g_text[i]) && IS_HIGH_SURROGATE(g_text[i - 1])) i++;
    return i;
}

static void CopySelection() {
    if (!HasSel()) return;
    int a = std::min(g_anchor, g_caret), b = std::max(g_anchor, g_caret);
    std::wstring s = g_text.substr(a, b - a);
    if (!OpenClipboard(g_hwnd)) return;
    EmptyClipboard();
    HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE, (s.size() + 1) * sizeof(wchar_t));
    if (h) {
        memcpy(GlobalLock(h), s.c_str(), (s.size() + 1) * sizeof(wchar_t));
        GlobalUnlock(h);
        if (!SetClipboardData(CF_UNICODETEXT, h)) GlobalFree(h);
    }
    CloseClipboard();
}

static void Paste() {
    if (!OpenClipboard(g_hwnd)) return;
    std::wstring s;
    if (HANDLE h = GetClipboardData(CF_UNICODETEXT)) {
        if (auto* p = (const wchar_t*)GlobalLock(h)) { s = p; GlobalUnlock(h); }
    }
    CloseClipboard();
    if (!s.empty()) InsertText(s);
}

static void AcceptGhost() {
    if (g_ghost.empty()) return;
    InsertText(g_ghost);
}

static void MoveCaret(int pos, bool extend) {
    g_caret = std::clamp(pos, 0, (int)g_text.size());
    if (!extend) g_anchor = g_caret;
    Touched();
}

static void OnKey(WPARAM vk) {
    if (g_animTo < 0.5) return;
    bool ctrl = GetKeyState(VK_CONTROL) < 0, shift = GetKeyState(VK_SHIFT) < 0;
    int n = (int)g_text.size();
    bool grid = g_mode == Mode::Themes;
    bool list = g_mode == Mode::Results || g_mode == Mode::Commands;
    switch (vk) {
    case VK_ESCAPE: Hide(); break;
    case VK_RETURN: Activate(shift && !ctrl, ctrl && shift); break;
    case VK_TAB:
        if (g_mode == Mode::Commands && g_ghost.empty() && g_sel < (int)g_cmdList.size())
            SetText(std::wstring(L"> ") + kCmds[g_cmdList[g_sel]].key);
        else AcceptGhost();
        break;
    case VK_UP:
        if (grid) Select(g_sel - Lay::GridCols >= 0 ? g_sel - Lay::GridCols : g_sel);
        else if (list) Select(g_sel - 1);
        break;
    case VK_DOWN:
        if (grid) Select(g_sel + Lay::GridCols < kThemeCount ? g_sel + Lay::GridCols : g_sel);
        else if (list) Select(g_sel + 1);
        break;
    case VK_LEFT:
        if (grid && !shift) Select(g_sel - 1);
        else if (!shift && HasSel()) MoveCaret(std::min(g_anchor, g_caret), false);
        else MoveCaret(ctrl ? WordLeft(g_caret) : PrevChar(g_caret), shift);
        break;
    case VK_RIGHT:
        if (grid && !shift) Select(g_sel + 1);
        else if (!shift && HasSel()) MoveCaret(std::max(g_anchor, g_caret), false);
        else if (!shift && g_caret == n && !g_ghost.empty()) AcceptGhost();
        else MoveCaret(ctrl ? WordRight(g_caret) : NextChar(g_caret), shift);
        break;
    case VK_HOME: MoveCaret(0, shift); break;
    case VK_END:
        if (!shift && g_caret == n && !g_ghost.empty()) AcceptGhost();
        else MoveCaret(n, shift);
        break;
    case VK_BACK:
        if (HasSel()) DeleteSel();
        else if (g_caret > 0) DeleteRange(ctrl ? WordLeft(g_caret) : PrevChar(g_caret), g_caret);
        else break;
        Edited();
        break;
    case VK_DELETE:
        if (HasSel()) DeleteSel();
        else if (g_caret < n) DeleteRange(g_caret, ctrl ? WordRight(g_caret) : NextChar(g_caret));
        else break;
        Edited();
        break;
    case 'A':
        if (ctrl) { g_anchor = 0; g_caret = n; Touched(); }
        break;
    case 'C':
        if (ctrl) CopySelection();
        break;
    case 'X':
        if (ctrl && HasSel()) { CopySelection(); DeleteSel(); Edited(); }
        break;
    case 'V':
        if (ctrl) Paste();
        break;
    }
}

static void OnChar(wchar_t c) {
    if (g_animTo < 0.5) return;
    if (c < 32 || c == 127) return;  // control chars handled in OnKey
    InsertText(std::wstring(1, c));
}

static int HitTextIndex(float xDip) {
    IDWriteTextLayout* lay = nullptr;
    if (FAILED(g_dw->CreateTextLayout(g_text.c_str(), (UINT32)g_text.size(), g_fmtText, 100000.f, Lay::BarH, &lay)))
        return g_caret;
    BOOL trailing = FALSE, inside = FALSE;
    DWRITE_HIT_TEST_METRICS m;
    lay->HitTestPoint(xDip - (Lay::Margin + Lay::TextX) + g_scroll, Lay::BarH / 2, &trailing, &inside, &m);
    lay->Release();
    int pos = (int)m.textPosition + (trailing ? (int)m.length : 0);
    return std::clamp(pos, 0, (int)g_text.size());
}

static int HitItemAt(float x, float y) {
    if (y < Lay::Margin + Lay::BarH || y > Lay::Margin + Lay::BarH + g_panelH) return -1;
    for (const HitItem& h : g_hits)
        if (x >= h.r.left && x <= h.r.right && y >= h.r.top && y <= h.r.bottom) return h.index;
    return -1;
}

// ---------------------------------------------------------------------------
// Tray icon (the Windows 11 logo, drawn at runtime)
// ---------------------------------------------------------------------------
static HICON MakeLogoIcon(int size) {
    BITMAPINFO bi = {};
    bi.bmiHeader.biSize = sizeof(bi.bmiHeader);
    bi.bmiHeader.biWidth = size;
    bi.bmiHeader.biHeight = -size;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    HDC dc = GetDC(nullptr);
    HBITMAP color = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
    ReleaseDC(nullptr, dc);
    if (!color) return nullptr;

    auto* px = static_cast<UINT32*>(bits);
    const float pad = size * 0.06f, S = size - 2 * pad, gap = S * 0.09f, q = (S - gap) / 2;
    auto inside = [&](float x, float y) {
        x -= pad; y -= pad;
        if (x < 0 || y < 0 || x >= S || y >= S) return false;
        bool cx = x < q || x >= q + gap, cyy = y < q || y >= q + gap;
        return cx && cyy;
    };
    const float r0 = 0x4C, g0 = 0xC2, b0 = 0xFF, r1 = 0x00, g1 = 0x67, b1 = 0xC0;
    for (int y = 0; y < size; y++)
        for (int x = 0; x < size; x++) {
            int hits = 0;
            for (int sy = 0; sy < 4; sy++)
                for (int sx = 0; sx < 4; sx++)
                    hits += inside(x + (sx + 0.5f) / 4, y + (sy + 0.5f) / 4);
            float a = hits / 16.f;
            float t = std::clamp((x + y) / (2.f * size), 0.f, 1.f);
            float r = (r0 + (r1 - r0) * t) * a, g = (g0 + (g1 - g0) * t) * a, b = (b0 + (b1 - b0) * t) * a;
            px[y * size + x] = (UINT32(a * 255 + 0.5f) << 24) | (UINT32(r + 0.5f) << 16) | (UINT32(g + 0.5f) << 8) |
                               UINT32(b + 0.5f);
        }

    std::vector<BYTE> maskBits(((size + 15) / 16) * 2 * size, 0);
    HBITMAP mask = CreateBitmap(size, size, 1, 1, maskBits.data());
    ICONINFO ii = {TRUE, 0, 0, mask, color};
    HICON icon = CreateIconIndirect(&ii);
    DeleteObject(mask);
    DeleteObject(color);
    return icon;
}

static void AddTrayIcon() {
    NOTIFYICONDATAW nid = {sizeof(nid)};
    nid.hWnd = g_hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_APP_TRAY;
    nid.hIcon = g_trayIcon;
    wcscpy_s(nid.szTip, L"Custom Start — press the Windows key");
    Shell_NotifyIconW(NIM_ADD, &nid);
}

static void RemoveTrayIcon() {
    NOTIFYICONDATAW nid = {sizeof(nid)};
    nid.hWnd = g_hwnd;
    nid.uID = 1;
    Shell_NotifyIconW(NIM_DELETE, &nid);
}

static void ShowTrayMenu() {
    HMENU themes = CreatePopupMenu();
    for (int i = 0; i < kThemeCount; i++)
        AppendMenuW(themes, MF_STRING | (i == g_themeIdx ? MF_CHECKED : 0), ID_THEME0 + i, kThemes[i].name);
    HMENU m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, ID_OPEN, L"Open command bar");
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_POPUP, (UINT_PTR)themes, L"Theme");
    AppendMenuW(m, MF_STRING | (g_expanded ? MF_CHECKED : 0), ID_EXPANDED, L"Expanded results");
    AppendMenuW(m, MF_STRING | (g_intercept ? MF_CHECKED : 0), ID_INTERCEPT, L"Replace Start menu");
    AppendMenuW(m, MF_STRING | (IsAutoStart() ? MF_CHECKED : 0), ID_AUTOSTART, L"Start with Windows");
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING, ID_EXIT, L"Exit");
    SetMenuDefaultItem(m, ID_OPEN, FALSE);

    POINT pt;
    GetCursorPos(&pt);
    SetForegroundWindow(g_hwnd);
    int cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, g_hwnd, nullptr);
    PostMessageW(g_hwnd, WM_NULL, 0, 0);
    DestroyMenu(m);  // also destroys the submenu

    if (cmd >= ID_THEME0 && cmd < ID_THEME0 + kThemeCount) { ApplyTheme(cmd - ID_THEME0); return; }
    switch (cmd) {
    case ID_OPEN: Show(); break;
    case ID_INTERCEPT: g_intercept = !g_intercept.load(); break;
    case ID_EXPANDED:
        g_expanded = !g_expanded;
        WriteDword(L"Expanded", g_expanded ? 1 : 0);
        break;
    case ID_AUTOSTART: SetAutoStart(!IsAutoStart()); break;
    case ID_EXIT: DestroyWindow(g_hwnd); break;
    }
}

// ---------------------------------------------------------------------------
// Window procedure
// ---------------------------------------------------------------------------
static LRESULT CALLBACK WndProc(HWND h, UINT msg, WPARAM wp, LPARAM lp) {
    if (msg == g_wmTaskbarCreated && msg != 0) {
        AddTrayIcon();
        return 0;
    }
    switch (msg) {
    case WM_APP_TOGGLE: Toggle(); return 0;
    case WM_APP_HIDE: Hide(); return 0;
    case WM_APP_QUIT: DestroyWindow(h); return 0;
    case WM_TIMER:
        if (wp == TIMER_UPDATE_CHECK) {
            // Ask Setup (installed next to us) whether GitHub has a newer version; it stays
            // hidden unless there is one, then shows "Update to V<x>?". Never in the middle of
            // a game: wait until nothing is fullscreen.
            if (g_fullscreen || ForegroundIsFullscreen()) {
                SetTimer(h, TIMER_UPDATE_CHECK, 5 * 60 * 1000, nullptr);
                return 0;
            }
            SetTimer(h, TIMER_UPDATE_CHECK, 6 * 60 * 60 * 1000, nullptr);
            wchar_t path[MAX_PATH];
            GetModuleFileNameW(nullptr, path, MAX_PATH);
            std::wstring setup = path;
            setup = setup.substr(0, setup.find_last_of(L'\\') + 1) + L"CSMInstaller.exe";
            if (GetFileAttributesW(setup.c_str()) != INVALID_FILE_ATTRIBUTES)
                ShellExecuteW(nullptr, L"open", setup.c_str(), L"--check-update", nullptr, SW_SHOWNORMAL);
        }
        return 0;
    case WM_APP_APPS:
        if (g_mode == Mode::Results || g_mode == Mode::None) UpdateState();
        return 0;
    case WM_APP_TRAY:
        switch (LOWORD(lp)) {
        case WM_LBUTTONUP:
            // A tray click first deactivates the open bar; don't immediately re-open it.
            if (Now() - g_lastHideTime > 0.3) Toggle();
            break;
        case WM_RBUTTONUP:
        case WM_CONTEXTMENU: ShowTrayMenu(); break;
        }
        return 0;
    case WM_ACTIVATE:
        if (LOWORD(wp) == WA_INACTIVE) Hide();
        return 0;
    case WM_MOUSEACTIVATE: return MA_ACTIVATE;
    case WM_CHAR: OnChar((wchar_t)wp); return 0;
    case WM_KEYDOWN: OnKey(wp); return 0;
    case WM_LBUTTONDOWN: {
        float s = 96.f / g_dpi;
        float x = GET_X_LPARAM(lp) * s, y = GET_Y_LPARAM(lp) * s;
        using namespace Lay;
        if (x < Margin || x > Margin + BarW || y < Margin || y > Margin + BarH + g_panelH) { Hide(); return 0; }
        if (y > Margin + BarH) {
            int idx = HitItemAt(x, y);
            if (idx >= 0) {
                Select(idx);
                g_sel = idx;
                Activate(false, false);
            }
            return 0;
        }
        SetCapture(h);
        MoveCaret(HitTextIndex(x), GetKeyState(VK_SHIFT) < 0);
        return 0;
    }
    case WM_MOUSEMOVE: {
        POINT p = {GET_X_LPARAM(lp), GET_Y_LPARAM(lp)};
        if (p.x == g_lastMouse.x && p.y == g_lastMouse.y) return 0;
        g_lastMouse = p;
        float s = 96.f / g_dpi;
        if ((wp & MK_LBUTTON) && GetCapture() == h) {
            MoveCaret(HitTextIndex(p.x * s), true);
        } else {
            int idx = HitItemAt(p.x * s, p.y * s);
            if (idx >= 0) Select(idx);
        }
        return 0;
    }
    case WM_LBUTTONUP:
        if (GetCapture() == h) ReleaseCapture();
        return 0;
    case WM_SETCURSOR: {
        POINT p;
        GetCursorPos(&p);
        ScreenToClient(h, &p);
        float s = 96.f / g_dpi, x = p.x * s, y = p.y * s;
        using namespace Lay;
        bool text = x >= Margin + TextX - 4 && x <= Margin + BarW && y >= Margin && y <= Margin + BarH;
        SetCursor(LoadCursorW(nullptr, text ? IDC_IBEAM : IDC_ARROW));
        return TRUE;
    }
    case WM_DPICHANGED: return 0;  // sizing is handled when the bar is shown
    case WM_CLOSE: Hide(); return 0;
    case WM_DESTROY: PostQuitMessage(0); return 0;
    }
    return DefWindowProcW(h, msg, wp, lp);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------
int WINAPI wWinMain(HINSTANCE inst, HINSTANCE, PWSTR, int) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    HANDLE mutex = CreateMutexW(nullptr, TRUE, L"Local\\CustomStartMenu.Singleton");
    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        if (HWND other = FindWindowW(kClassName, nullptr)) PostMessageW(other, WM_APP_TOGGLE, 0, 0);
        return 0;
    }

    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    QueryPerformanceFrequency(&g_qpf);

    g_themeIdx = std::clamp((int)ReadDword(L"Theme", 0), 0, kThemeCount - 1);
    g_expanded = ReadDword(L"Expanded", 1) != 0;
    g_autostart = IsAutoStart();
    g_previewIdx = g_themeIdx;
    g_th = g_thFrom = kThemes[g_themeIdx];
    // Hardware acceleration (Setup asks; on by default). Falls back to software if there is
    // no usable Direct3D 11 device at all.
    g_hw = ReadDword(L"HardwareAcceleration", 1) != 0;
    if (g_hw) {
        if (ID3D11Device* probe = CreateD3DDevice()) probe->Release();
        else g_hw = false;
    }

    if (FAILED(D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, &g_d2d)) ||
        FAILED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                                   reinterpret_cast<IUnknown**>(&g_dw)))) {
        MessageBoxW(nullptr, L"Direct2D / DirectWrite unavailable.", L"Custom Start", MB_ICONERROR);
        return 1;
    }
    CreateTextResources();
    g_memDC = CreateCompatibleDC(nullptr);

    int iconSize = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem());
    g_trayIcon = MakeLogoIcon(iconSize);
    HICON bigIcon = MakeLogoIcon(GetSystemMetricsForDpi(SM_CXICON, GetDpiForSystem()));

    WNDCLASSEXW wc = {sizeof(wc)};
    wc.lpfnWndProc = WndProc;
    wc.hInstance = inst;
    wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    wc.hIcon = LoadIconW(inst, MAKEINTRESOURCEW(1));
    if (!wc.hIcon) wc.hIcon = bigIcon;
    wc.hIconSm = g_trayIcon;
    wc.lpszClassName = kClassName;
    RegisterClassExW(&wc);

    DWORD exStyle = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | (g_hw ? WS_EX_NOREDIRECTIONBITMAP : WS_EX_LAYERED);
    g_hwnd = CreateWindowExW(exStyle, kClassName, L"Custom Start", WS_POPUP, 0, 0, 1, 1, nullptr, nullptr, inst,
                             nullptr);
    if (!g_hwnd) return 1;

    g_wmTaskbarCreated = RegisterWindowMessageW(L"TaskbarCreated");
    AddTrayIcon();
    SetTimer(g_hwnd, TIMER_UPDATE_CHECK, 20 * 1000, nullptr);

    HANDLE hookReady = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    HANDLE hookThread = CreateThread(nullptr, 0, HookThread, hookReady, 0, &g_hookTid);
    if (hookThread) WaitForSingleObject(hookReady, 2000);
    CloseHandle(hookReady);

    g_quitEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    HANDLE uiaThread = CreateThread(nullptr, 0, StartButtonThread, nullptr, 0, nullptr);
    std::thread iconThread(IconWorker);
    RefreshApps(true);

    // Message loop: block while hidden; while visible, pump messages and present one
    // frame per display refresh (DwmFlush waits for the next composition pass).
    MSG msg;
    bool running = true;
    while (running) {
        if (g_visible) {
            while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
                if (msg.message == WM_QUIT) { running = false; break; }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if (!running) break;
            if (g_visible) {
                // The swap chain's Present(1) already waits for the next refresh; the layered
                // window (or a Present that returned early) waits on the compositor instead.
                double t0 = Now();
                Render();
                if (!g_hw || Now() - t0 < 0.004)
                    if (FAILED(DwmFlush())) Sleep(8);
            }
        } else {
            BOOL r = GetMessageW(&msg, nullptr, 0, 0);
            if (r <= 0) break;
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    SetEvent(g_quitEvent);
    {
        std::lock_guard<std::mutex> lk(g_iconMx);
        g_iconQuit = true;
    }
    g_iconCv.notify_all();
    iconThread.join();
    if (uiaThread) {
        WaitForSingleObject(uiaThread, 3000);
        CloseHandle(uiaThread);
    }
    if (hookThread) {
        PostThreadMessageW(g_hookTid, WM_QUIT, 0, 0);
        WaitForSingleObject(hookThread, 2000);
        CloseHandle(hookThread);
    }
    RemoveTrayIcon();
    DiscardDeviceResources();
    SafeRelease(g_placeholderLayout);
    for (auto* f : {g_fmtText, g_fmtHint, g_fmtRow, g_fmtRowRight, g_fmtGlyph, g_fmtHeader, g_fmtCard})
        if (f) f->Release();
    SafeRelease(g_dw);
    SafeRelease(g_d2d);
    if (g_dib) { SelectObject(g_memDC, g_oldBmp); DeleteObject(g_dib); }
    DeleteDC(g_memDC);
    DestroyIcon(g_trayIcon);
    DestroyIcon(bigIcon);
    CoUninitialize();
    ReleaseMutex(mutex);
    CloseHandle(mutex);
    return 0;
}
