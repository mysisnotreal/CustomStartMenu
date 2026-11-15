// Every panel, in the order the "> " list shows them. Same as cpp/register.inl.

use super::panels::*;
use super::*;
use super::{custom as cu, tools as tl, widgets as wg};

fn on_off(on: bool) -> String {
    if on { "On" } else { "Off" }.into()
}
fn radio_detail(kind: usize) -> String {
    let s = tl::RADIO[kind].load(Relaxed);
    if s < 0 {
        String::new()
    } else {
        on_off(s == 1)
    }
}

pub fn register_panels() {
    // ---- live ----
    let mut p = PanelDef::new("media", "Now playing", "\u{EC4F}");
    p.aliases = vec!["music", "playing", "song", "np", "nowplaying"];
    p.fill = Some(wg::media_fill);
    p.leave = Some(wg::media_leave);
    p.detail = Some(|| {
        let (m, _) = wg::media_now();
        if m.has && !m.title.is_empty() {
            m.title
        } else {
            "Nothing playing".into()
        }
    });
    p.height = Some(wg::media_height);
    p.draw = Some(wg::draw_media);
    p.keys = Some(wg::media_key);
    p.click = Some(wg::media_click);
    p.enter = Some(wg::media_enter);
    add_panel(p);

    let mut p = PanelDef::new("weather", "Weather", "\u{E753}");
    p.aliases = vec!["forecast", "temp", "temperature"];
    p.fill = Some(|first| {
        if first {
            wg::weather_maybe_refresh(false);
        }
    });
    p.detail = Some(|| {
        let w = wg::weather_now();
        if w.ok {
            format!("{}  {}", wg::deg(w.temp), wg::weather_word(w.code, w.day))
        } else {
            String::new()
        }
    });
    p.height = Some(wg::weather_height);
    p.draw = Some(wg::draw_weather);
    p.enter = Some(|_, _| {
        wg::weather_maybe_refresh(true);
        flash("Refreshing…");
    });
    add_panel(p);

    let mut p = PanelDef::new("clock", "Clock & calendar", "\u{E787}");
    p.aliases = vec!["calendar", "time", "date", "cal"];
    p.fill = Some(|first| {
        if first {
            wg::calendar_leave();
        }
    });
    p.detail = Some(wg::calendar_detail);
    p.height = Some(wg::calendar_height);
    p.draw = Some(wg::draw_calendar);
    p.keys = Some(wg::calendar_key);
    p.click = Some(wg::calendar_click);
    p.enter = Some(wg::calendar_enter);
    add_panel(p);

    let mut p = PanelDef::new("system", "This PC", "\u{E9D9}");
    p.aliases = vec!["stats", "cpu", "ram", "gpu", "pc", "specs", "usage"];
    p.detail = Some(wg::system_detail);
    p.height = Some(wg::system_height);
    p.draw = Some(wg::draw_system);
    add_panel(p);

    let mut p = PanelDef::new("network", "Network", "\u{E968}");
    p.aliases = vec!["net", "internet", "speed"];
    p.detail = Some(wg::network_detail);
    p.height = Some(wg::network_height);
    p.draw = Some(wg::draw_network);
    add_panel(p);

    let mut p = PanelDef::new("timer", "Timer", "\u{E916}");
    p.takes_arg = true;
    p.detail = Some(|| {
        let d = wg::timer_detail();
        if d.is_empty() {
            "5m, 90s, 1h30m…".into()
        } else {
            d
        }
    });
    p.height = Some(wg::timer_height);
    p.draw = Some(wg::draw_timer);
    p.enter = Some(wg::timer_enter);
    p.keys = Some(wg::timer_key);
    add_panel(p);

    let mut p = PanelDef::new("stopwatch", "Stopwatch", "\u{E916}");
    p.aliases = vec!["sw"];
    p.detail = Some(wg::stopwatch_detail);
    p.height = Some(wg::stopwatch_height);
    p.draw = Some(wg::draw_stopwatch);
    p.enter = Some(wg::stopwatch_enter);
    p.keys = Some(wg::stopwatch_key);
    add_panel(p);

    // ---- tools ----
    let mut p = PanelDef::new("windows", "Switch window", "\u{E8AB}");
    p.aliases = vec!["window", "switch", "win"];
    p.fill = Some(tl::windows_fill);
    p.enter = Some(tl::windows_enter);
    p.del = Some(tl::windows_del);
    p.refresh = 2.0;
    p.empty = "No open windows";
    add_panel(p);

    let mut p = PanelDef::new("emoji", "Emoji", "\u{E76E}");
    p.aliases = vec!["emojis", "em"];
    p.takes_arg = true;
    p.fill = Some(emoji::fill);
    p.count = Some(emoji::count);
    p.height = Some(emoji::height);
    p.draw = Some(emoji::draw);
    p.keys = Some(emoji::key);
    p.click = Some(emoji::click);
    p.wheel = Some(emoji::wheel);
    p.scroll_frac = Some(emoji::scroll_frac);
    p.enter = Some(emoji::enter);
    add_panel(p);

    let mut p = PanelDef::new("clip", "Clipboard history", "\u{E77F}");
    p.aliases = vec!["clipboard", "clips", "pastes"];
    p.fill = Some(tl::clip_fill);
    p.enter = Some(tl::clip_enter);
    p.del = Some(tl::clip_del);
    p.detail = Some(|| {
        let n = tl::clip_count();
        if n == 0 {
            String::new()
        } else {
            format!("{} copied", n)
        }
    });
    p.empty = "Copy some text and it shows up here (kept only until Custom Start closes)";
    add_panel(p);

    let mut p = PanelDef::new("files", "Find files", "\u{E8B7}");
    p.arg_only = vec!["f"];
    p.aliases = vec!["file", "find"];
    p.takes_arg = true;
    p.fill = Some(tl::files_fill);
    p.enter = Some(tl::files_enter);
    p.detail = Some(|| "> f name".into());
    p.empty = "Type part of a file name";
    add_panel(p);

    let mut p = PanelDef::new("kill", "Close an app", "\u{E711}");
    p.aliases = vec!["taskkill", "end", "close"];
    p.takes_arg = true;
    p.fill = Some(tl::kill_fill);
    p.enter = Some(tl::kill_enter);
    p.refresh = 3.0;
    p.empty = "Nothing by that name is running";
    add_panel(p);

    let mut p = PanelDef::new("note", "New note", "\u{E70B}");
    p.takes_arg = true;
    p.fill = Some(tl::notes_fill);
    p.enter = Some(tl::notes_enter);
    p.del = Some(tl::notes_del);
    p.empty = "Type a note after > note";
    add_panel(p);

    let mut p = PanelDef::new("notes", "Notes", "\u{E8F1}");
    p.fill = Some(tl::notes_fill);
    p.enter = Some(tl::notes_enter);
    p.del = Some(tl::notes_del);
    p.detail = Some(|| {
        let n = tl::notes_count();
        if n == 0 {
            String::new()
        } else {
            n.to_string()
        }
    });
    p.empty = "No notes yet. Type > note and something to remember";
    add_panel(p);

    let mut p = PanelDef::new("searches", "Web searches", "\u{E774}");
    p.aliases = vec!["web", "search"];
    p.fill = Some(calc::searches_fill);
    p.enter = Some(calc::searches_enter);
    p.detail = Some(|| "g cats, yt lofi…".into());
    add_panel(p);

    // ---- this PC ----
    let mut p = PanelDef::new("volume", "Volume", "\u{E767}");
    p.aliases = vec!["vol", "mute", "unmute"];
    p.takes_arg = true;
    p.fill = Some(|_| wg::volume_read(true));
    p.refresh = 0.5;
    p.detail = Some(wg::volume_detail);
    p.height = Some(wg::slider_height);
    p.draw = Some(|top, _, alpha, _| wg::draw_slider(0, top, alpha));
    p.keys = Some(|vk, shift| wg::slider_key(0, vk, shift));
    p.click = Some(|x, y| wg::slider_click(0, x, y));
    p.wheel = Some(|steps| wg::volume_wheel(steps));
    p.enter = Some(|_, _| wg::slider_enter(0));
    add_panel(p);

    let mut p = PanelDef::new("brightness", "Brightness", "\u{E706}");
    p.aliases = vec!["bright", "screen"];
    p.takes_arg = true;
    p.fill = Some(|first| {
        if first {
            wg::brightness_kick();
        }
    });
    p.detail = Some(wg::brightness_detail);
    p.height = Some(wg::slider_height);
    p.draw = Some(|top, _, alpha, _| wg::draw_slider(1, top, alpha));
    p.keys = Some(|vk, shift| wg::slider_key(1, vk, shift));
    p.click = Some(|x, y| wg::slider_click(1, x, y));
    p.wheel = Some(|steps| wg::brightness_wheel(steps));
    p.enter = Some(|_, _| wg::slider_enter(1));
    add_panel(p);

    let mut p = PanelDef::new("wifi", "Wi‑Fi", "\u{E701}");
    p.aliases = vec!["wi-fi", "wlan"];
    p.fill = Some(|first| {
        if first {
            tl::radio_refresh(0, -1);
        }
        tl::radio_fill(0);
    });
    p.enter = Some(|_, _| tl::radio_enter(0));
    p.detail = Some(|| radio_detail(0));
    add_panel(p);

    let mut p = PanelDef::new("bluetooth", "Bluetooth", "\u{E702}");
    p.aliases = vec!["bt"];
    p.fill = Some(|first| {
        if first {
            tl::radio_refresh(1, -1);
        }
        tl::radio_fill(1);
    });
    p.enter = Some(|_, _| tl::radio_enter(1));
    p.detail = Some(|| radio_detail(1));
    add_panel(p);

    let mut p = PanelDef::new("dark", "Dark mode", "\u{E708}");
    p.aliases = vec!["darkmode", "dark mode", "light", "lightmode", "light mode"];
    p.fill = Some(tl::dark_fill);
    p.enter = Some(tl::dark_enter);
    p.detail = Some(|| on_off(tl::windows_dark()));
    add_panel(p);

    for (i, &(key, title, glyph, _)) in tl::POWER.iter().enumerate() {
        let mut p = PanelDef::new(key, title, glyph);
        p.tag = i as i32;
        match i {
            2 => p.aliases = vec!["reboot"],
            3 => p.aliases = vec!["shut down", "poweroff", "power off"],
            4 => p.aliases = vec!["sign out", "logoff", "logout", "log out"],
            _ => {}
        }
        p.fill = Some(tl::power_fill);
        p.enter = Some(tl::power_enter);
        add_panel(p);
    }

    // ---- make it yours ----
    let mut p = PanelDef::new("newtheme", "Make your own theme", "\u{E771}");
    p.aliases = vec!["theme new", "theme edit", "theme custom", "custom theme", "themeeditor", "edit theme"];
    p.fill = Some(cu::theme_edit_fill);
    p.enter = Some(cu::theme_edit_enter);
    p.keys = Some(cu::theme_edit_key);
    p.leave = Some(cu::theme_edit_leave);
    add_panel(p);

    let mut p = PanelDef::new("sounds", "Sounds", "\u{E8D6}");
    p.fill = Some(|_| settings_fill(&cu::sound_defs()));
    p.keys = Some(|vk, shift| settings_key(&cu::sound_defs(), vk, shift));
    p.enter = Some(|sel, _| settings_enter(&cu::sound_defs(), sel));
    p.detail = Some(|| on_off(cu::sounds_on()));
    add_panel(p);

    let mut p = PanelDef::new("layout", "Layout", "\u{E80A}");
    p.aliases = vec!["position", "size"];
    p.fill = Some(|_| settings_fill(&cu::layout_defs()));
    p.keys = Some(|vk, shift| settings_key(&cu::layout_defs(), vk, shift));
    p.enter = Some(|sel, _| settings_enter(&cu::layout_defs(), sel));
    p.detail = Some(cu::layout_detail);
    add_panel(p);

    let mut p = PanelDef::new("hotkey", "Hotkey", "\u{E765}");
    p.aliases = vec!["shortcut", "keys"];
    p.fill = Some(|_| settings_fill(&cu::hotkey_defs()));
    p.keys = Some(|vk, shift| settings_key(&cu::hotkey_defs(), vk, shift));
    p.enter = Some(|sel, _| settings_enter(&cu::hotkey_defs(), sel));
    p.detail = Some(cu::hotkey_detail);
    add_panel(p);

    let mut p = PanelDef::new("commandsfolder", "Your commands folder", "\u{E8B7}");
    p.aliases = vec!["mycommands", "scripts"];
    p.fill = Some(|_| {
        let r = rows();
        r.clear();
        r.push(PRow {
            glyph: "\u{E838}".into(),
            title: "Open the Commands folder".into(),
            detail: "Scripts and apps there become > commands".into(),
            action: "Open  ↵".into(),
            ..Default::default()
        });
    });
    p.enter = Some(|_, _| tl::open_commands_folder());
    p.detail = Some(|| {
        tl::scan_user_commands();
        let n = ps().user_cmds.len();
        if n == 0 {
            "Add your own".into()
        } else {
            format!("{} added", n)
        }
    });
    add_panel(p);

    let mut p = PanelDef::new("\u{1}run", "", "");
    p.listed = false;
    p.fill = Some(tl::user_cmd_fill);
    p.enter = Some(tl::user_cmd_enter);
    ps().run_panel = add_panel(p) as i32;
}
