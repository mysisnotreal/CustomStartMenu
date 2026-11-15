// ---------------------------------------------------------------------------
// "> emoji": every emoji in a grid (res/emoji.txt, built into the exe). Type to search by
// name; Enter copies. The ones used lately come first.
// ---------------------------------------------------------------------------
struct EmojiDef {
    std::wstring ch, name;
    int group;
};
static std::vector<EmojiDef> g_emoji;
static std::vector<std::wstring> g_emojiGroups;
static std::vector<int> g_emojiItems;  // what the grid shows: index into g_emoji
static std::vector<std::wstring> g_emojiCats;
static CardGrid g_emojiGrid{10, 42.f, 0.f};
static std::vector<std::wstring> g_emojiRecent;

static void LoadEmoji() {
    if (!g_emoji.empty()) return;
    HRSRC r = FindResourceW(nullptr, MAKEINTRESOURCEW(3), RT_RCDATA);
    HGLOBAL g = r ? LoadResource(nullptr, r) : nullptr;
    const char* data = g ? (const char*)LockResource(g) : nullptr;
    if (!data) return;
    std::wstring all = Utf8ToW(std::string(data, SizeofResource(nullptr, r)));
    size_t p = 0;
    while (p < all.size()) {
        size_t e = all.find(L'\n', p);
        if (e == std::wstring::npos) e = all.size();
        std::wstring line = all.substr(p, e - p);
        p = e + 1;
        if (line.empty()) continue;
        if (line[0] == L'@') {
            g_emojiGroups.push_back(line.substr(1));
            continue;
        }
        size_t tab = line.find(L'\t');
        if (tab == std::wstring::npos) continue;
        g_emoji.push_back({line.substr(0, tab), line.substr(tab + 1), (int)g_emojiGroups.size() - 1});
    }
    std::wstring rec;
    if (ReadString(L"RecentEmoji", rec)) {
        size_t q = 0;
        while (q < rec.size()) {
            size_t sp = rec.find(L' ', q);
            if (sp == std::wstring::npos) sp = rec.size();
            if (sp > q) g_emojiRecent.push_back(rec.substr(q, sp - q));
            q = sp + 1;
        }
    }
}

static void EmojiFill(bool first) {
    LoadEmoji();
    g_emojiItems.clear();
    g_emojiCats.clear();
    std::wstring a = Lower(g_panelArg);
    std::vector<std::wstring> words;
    for (size_t p = 0; p < a.size();) {
        size_t sp = a.find(L' ', p);
        if (sp == std::wstring::npos) sp = a.size();
        if (sp > p) words.push_back(a.substr(p, sp - p));
        p = sp + 1;
    }
    auto match = [&](const EmojiDef& e) {
        for (auto& w : words)
            if (e.name.find(w) == std::wstring::npos && Lower(g_emojiGroups[e.group]).find(w) == std::wstring::npos) return false;
        return true;
    };
    if (words.empty())
        for (auto& r : g_emojiRecent)
            for (int i = 0; i < (int)g_emoji.size(); i++)
                if (g_emoji[i].ch == r) {
                    g_emojiItems.push_back(i);
                    g_emojiCats.push_back(L"Recently used");
                    break;
                }
    for (int i = 0; i < (int)g_emoji.size(); i++)
        if (match(g_emoji[i])) {
            g_emojiItems.push_back(i);
            g_emojiCats.push_back(words.empty() ? g_emojiGroups[g_emoji[i].group] : L"Results");
        }
    g_emojiGrid.Layout(g_emojiCats);
    if (first) {
        g_sel = 0;
        g_emojiGrid.ScrollTo(0, true);
    }
}
static int EmojiCount() { return (int)g_emojiItems.size(); }
static float EmojiHeight() { return g_emojiItems.empty() ? 120.f : g_emojiGrid.PanelH(); }

static void DrawEmoji(float top, float dt, float alpha, double now) {
    using namespace Lay;
    CardGrid& G = g_emojiGrid;
    const Theme& th = g_th;
    const float cw = G.CardW(), ch = G.prevH;
    std::wstring selName;
    if (g_sel >= 0 && g_sel < (int)g_emojiItems.size()) selName = g_emoji[g_emojiItems[g_sel]].name;
    PanelHeader(L"Emoji", selName.empty() ? L"" : selName + L"   ↵ copy", top, alpha);
    float cTop = top + CardGrid::TopH, cBottom = top + g_panelH;
    G.Animate(dt);
    g_rt->PushAxisAlignedClip(D2D1::RectF(Margin, cTop, Margin + BarW, cBottom), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    if (g_emojiItems.empty()) DrawLayout(L"No emoji with that name", g_fmtRow, Margin + GridPad + 2, cTop, 300, 40, WithA(th.dim, alpha));
    for (auto& hd : G.headers) {
        float y = cTop + hd.second - G.off;
        if (y > cBottom || y + G.headH < cTop) continue;
        DrawLayout(hd.first, g_fmtHeader, Margin + GridPad + 2, y, 400, G.headH - 2, WithA(th.dim, alpha * 0.9f));
    }
    for (int k = 0; k < (int)G.cells.size(); k++) {
        const GridCell& c = G.cells[k];
        float x = c.x, y = cTop + c.y - G.off;
        G.cellSel[k] = Damp(G.cellSel[k], k == g_sel ? 1.f : 0.f, 18.f, dt);
        if (y > cBottom || y + G.RowH() < cTop) continue;
        float sel = G.cellSel[k];
        D2D1_RECT_F hit = D2D1::RectF(x, std::max(y, cTop), x + cw, std::min(y + ch, cBottom));
        if (hit.bottom > hit.top) g_hits.push_back({hit, k});
        if (sel > 0.01f) {
            g_solid->SetColor(WithA(th.highlight, alpha * sel * 2.f));
            g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x, y, x + cw, y + ch), 9, 9), g_solid);
            g_solid->SetColor(WithA(th.accent, alpha * sel));
            g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + .75f, y + .75f, x + cw - .75f, y + ch - .75f), 8.5f, 8.5f), g_solid, 1.5f);
        }
        const std::wstring& e = g_emoji[g_emojiItems[c.item]].ch;
        IDWriteTextLayout* l = nullptr;
        if (SUCCEEDED(g_dw->CreateTextLayout(e.c_str(), (UINT32)e.size(), g_fmtEmoji, cw, ch, &l))) {
            float sk = 1.f + 0.12f * sel;
            D2D1_MATRIX_3X2_F old;
            g_rt->GetTransform(&old);
            g_rt->SetTransform(Matrix3x2F::Scale(sk, sk, {x + cw / 2, y + ch / 2}) * Matrix3x2F::ReinterpretBaseType(&old)[0]);
            g_solid->SetColor(WithA(th.text, alpha));
            g_rt->DrawTextLayout({x, y}, l, g_solid, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
            g_rt->SetTransform(old);
            l->Release();
        }
    }
    g_rt->PopAxisAlignedClip();
    float view = cBottom - cTop;
    if (G.MaxOff() > 0.5f) DrawScrollBar(cTop + 4, view - 8, G.off / G.MaxOff(), view / (G.contentH + 6), alpha, dt);
}

static void EmojiCopy(int k) {
    if (k < 0 || k >= (int)g_emojiItems.size()) return;
    const std::wstring e = g_emoji[g_emojiItems[k]].ch;
    if (!SetClipboardText(e)) return;
    Flash(L"Copied " + e);
    g_emojiRecent.erase(std::remove(g_emojiRecent.begin(), g_emojiRecent.end(), e), g_emojiRecent.end());
    g_emojiRecent.insert(g_emojiRecent.begin(), e);
    if (g_emojiRecent.size() > 20) g_emojiRecent.resize(20);
    std::wstring s;
    for (auto& r : g_emojiRecent) s += (s.empty() ? L"" : L" ") + r;
    WriteString(L"RecentEmoji", s);
}
static bool EmojiKey(WPARAM vk, bool shift) {
    int k = g_emojiGrid.Key(vk, shift, g_sel);
    if (k < 0) return false;
    Select(k);
    g_emojiGrid.EnsureVisible(g_sel, false);
    return true;
}
static bool EmojiClick(float x, float y) {
    int k = HitItemAt(x, y);
    if (k >= 0) {
        g_sel = k;
        EmojiCopy(k);
    }
    return true;
}
static void EmojiWheel(int steps) { g_emojiGrid.ScrollTo(g_emojiGrid.offTarget + steps * 60.f, false); }
static void EmojiScrollFrac(float f) { g_emojiGrid.ScrollTo(f * g_emojiGrid.MaxOff(), true); }
