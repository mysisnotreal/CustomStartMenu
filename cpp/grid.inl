// ---------------------------------------------------------------------------
// Card grids ("> font", "> caret"): cards in rows under a title for each category,
// scrolled inside the panel. Also the panel's scroll bar, which can be dragged.
// Included by main.cpp before the list drawing.
// ---------------------------------------------------------------------------
struct GridCell {
    int item;    // position in the grid's list
    float x, y;  // x in bar space, y from the top of the scrolling area
};

struct CardGrid {
    int cols;
    float prevH, nameH;
    float rowGap = 8.f, headH = 28.f;
    std::vector<GridCell> cells;
    std::vector<std::pair<std::wstring, float>> headers;  // category title, y
    std::vector<float> cellSel;
    float contentH = 0, off = 0, offTarget = 0;

    static constexpr float TopH = Lay::GridPad + Lay::GridHeaderH - 4;  // fixed header above the cards
    float CardW() const { return (Lay::BarW - 2 * Lay::GridPad - (cols - 1) * Lay::CardGap) / cols; }
    float RowH() const { return prevH + nameH + rowGap; }
    float PanelH() const { return std::min(Lay::MaxPanelH, TopH + contentH + 6.f); }
    float ViewH() const { return PanelH() - TopH; }
    float MaxOff() const { return std::max(0.f, contentH + 6.f - ViewH()); }
    void ScrollTo(float o, bool instant) {
        offTarget = std::clamp(o, 0.f, MaxOff());
        if (instant) off = offTarget;
    }
    void Animate(float dt) {
        off = Damp(off, offTarget, 18.f, dt);
        if (std::fabs(off - offTarget) < 0.3f) off = offTarget;
    }

    // One category per item, in order; each new category starts a row under its title.
    void Layout(const std::vector<std::wstring>& cats) {
        cells.clear();
        headers.clear();
        float y = 0;
        int col = 0;
        for (int k = 0; k < (int)cats.size(); k++) {
            if (k == 0 || cats[k] != cats[k - 1]) {
                if (col) { y += RowH(); col = 0; }
                headers.push_back({cats[k], y});
                y += headH;
            }
            cells.push_back({k, Lay::Margin + Lay::GridPad + col * (CardW() + Lay::CardGap), y});
            if (++col == cols) { col = 0; y += RowH(); }
        }
        if (col) y += RowH();
        contentH = cats.empty() ? 44.f : y;
        cellSel.assign(cells.size(), 0.f);
    }

    // Keeps a card (and its category title when it's the first row) in view.
    void EnsureVisible(int k, bool instant) {
        if (k < 0 || k >= (int)cells.size()) return;
        float y = cells[k].y, view = ViewH();
        float top = y - headH - 4, bottom = y + RowH();
        if (top < offTarget) ScrollTo(top, instant);
        else if (bottom > offTarget + view) ScrollTo(bottom - view, instant);
    }

    // Up / Down: the card in the next row nearest to this one's column.
    int Vertical(int k, int dir) const {
        if (k < 0 || k >= (int)cells.size()) return k;
        float y = cells[k].y, x = cells[k].x;
        int best = k;
        float bestRowY = dir > 0 ? 1e9f : -1e9f, bestDx = 1e9f;
        for (int i = 0; i < (int)cells.size(); i++) {
            float cy = cells[i].y;
            if (dir > 0 ? cy <= y + 1 : cy >= y - 1) continue;
            bool closerRow = dir > 0 ? cy < bestRowY - 1 : cy > bestRowY + 1;
            bool sameRow = std::fabs(cy - bestRowY) <= 1;
            float dx = std::fabs(cells[i].x - x);
            if (closerRow || (sameRow && dx < bestDx)) { best = i; bestRowY = cy; bestDx = dx; }
        }
        return best;
    }

    // Arrow keys / Page Up / Page Down; returns the new selection, or -1 when the key isn't ours.
    int Key(WPARAM vk, bool shift, int sel) const {
        int n = (int)cells.size();
        if (!n) return -1;
        int k = sel;
        switch (vk) {
        case VK_UP: k = Vertical(sel, -1); break;
        case VK_DOWN: k = Vertical(sel, 1); break;
        case VK_LEFT: if (shift) return -1; k = sel - 1; break;
        case VK_RIGHT: if (shift) return -1; k = sel + 1; break;
        case VK_PRIOR: for (int i = 0; i < 3; i++) k = Vertical(k, -1); break;
        case VK_NEXT: for (int i = 0; i < 3; i++) k = Vertical(k, 1); break;
        default: return -1;
        }
        return std::clamp(k, 0, n - 1);
    }
};

// ---- scroll bar ------------------------------------------------------------------------
struct ScrollBarGeo {
    bool shown = false;
    float trackTop = 0, trackH = 0, thumbY = 0, thumbH = 0;
};
static ScrollBarGeo g_sb;        // the panel's scroll bar as last drawn (bar space)
static bool g_sbDrag = false;    // the thumb is being dragged
static float g_sbGrab = 0;       // where on the thumb it was grabbed
static bool g_sbHot = false;     // under the mouse
static float g_sbW = 3.f;        // drawn width (grows under the mouse)

// frac: how far down the content is scrolled (0..1); visible: how much of it fits (0..1).
static void DrawScrollBar(float trackTop, float trackH, float frac, float visible, float alpha, float dt) {
    using namespace Lay;
    const Theme& th = g_th;
    float thumbH = std::max(24.f, trackH * std::clamp(visible, 0.f, 1.f));
    float thumbY = trackTop + (trackH - thumbH) * std::clamp(frac, 0.f, 1.f);
    g_sb = {true, trackTop, trackH, thumbY, thumbH};
    bool wide = g_sbHot || g_sbDrag;
    g_sbW = Damp(g_sbW, wide ? 6.f : 3.f, 18.f, dt);
    float x1 = Margin + BarW - 3, x0 = x1 - g_sbW;
    if (wide) {  // a faint track so it's clear where it can go
        g_solid->SetColor(WithA(th.dim, alpha * 0.12f));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x0, trackTop, x1, trackTop + trackH), g_sbW / 2, g_sbW / 2), g_solid);
    }
    g_solid->SetColor(WithA(th.dim, alpha * (g_sbDrag ? 0.85f : wide ? 0.7f : 0.45f)));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x0, thumbY, x1, thumbY + thumbH), g_sbW / 2, g_sbW / 2), g_solid);
}

static bool ScrollBarHit(float x, float y) {
    using namespace Lay;
    return g_sb.shown && x >= Margin + BarW - 16 && x <= Margin + BarW + 2 && y >= g_sb.trackTop - 4 &&
           y <= g_sb.trackTop + g_sb.trackH + 4;
}
static bool ScrollBarOnThumb(float y) { return y >= g_sb.thumbY && y <= g_sb.thumbY + g_sb.thumbH; }
// Where the thumb's top goes for the mouse at y, as 0..1 of the track.
static float ScrollBarFrac(float y) {
    float room = std::max(1.f, g_sb.trackH - g_sb.thumbH);
    return std::clamp((y - g_sbGrab - g_sb.trackTop) / room, 0.f, 1.f);
}
