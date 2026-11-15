// The few Windows Runtime APIs the bar uses (what's playing, the Wi-Fi / Bluetooth radios),
// kept in their own file so main.cpp doesn't need the C++/WinRT headers. All of these block,
// so call them from worker threads.
#pragma once
#include <cstdint>
#include <string>
#include <vector>

namespace wb {

struct MediaInfo {
    bool has = false;      // something is (or was) playing
    bool playing = false;
    std::wstring title, artist, app;
    double pos = 0, dur = 0;     // seconds, as of the moment it was read
    bool canPrev = false, canNext = false;
    std::vector<uint8_t> thumb;  // the album art file (PNG / JPEG), when asked for
};

// Reads the current media session (the one Windows shows in its volume flyout).
bool MediaPoll(MediaInfo& out, bool wantThumb);
// 0 play / pause, 1 next, 2 previous
void MediaCommand(int cmd);

// kind 0 = Wi-Fi, 1 = Bluetooth. State: -1 no such radio, 0 off, 1 on.
int RadioState(int kind);
bool RadioSet(int kind, bool on);

}  // namespace wb
