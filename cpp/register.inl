// ---------------------------------------------------------------------------
// Every panel, in the order the "> " list shows them.
// ---------------------------------------------------------------------------

static void RegisterPanels() {
    // ---- live ----
    {
        PanelDef& p = AddPanel(L"media", L"Now playing", L"\xEC4F");
        p.aliases = {L"music", L"playing", L"song", L"np", L"nowplaying"};
        p.fill = [](bool) { VizStart(); MediaWake(); };
        p.leave = [] { VizStop(); };
        p.detail = [] {
            wb::MediaInfo m = MediaNow(nullptr);
            return m.has && !m.title.empty() ? m.title : std::wstring(L"Nothing playing");
        };
        p.height = MediaHeight;
        p.draw = DrawMedia;
        p.keys = MediaKey;
        p.click = MediaClick;
        p.enter = [](int, bool) { MediaSend(0); };
    }
    {
        PanelDef& p = AddPanel(L"weather", L"Weather", L"\xE753");
        p.aliases = {L"forecast", L"temp", L"temperature"};
        p.fill = [](bool first) { if (first) WeatherMaybeRefresh(); };
        p.detail = [] {
            WeatherState w = WeatherNow();
            return w.ok ? Deg(w.temp) + L"  " + WeatherWord(w.code, w.day) : std::wstring(L"");
        };
        p.height = WeatherHeight;
        p.draw = DrawWeather;
        p.enter = [](int, bool) { WeatherMaybeRefresh(true); Flash(L"Refreshing…"); };
    }
    {
        PanelDef& p = AddPanel(L"clock", L"Clock & calendar", L"\xE787");
        p.aliases = {L"calendar", L"time", L"date", L"cal"};
        p.fill = [](bool first) { if (first) g_calMonthOff = 0; };
        p.detail = [] {
            SYSTEMTIME st;
            GetLocalTime(&st);
            return TimeText(st, false);
        };
        p.height = CalendarHeight;
        p.draw = DrawCalendar;
        p.keys = CalendarKey;
        p.click = CalendarClick;
        p.enter = [](int, bool) { g_calMonthOff = 0; };
    }
    {
        PanelDef& p = AddPanel(L"system", L"This PC", L"\xE9D9");
        p.aliases = {L"stats", L"cpu", L"ram", L"gpu", L"pc", L"specs", L"usage"};
        p.detail = [] {
            SysState s = SysNow();
            return L"CPU " + Pct(s.cpuNow) + L"  ·  RAM " + Pct(s.ramNow);
        };
        p.height = SystemHeight;
        p.draw = DrawSystem;
    }
    {
        PanelDef& p = AddPanel(L"network", L"Network", L"\xE968");
        p.aliases = {L"net", L"internet", L"speed"};
        p.detail = [] {
            SysState s = SysNow();
            return std::wstring(s.net == 1 ? L"Wi‑Fi" : s.net == 2 ? L"Ethernet" : L"Offline");
        };
        p.height = NetworkHeight;
        p.draw = DrawNetwork;
    }
    {
        PanelDef& p = AddPanel(L"timer", L"Timer", L"\xE916");
        p.takesArg = true;
        p.detail = [] { return g_tmSet ? Clock(std::ceil(TimerLeft())) + (g_tmRun ? L" left" : L" paused") : std::wstring(L"5m, 90s, 1h30m…"); };
        p.height = TimerHeight;
        p.draw = DrawTimer;
        p.enter = TimerEnter;
        p.keys = TimerKey;
    }
    {
        PanelDef& p = AddPanel(L"stopwatch", L"Stopwatch", L"\xE916");
        p.aliases = {L"sw"};
        p.detail = [] { return g_swRun || StopwatchNow() > 0 ? SwText(StopwatchNow(), false) : std::wstring(L""); };
        p.height = StopwatchHeight;
        p.draw = DrawStopwatch;
        p.enter = StopwatchEnter;
        p.keys = StopwatchKey;
    }
    // ---- tools ----
    {
        PanelDef& p = AddPanel(L"windows", L"Switch window", L"\xE8AB");
        p.aliases = {L"window", L"switch", L"win"};
        p.fill = WindowsFill;
        p.enter = WindowsEnter;
        p.del = WindowsDel;
        p.refresh = 2.0;
        p.empty = L"No open windows";
    }
    {
        PanelDef& p = AddPanel(L"emoji", L"Emoji", L"\xE76E");
        p.aliases = {L"emojis", L"em"};
        p.takesArg = true;
        p.fill = EmojiFill;
        p.count = EmojiCount;
        p.height = EmojiHeight;
        p.draw = DrawEmoji;
        p.keys = EmojiKey;
        p.click = EmojiClick;
        p.wheel = EmojiWheel;
        p.scrollFrac = EmojiScrollFrac;
        p.enter = [](int sel, bool) { EmojiCopy(sel); };
    }
    {
        PanelDef& p = AddPanel(L"clip", L"Clipboard history", L"\xE77F");
        p.aliases = {L"clipboard", L"clips", L"pastes"};
        p.fill = ClipFill;
        p.enter = ClipEnter;
        p.del = ClipDel;
        p.detail = [] { return g_clips.empty() ? std::wstring(L"") : std::to_wstring(g_clips.size()) + L" copied"; };
        p.empty = L"Copy some text and it shows up here (kept only until Custom Start closes)";
    }
    {
        PanelDef& p = AddPanel(L"files", L"Find files", L"\xE8B7");
        p.argOnly = {L"f"};
        p.aliases = {L"file", L"find"};
        p.takesArg = true;
        p.fill = FilesFill;
        p.enter = FilesEnter;
        p.detail = [] { return std::wstring(L"> f name"); };
        p.empty = L"Type part of a file name";
    }
    {
        PanelDef& p = AddPanel(L"kill", L"Close an app", L"\xE711");
        p.aliases = {L"taskkill", L"end", L"close"};
        p.takesArg = true;
        p.fill = KillFill;
        p.enter = KillEnter;
        p.refresh = 3.0;
        p.empty = L"Nothing by that name is running";
    }
    {
        PanelDef& p = AddPanel(L"note", L"New note", L"\xE70B");
        p.takesArg = true;
        p.fill = NotesFill;
        p.enter = NotesEnter;
        p.del = NotesDel;
        p.empty = L"Type a note after > note";
    }
    {
        PanelDef& p = AddPanel(L"notes", L"Notes", L"\xE8F1");
        p.fill = NotesFill;
        p.enter = NotesEnter;
        p.del = NotesDel;
        p.detail = [] {
            size_t n = LoadNotes().size();
            return n ? std::to_wstring(n) : std::wstring(L"");
        };
        p.empty = L"No notes yet. Type > note and something to remember";
    }
    {
        PanelDef& p = AddPanel(L"searches", L"Web searches", L"\xE774");
        p.aliases = {L"web", L"search"};
        p.fill = SearchesFill;
        p.enter = SearchesEnter;
        p.detail = [] { return std::wstring(L"g cats, yt lofi…"); };
    }
    // ---- this PC ----
    {
        PanelDef& p = AddPanel(L"volume", L"Volume", L"\xE767");
        p.aliases = {L"vol", L"mute", L"unmute"};
        p.takesArg = true;
        p.fill = [](bool) { VolumeRead(true); };
        p.refresh = 0.5;
        p.detail = [] {
            VolumeRead();
            return g_volShown < 0 ? std::wstring(L"") : g_muteShown ? std::wstring(L"Muted") : std::to_wstring(g_volShown) + L"%";
        };
        p.height = SliderHeight;
        p.draw = [](float top, float, float alpha, double) { DrawSlider(0, top, alpha); };
        p.keys = [](WPARAM vk, bool shift) { return SliderKey(0, vk, shift); };
        p.click = [](float x, float y) { return SliderClick(0, x, y); };
        p.wheel = [](int steps) { if (g_volShown >= 0) VolumeSet(g_volShown - steps * 2); };
        p.enter = [](int, bool) { SliderEnter(0); };
    }
    {
        PanelDef& p = AddPanel(L"brightness", L"Brightness", L"\xE706");
        p.aliases = {L"bright", L"screen"};
        p.takesArg = true;
        p.fill = [](bool first) { if (first) BrightnessKick(); };
        p.detail = [] { int b = g_bright; return b >= 0 ? std::to_wstring(b) + L"%" : std::wstring(L""); };
        p.height = SliderHeight;
        p.draw = [](float top, float, float alpha, double) { DrawSlider(1, top, alpha); };
        p.keys = [](WPARAM vk, bool shift) { return SliderKey(1, vk, shift); };
        p.click = [](float x, float y) { return SliderClick(1, x, y); };
        p.wheel = [](int steps) { if (g_bright >= 0) BrightnessSet(g_bright - steps * 5); };
        p.enter = [](int, bool) { SliderEnter(1); };
    }
    {
        PanelDef& p = AddPanel(L"wifi", L"Wi‑Fi", L"\xE701");
        p.aliases = {L"wi-fi", L"wlan"};
        p.fill = [](bool first) { if (first) RadioRefresh(0); RadioFill(0); };
        p.enter = [](int, bool) { RadioEnter(0); };
        p.detail = [] { int s = g_radio[0]; return s < 0 ? std::wstring(L"") : s ? std::wstring(L"On") : std::wstring(L"Off"); };
    }
    {
        PanelDef& p = AddPanel(L"bluetooth", L"Bluetooth", L"\xE702");
        p.aliases = {L"bt"};
        p.fill = [](bool first) { if (first) RadioRefresh(1); RadioFill(1); };
        p.enter = [](int, bool) { RadioEnter(1); };
        p.detail = [] { int s = g_radio[1]; return s < 0 ? std::wstring(L"") : s ? std::wstring(L"On") : std::wstring(L"Off"); };
    }
    {
        PanelDef& p = AddPanel(L"dark", L"Dark mode", L"\xE708");
        p.aliases = {L"darkmode", L"dark mode", L"light", L"lightmode", L"light mode"};
        p.fill = DarkFill;
        p.enter = DarkEnter;
        p.detail = [] { return std::wstring(WindowsDark() ? L"On" : L"Off"); };
    }
    for (int i = 0; i < 5; i++) {
        PanelDef& p = AddPanel(kPower[i].key, kPower[i].title, kPower[i].glyph);
        if (i == PW_RESTART) p.aliases = {L"reboot"};
        if (i == PW_SHUTDOWN) p.aliases = {L"shut down", L"poweroff", L"power off"};
        if (i == PW_SIGNOUT) p.aliases = {L"sign out", L"logoff", L"logout", L"log out"};
        p.fill = [i](bool) { PowerFill(i); };
        p.enter = [i](int, bool) { PowerEnter(i); };
    }
    // ---- make it yours ----
    {
        PanelDef& p = AddPanel(L"newtheme", L"Make your own theme", L"\xE771");
        p.aliases = {L"theme new", L"theme edit", L"theme custom", L"custom theme", L"themeeditor", L"edit theme"};
        p.fill = ThemeEditFill;
        p.enter = ThemeEditEnter;
        p.keys = ThemeEditKey;
        p.leave = ThemeEditLeave;
    }
    {
        PanelDef& p = AddPanel(L"sounds", L"Sounds", L"\xE8D6");
        p.fill = [](bool) { SettingsFill(SoundDefs()); };
        p.keys = [](WPARAM vk, bool shift) { return SettingsKey(SoundDefs(), vk, shift); };
        p.enter = [](int sel, bool) { SettingsEnter(SoundDefs(), sel); };
        p.detail = [] { return std::wstring(g_sndType || g_sndOpen ? L"On" : L"Off"); };
    }
    {
        PanelDef& p = AddPanel(L"layout", L"Layout", L"\xE80A");
        p.aliases = {L"position", L"size"};
        p.fill = [](bool) { SettingsFill(LayoutDefs()); };
        p.keys = [](WPARAM vk, bool shift) { return SettingsKey(LayoutDefs(), vk, shift); };
        p.enter = [](int sel, bool) { SettingsEnter(LayoutDefs(), sel); };
        p.detail = [] { return std::wstring(kLayWidth[g_layWidth]) + L" · " + kLayPos[g_layPos]; };
    }
    {
        PanelDef& p = AddPanel(L"hotkey", L"Hotkey", L"\xE765");
        p.aliases = {L"shortcut", L"keys"};
        p.fill = [](bool) { SettingsFill(HotkeyDefs()); };
        p.keys = [](WPARAM vk, bool shift) { return SettingsKey(HotkeyDefs(), vk, shift); };
        p.enter = [](int sel, bool) { SettingsEnter(HotkeyDefs(), sel); };
        p.detail = [] { return std::wstring(g_hotkey ? kHotkeys[g_hotkey] : g_intercept ? L"Windows key" : L"Tray only"); };
    }
    {
        PanelDef& p = AddPanel(L"commandsfolder", L"Your commands folder", L"\xE8B7");
        p.aliases = {L"mycommands", L"scripts"};
        p.fill = [](bool) {
            g_prows.clear();
            PRow r;
            r.glyph = L"\xE838";
            r.title = L"Open the Commands folder";
            r.detail = L"Scripts and apps there become > commands";
            r.action = L"Open  ↵";
            g_prows.push_back(r);
        };
        p.enter = [](int, bool) { OpenCommandsFolder(); };
        p.detail = [] {
            ScanUserCommands();
            return g_userCmds.empty() ? std::wstring(L"Add your own") : std::to_wstring(g_userCmds.size()) + L" added";
        };
    }
    {
        PanelDef& p = AddPanel(L"\x1run", L"", L"");
        p.listed = false;
        p.fill = UserCmdFill;
        p.enter = UserCmdEnter;
        g_runPanel = (int)g_panels.size() - 1;
    }
}
