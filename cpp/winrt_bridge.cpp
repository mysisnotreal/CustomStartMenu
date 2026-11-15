// See winrt_bridge.h.
#include "winrt_bridge.h"

#include <winrt/Windows.Devices.Radios.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Media.Control.h>
#include <winrt/Windows.Storage.Streams.h>
#pragma comment(lib, "windowsapp")

using namespace winrt;
using namespace winrt::Windows::Media::Control;
using namespace winrt::Windows::Devices::Radios;
using namespace winrt::Windows::Storage::Streams;

namespace wb {

static GlobalSystemMediaTransportControlsSessionManager Manager() {
    static GlobalSystemMediaTransportControlsSessionManager m{nullptr};
    if (!m) m = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().get();
    return m;
}

static double Seconds(winrt::Windows::Foundation::TimeSpan t) {
    return std::chrono::duration<double>(t).count();
}

bool MediaPoll(MediaInfo& out, bool wantThumb) {
    out = MediaInfo();
    try {
        auto session = Manager().GetCurrentSession();
        if (!session) return true;
        auto props = session.TryGetMediaPropertiesAsync().get();
        auto pb = session.GetPlaybackInfo();
        auto tl = session.GetTimelineProperties();
        out.has = true;
        out.title = props.Title().c_str();
        out.artist = props.Artist().c_str();
        if (out.artist.empty()) out.artist = props.AlbumArtist().c_str();
        out.app = session.SourceAppUserModelId().c_str();
        out.playing = pb.PlaybackStatus() == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
        auto ctl = pb.Controls();
        out.canPrev = ctl.IsPreviousEnabled();
        out.canNext = ctl.IsNextEnabled();
        out.dur = Seconds(tl.EndTime() - tl.StartTime());
        out.pos = Seconds(tl.Position() - tl.StartTime());
        if (out.playing) {  // the position is from when the app last reported it
            auto since = winrt::clock::now() - tl.LastUpdatedTime();
            double s = std::chrono::duration<double>(since).count();
            if (s > 0 && s < 24 * 3600) out.pos += s;
        }
        if (out.dur > 0) out.pos = std::min(std::max(out.pos, 0.0), out.dur);
        if (wantThumb) {
            if (auto ref = props.Thumbnail()) {
                auto stream = ref.OpenReadAsync().get();
                uint32_t size = (uint32_t)stream.Size();
                if (size > 0 && size < (16u << 20)) {
                    Buffer buf(size);
                    auto got = stream.ReadAsync(buf, size, InputStreamOptions::None).get();
                    out.thumb.assign(got.data(), got.data() + got.Length());
                }
            }
        }
        return true;
    } catch (...) {
        return false;
    }
}

void MediaCommand(int cmd) {
    try {
        auto session = Manager().GetCurrentSession();
        if (!session) return;
        if (cmd == 0) session.TryTogglePlayPauseAsync().get();
        else if (cmd == 1) session.TrySkipNextAsync().get();
        else session.TrySkipPreviousAsync().get();
    } catch (...) {
    }
}

static Radio FindRadio(int kind) {
    static bool asked = false;
    if (!asked) {
        asked = true;
        try { Radio::RequestAccessAsync().get(); } catch (...) {}
    }
    for (auto r : Radio::GetRadiosAsync().get())
        if (r.Kind() == (kind == 0 ? RadioKind::WiFi : RadioKind::Bluetooth)) return r;
    return nullptr;
}

int RadioState(int kind) {
    try {
        auto r = FindRadio(kind);
        if (!r) return -1;
        return r.State() == winrt::Windows::Devices::Radios::RadioState::On ? 1 : 0;
    } catch (...) {
        return -1;
    }
}

bool RadioSet(int kind, bool on) {
    try {
        auto r = FindRadio(kind);
        if (!r) return false;
        auto st = r.SetStateAsync(on ? winrt::Windows::Devices::Radios::RadioState::On
                                     : winrt::Windows::Devices::Radios::RadioState::Off).get();
        return st == RadioAccessStatus::Allowed;
    } catch (...) {
        return false;
    }
}

}  // namespace wb
