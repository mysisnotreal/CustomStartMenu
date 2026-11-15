// ---------------------------------------------------------------------------
// Panels: the "> commands" added in 1.5 (live widgets, tools, settings). Each one is a
// PanelDef. A panel is either a list of rows (g_prows, drawn by DrawRows) or draws itself.
// Included by main.cpp after caret.inl; the declarations it shares with main.cpp are at the
// top of main.cpp (PRow, g_prows, g_panel...).
// ---------------------------------------------------------------------------
static void Launch(const App* app, const std::wstring& q, bool admin);
static void Hide();
static void SetText(const std::wstring& s);
static void Select(int idx);
static void ShowInExplorer(const std::wstring& file);
static std::wstring UrlEncode(const std::wstring& s);

struct PanelDef {
    std::wstring key, title, glyph;
    std::vector<std::wstring> aliases;  // other names that open it ("> music" for "> media")
    std::vector<std::wstring> argOnly;  // names that only count with something after them ("> f cat")
    bool listed = true;                 // shown in the "> " list
    bool takesArg = false;              // picking it from the list types "> key " (with a space)
    std::function<void(bool first)> fill;  // (re)builds g_prows from g_panelArg
    std::function<std::wstring()> detail;  // right-hand text in the "> " list
    std::function<void(int sel, bool shift)> enter;
    std::function<bool(int sel)> del;      // Delete with the caret at the end of the text
    std::function<float()> height;         // set for panels that draw themselves
    std::function<void(float top, float dt, float alpha, double now)> draw;
    std::function<bool(WPARAM vk, bool shift)> keys;
    std::function<bool(float x, float y)> click;
    std::function<void(int steps)> wheel;
    std::function<void(float frac)> scrollFrac;  // the scroll bar was dragged (self-drawn panels)
    std::function<int()> count;
    std::function<void()> leave;
    std::wstring empty = L"Nothing here";
    double refresh = 0;  // seconds between fill(false) calls while it's open (live lists)
};
static std::vector<PanelDef> g_panels;
static double g_panelFillAt = 0;

static PanelDef& AddPanel(const wchar_t* key, const wchar_t* title, const wchar_t* glyph) {
    g_panels.emplace_back();
    PanelDef& p = g_panels.back();
    p.key = key;
    p.title = title;
    p.glyph = glyph;
    return p;
}
static int FindPanel(const wchar_t* key) {
    for (int i = 0; i < (int)g_panels.size(); i++)
        if (g_panels[i].key == key) return i;
    return -1;
}
static PanelDef* CurPanel() { return g_panel >= 0 && g_panel < (int)g_panels.size() ? &g_panels[g_panel] : nullptr; }

// ---- flash messages and "press Enter again" ------------------------------------------
static void Flash(const std::wstring& s) {
    g_flash = s;
    g_flashTime = Now();
}
static std::wstring g_confirmKey;
static double g_confirmAt = -10;
// The first Enter arms, a second one within 3 seconds goes ahead.
static bool Confirmed(const std::wstring& what) {
    double t = Now();
    if (g_confirmKey == what && t - g_confirmAt < 3.0) {
        g_confirmKey.clear();
        return true;
    }
    g_confirmKey = what;
    g_confirmAt = t;
    return false;
}
static bool Armed(const std::wstring& what) { return g_confirmKey == what && Now() - g_confirmAt < 3.0; }

static void PanelLeave() {
    PanelDef* p = CurPanel();
    if (p && p->leave) p->leave();
    g_confirmKey.clear();
}
static std::wstring CurPanelEmpty() {
    PanelDef* p = CurPanel();
    return p ? p->empty : L"";
}

static void PanelFill(bool first) {
    PanelDef* p = CurPanel();
    if (!p) return;
    g_panelFillAt = Now();
    if (first) g_prows.clear();
    if (p->fill) p->fill(first);
    int n = PanelItemCount();
    g_sel = n ? std::clamp(g_sel, 0, n - 1) : 0;
}

// Live panels refresh themselves while they're open.
static void PanelTick(double now) {
    PanelDef* p = CurPanel();
    if (g_mode != Mode::Panel || !p || p->refresh <= 0) return;
    if (now - g_panelFillAt >= p->refresh) PanelFill(false);
}

static bool PanelHasRows() {
    PanelDef* p = CurPanel();
    return p && !p->draw;
}
static int PanelItemCount() {
    PanelDef* p = CurPanel();
    if (!p) return 0;
    if (p->count) return p->count();
    return p->draw ? 0 : (int)g_prows.size();
}
static float PanelHeight() {
    using namespace Lay;
    PanelDef* p = CurPanel();
    if (!p) return 0;
    if (p->height) return std::min(MaxPanelH, p->height());
    return PanelPad * 2 + RowH * std::clamp((int)g_prows.size(), 1, MaxRows);
}

// Which panel the text after ">" opens: the longest name (or alias) that is the whole text
// or is followed by a space. arg is the rest, as typed.
static int PanelMatch(const std::wstring& ql, const std::wstring& q, std::wstring& arg) {
    int best = -1;
    size_t bestLen = 0;
    for (int i = 0; i < (int)g_panels.size(); i++) {
        auto test = [&](const std::wstring& k) {
            if (k.size() <= bestLen) return;
            if (ql == k || (ql.size() > k.size() && ql.compare(0, k.size(), k) == 0 && ql[k.size()] == L' ')) {
                best = i;
                bestLen = k.size();
            }
        };
        test(g_panels[i].key);
        for (auto& a : g_panels[i].aliases) test(a);
        for (auto& a : g_panels[i].argOnly)
            if (ql.size() > a.size() + 1) test(a);
    }
    if (best >= 0) arg = Trim(q.substr(std::min(bestLen, q.size())));
    return best;
}

static void PanelActivate(bool shift) {
    PanelDef* p = CurPanel();
    if (p && p->enter) p->enter(g_sel, shift);
}

static bool PanelKey(WPARAM vk, bool shift) {
    PanelDef* p = CurPanel();
    if (!p) return false;
    if (p->keys && p->keys(vk, shift)) return true;
    if (vk == VK_DELETE && p->del && g_caret == (int)g_text.size() && g_anchor == g_caret) return p->del(g_sel);
    return false;
}

// ---- the "> " list: built-in commands, then panels, then the user's own commands -------
struct UserCmd {
    std::wstring name, lower, path;
};
static std::vector<UserCmd> g_userCmds;  // g_userCmd: the one "> name" is about to run
enum { CMDID_PANEL = 1000, CMDID_USER = 5000 };

static std::wstring CmdKey(int id) {
    if (id >= CMDID_USER) return g_userCmds[id - CMDID_USER].lower;
    if (id >= CMDID_PANEL) return g_panels[id - CMDID_PANEL].key;
    return kCmds[id].key;
}
static std::wstring CmdTitle(int id) {
    if (id >= CMDID_USER) return g_userCmds[id - CMDID_USER].name;
    if (id >= CMDID_PANEL) return g_panels[id - CMDID_PANEL].title;
    return kCmds[id].title;
}
static std::wstring CmdGlyph(int id) {
    if (id >= CMDID_USER) return L"\xE756";  // command prompt
    if (id >= CMDID_PANEL) return g_panels[id - CMDID_PANEL].glyph;
    return kCmds[id].glyph;
}
static std::wstring CommandDetail(int c);
static std::wstring CmdDetailOf(int id) {
    if (id >= CMDID_USER) {
        std::wstring ext = g_userCmds[id - CMDID_USER].path;
        size_t d = ext.find_last_of(L'.');
        return d == std::wstring::npos ? L"Your command" : L"Your command · " + Lower(ext.substr(d + 1));
    }
    if (id >= CMDID_PANEL) {
        auto& p = g_panels[id - CMDID_PANEL];
        return p.detail ? p.detail() : L"";
    }
    return CommandDetail(id);
}
static void CmdRun(int id) {
    std::wstring k = CmdKey(id);
    bool arg = id >= CMDID_USER || (id >= CMDID_PANEL && g_panels[id - CMDID_PANEL].takesArg);
    SetText(L"> " + k + (arg ? L" " : L""));
}

static void ScanUserCommands();
static void CmdListFill(const std::wstring& q) {
    ScanUserCommands();
    auto match = [&](const std::wstring& key, const std::wstring& title, const std::vector<std::wstring>* al) {
        if (q.empty() || StartsWith(key, q) || Lower(title).find(q) != std::wstring::npos) return true;
        if (al)
            for (auto& a : *al)
                if (StartsWith(a, q)) return true;
        return false;
    };
    for (int i = 0; i <= CMD_FONTFOLDER; i++)
        if (match(kCmds[i].key, kCmds[i].title, nullptr)) g_cmdList.push_back(i);
    for (int i = 0; i < (int)g_panels.size(); i++)
        if (g_panels[i].listed && match(g_panels[i].key, g_panels[i].title, &g_panels[i].aliases))
            g_cmdList.push_back(CMDID_PANEL + i);
    for (int i = CMD_FONTFOLDER + 1; i < kCmdCount; i++)
        if (match(kCmds[i].key, kCmds[i].title, nullptr)) g_cmdList.push_back(i);
    for (int i = 0; i < (int)g_userCmds.size(); i++)
        if (match(g_userCmds[i].lower, g_userCmds[i].name, nullptr)) g_cmdList.push_back(CMDID_USER + i);
}

// ---- small shared helpers ---------------------------------------------------------------
// The install folder (Programs\CustomStartMenu), where Notes.txt, Commands\ and the caches live.
static std::wstring AppDir() {
    std::wstring d = KnownFolderPath(FOLDERID_UserProgramFiles);
    if (d.empty()) d = ExpandEnv(L"%LOCALAPPDATA%\\Programs");
    d += L"\\CustomStartMenu";
    CreateDirectoryW(d.c_str(), nullptr);
    return d;
}

static bool SetClipboardText(const std::wstring& s) {
    if (!OpenClipboard(g_hwnd)) return false;
    EmptyClipboard();
    bool ok = false;
    HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE, (s.size() + 1) * sizeof(wchar_t));
    if (h) {
        memcpy(GlobalLock(h), s.c_str(), (s.size() + 1) * sizeof(wchar_t));
        GlobalUnlock(h);
        ok = SetClipboardData(CF_UNICODETEXT, h) != nullptr;
        if (!ok) GlobalFree(h);
    }
    CloseClipboard();
    return ok;
}

static std::wstring Utf8ToW(const std::string& s) {
    if (s.empty()) return L"";
    int n = MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0);
    std::wstring w(n, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), &w[0], n);
    return w;
}
static std::string WToUtf8(const std::wstring& s) {
    if (s.empty()) return "";
    int n = WideCharToMultiByte(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0, nullptr, nullptr);
    std::string u(n, '\0');
    WideCharToMultiByte(CP_UTF8, 0, s.data(), (int)s.size(), &u[0], n, nullptr, nullptr);
    return u;
}

// "3 min ago", "2 h ago", "yesterday"...
static std::wstring Ago(double seconds) {
    if (seconds < 60) return L"just now";
    if (seconds < 3600) return std::to_wstring((int)(seconds / 60)) + L" min ago";
    if (seconds < 86400) return std::to_wstring((int)(seconds / 3600)) + L" h ago";
    int d = (int)(seconds / 86400);
    return d == 1 ? L"yesterday" : std::to_wstring(d) + L" days ago";
}

static std::wstring OneLine(const std::wstring& s, size_t max = 160) {
    std::wstring o;
    bool sp = false;
    for (wchar_t c : s) {
        if (c == L'\r' || c == L'\n' || c == L'\t' || c == L' ') {
            sp = !o.empty();
            continue;
        }
        if (sp) o.push_back(L' ');
        sp = false;
        o.push_back(c);
        if (o.size() >= max) break;
    }
    return o;
}

static std::wstring FormatBytes(double b) {
    const wchar_t* u[] = {L"B", L"KB", L"MB", L"GB", L"TB"};
    int i = 0;
    while (b >= 1024 && i < 4) { b /= 1024; i++; }
    wchar_t buf[32];
    swprintf(buf, 32, i == 0 || b >= 100 ? L"%.0f %s" : L"%.1f %s", b, u[i]);
    return buf;
}

// Settings panels ("> sounds", "> layout"...): one row per setting; Left / Right / Enter
// change the selected one.
struct SettingDef {
    std::wstring name, glyph;
    std::vector<std::wstring> options;
    std::function<int()> get;
    std::function<void(int)> set;
};
static void SettingsFill(const std::vector<SettingDef>& defs) {
    g_prows.clear();
    for (auto& d : defs) {
        PRow r;
        r.glyph = d.glyph;
        r.title = d.name;
        int v = std::clamp(d.get(), 0, (int)d.options.size() - 1);
        r.detail = d.options[v];
        r.action = L"‹  " + d.options[v] + L"  ›";
        r.on = true;
        g_prows.push_back(r);
    }
}
static bool SettingsKey(const std::vector<SettingDef>& defs, WPARAM vk, bool shift) {
    if (shift || (vk != VK_LEFT && vk != VK_RIGHT)) return false;
    if (g_sel < 0 || g_sel >= (int)defs.size()) return false;
    auto& d = defs[g_sel];
    int n = (int)d.options.size();
    d.set(((d.get() + (vk == VK_RIGHT ? 1 : -1)) % n + n) % n);
    PanelFill(false);
    return true;
}
static void SettingsEnter(const std::vector<SettingDef>& defs, int sel) {
    if (sel < 0 || sel >= (int)defs.size()) return;
    auto& d = defs[sel];
    d.set((d.get() + 1) % (int)d.options.size());
    PanelFill(false);
}

// A row in a panel list.
static void DrawPanelRow(const PRow& r, float y, bool isSel, float rowA, double now) {
    using namespace Lay;
    const Theme& th = g_th;
    float ix = Margin + 20, iy = y + (RowH - 24) / 2;
    if (!r.icon.empty()) {
        double shownAt = 0;
        ID2D1Bitmap* icon = GetIcon(r.icon, &shownAt);
        float io = icon ? (float)EaseOutCubic(Clamp01((now - shownAt) / 0.18)) : 0.f;
        if (io < 1.f) DrawLayout(r.glyph.empty() ? L"\xE7C3" : r.glyph, g_fmtGlyph, ix, y, 24, RowH, WithA(th.dim, rowA * (1 - io)));
        if (icon) g_rt->DrawBitmap(icon, D2D1::RectF(ix, iy, ix + 24, iy + 24), rowA * io);
    } else if (r.swatch.a > 0) {
        g_solid->SetColor(WithA(r.swatch, rowA));
        g_rt->FillEllipse(D2D1::Ellipse({ix + 12, y + RowH / 2}, 8, 8), g_solid);
    } else {
        DrawLayout(r.glyph, g_fmtGlyph, ix, y, 24, RowH, WithA(isSel ? th.accent : th.dim, rowA));
    }
    std::wstring right = isSel && !r.action.empty() ? r.action : r.detail;
    float rw = 0;
    const float maxW = std::min(300.f, (BarW - 76) * 0.48f);  // the title keeps at least half
    if (!right.empty()) {
        IDWriteTextLayout* l = nullptr;
        if (SUCCEEDED(g_dw->CreateTextLayout(right.c_str(), (UINT32)right.size(), g_fmtRowRight, maxW, RowH, &l))) {
            DWRITE_TRIMMING trim{DWRITE_TRIMMING_GRANULARITY_CHARACTER, 0, 0};
            IDWriteInlineObject* sign = nullptr;
            if (SUCCEEDED(g_dw->CreateEllipsisTrimmingSign(l, &sign))) {
                l->SetTrimming(&trim, sign);
                sign->Release();
            }
            DWRITE_TEXT_METRICS tm;
            l->GetMetrics(&tm);
            rw = std::min(maxW, tm.width);
            g_solid->SetColor(WithA(r.on || (isSel && !r.action.empty()) ? th.accent : th.dim, rowA * (isSel ? 1.f : 0.85f)));
            g_rt->DrawTextLayout({Margin + BarW - 20 - maxW, y}, l, g_solid);
            l->Release();
        }
    }
    DrawLayout(r.title, g_fmtRow, Margin + 56, y, BarW - 56 - 36 - rw, RowH,
               WithA(isSel ? th.text : LerpC(th.dim, th.text, 0.55f), rowA));
}

// Shared look for the self-drawn panels: a section title on the left, key hints on the right.
static void PanelHeader(const std::wstring& title, const std::wstring& hints, float top, float alpha) {
    using namespace Lay;
    DrawLayout(title, g_fmtHeader, Margin + GridPad + 2, top + GridPad - 4, 300, GridHeaderH, WithA(g_th.dim, alpha));
    if (!hints.empty())
        DrawLayout(hints, g_fmtRowRight, Margin + BarW - GridPad - 330, top + GridPad - 4, 328, GridHeaderH,
                   WithA(g_th.dim, alpha * 0.85f));
}

// A filled round-cornered card in the panel's material (like the theme cards).
static void DrawCard(const D2D1_RECT_F& r, float alpha, float radius = 10.f) {
    const Theme& th = g_th;
    g_solid->SetColor(WithA(th.highlight, alpha * 1.1f));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(r, radius, radius), g_solid);
    g_solid->SetColor(WithA(th.divider, alpha));
    g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(r.left + .5f, r.top + .5f, r.right - .5f, r.bottom - .5f), radius, radius),
                               g_solid, 1.f);
}

// A line graph of values 0..1 (oldest first), filled underneath.
static void DrawGraph(const std::vector<float>& v, const D2D1_RECT_F& r, D2D1_COLOR_F col, float alpha, float maxV = 1.f) {
    if (v.size() < 2 || maxV <= 0) return;
    ID2D1PathGeometry* line = nullptr;
    ID2D1PathGeometry* fill = nullptr;
    if (FAILED(g_d2d->CreatePathGeometry(&line)) || FAILED(g_d2d->CreatePathGeometry(&fill))) {
        SafeRelease(line);
        return;
    }
    float w = r.right - r.left, h = r.bottom - r.top;
    auto pt = [&](size_t i) {
        float x = r.left + w * (float)i / (float)(v.size() - 1);
        float y = r.bottom - h * std::clamp(v[i] / maxV, 0.f, 1.f);
        return D2D1::Point2F(x, y);
    };
    ID2D1GeometrySink* s = nullptr;
    if (SUCCEEDED(line->Open(&s))) {
        s->BeginFigure(pt(0), D2D1_FIGURE_BEGIN_HOLLOW);
        for (size_t i = 1; i < v.size(); i++) s->AddLine(pt(i));
        s->EndFigure(D2D1_FIGURE_END_OPEN);
        s->Close();
        s->Release();
    }
    if (SUCCEEDED(fill->Open(&s))) {
        s->BeginFigure({r.left, r.bottom}, D2D1_FIGURE_BEGIN_FILLED);
        for (size_t i = 0; i < v.size(); i++) s->AddLine(pt(i));
        s->AddLine({r.right, r.bottom});
        s->EndFigure(D2D1_FIGURE_END_CLOSED);
        s->Close();
        s->Release();
    }
    if (ID2D1LinearGradientBrush* b = MakeGradient({0, r.top}, {0, r.bottom}, WithA(col, alpha * 0.30f), WithA(col, 0.f))) {
        g_rt->FillGeometry(fill, b);
        b->Release();
    }
    g_solid->SetColor(WithA(col, alpha));
    g_rt->DrawGeometry(line, g_solid, 1.5f);
    line->Release();
    fill->Release();
}

// Big text helpers (formats made in CreateTextResources).
static float TextWidth(const std::wstring& s, IDWriteTextFormat* f) {
    IDWriteTextLayout* l = nullptr;
    float w = 0;
    if (SUCCEEDED(g_dw->CreateTextLayout(s.c_str(), (UINT32)s.size(), f, 4000.f, 100.f, &l))) {
        DWRITE_TEXT_METRICS tm;
        l->GetMetrics(&tm);
        w = tm.widthIncludingTrailingWhitespace;
        l->Release();
    }
    return w;
}
