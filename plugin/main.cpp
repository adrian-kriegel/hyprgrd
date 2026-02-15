// hyprgrd-plugin — Hyprland plugin that adds native dispatchers and swipe
// gesture forwarding for hyprgrd.
//
// ## Dispatchers
//
//   hyprgrd:go                  <direction>      — move one grid cell
//   hyprgrd:movego              <direction>      — move the focused window and follow
//   hyprgrd:switch              <col> <row>      — jump to an absolute grid position
//   hyprgrd:movetomonitor       <direction>      — move focused window to monitor in direction
//   hyprgrd:movetomonitorindex  <n>              — move focused window to monitor n (0-based)
//   hyprgrd:togglevis                            — toggle persistent visualizer overlay
//
// ## Swipe gesture forwarding
//
// The plugin hooks Hyprland's swipeBegin / swipeUpdate / swipeEnd events,
// forwards them to the hyprgrd daemon over its Unix socket as
// SwipeBegin / SwipeUpdate / SwipeEnd JSON commands, and **cancels** the
// default Hyprland workspace-swipe handling.  This lets hyprgrd own the
// gesture without Hyprland fighting over it.
//
// Requires Hyprland 0.51+ gesture config so the compositor emits swipe
// events (the plugin then eats them before Hyprland acts). Example:
//
//   gestures {
//       gesture = 3, horizontal, workspace
//       gesture = 4, horizontal, workspace
//   }
//
//   bind = SUPER, right, hyprgrd:go,     right
//   bind = SUPER, left,  hyprgrd:go,     left
//   bind = SUPER, up,    hyprgrd:go,     up
//   bind = SUPER, down,  hyprgrd:go,     down
//
//   bind = SUPER SHIFT, right, hyprgrd:movego, right
//   bind = SUPER SHIFT, left,  hyprgrd:movego, left
//
//   bind = SUPER, 1, hyprgrd:switch, 0 0
//   bind = SUPER, 2, hyprgrd:switch, 1 0

#include "helpers.hpp"

#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/devices/IPointer.hpp>

#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include <any>
#include <cstring>

inline HANDLE PHANDLE = nullptr;

/// Finger count of the current swipe (set on swipeBegin, used through swipeEnd).
static uint32_t g_swipeFingers = 0;

/// Connect to the hyprgrd Unix socket, send `json` + newline, and close.
/// Returns true on success.
static bool sendCommand(const std::string& json) {
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0)
        return false;

    struct sockaddr_un addr {};
    addr.sun_family = AF_UNIX;
    std::string path = socketPath();
    strncpy(addr.sun_path, path.c_str(), sizeof(addr.sun_path) - 1);

    if (connect(fd, reinterpret_cast<struct sockaddr*>(&addr), sizeof(addr)) < 0) {
        close(fd);
        return false;
    }

    std::string msg = json + "\n";
    ssize_t written = write(fd, msg.c_str(), msg.size());
    close(fd);
    return written == static_cast<ssize_t>(msg.size());
}

//  Dispatchers 

/// hyprgrd:go <direction>
///
/// Move one grid cell in the given direction.
/// <direction> is one of: left, right, up, down (case-insensitive).
///
/// Sends: {"Go":"Right"}  (etc.)
static SDispatchResult dispatchGo(std::string arg) {
    auto result = buildGoJson(arg);
    if (!result.ok)
        return SDispatchResult{.success = false, .error = result.value};
    bool ok = sendCommand(result.value);
    return ok ? SDispatchResult{} : SDispatchResult{.success = false, .error = "failed to send command"};
}

/// hyprgrd:movego <direction>
///
/// Move the focused window one grid cell and follow it.
/// <direction> is one of: left, right, up, down (case-insensitive).
///
/// Sends: {"MoveWindowAndGo":"Right"}  (etc.)
static SDispatchResult dispatchMoveGo(std::string arg) {
    auto result = buildMoveGoJson(arg);
    if (!result.ok)
        return SDispatchResult{.success = false, .error = result.value};
    bool ok = sendCommand(result.value);
    return ok ? SDispatchResult{} : SDispatchResult{.success = false, .error = "failed to send command"};
}

/// hyprgrd:switch <col> <row>
///
/// Jump to an absolute grid position.
/// Arguments are space-separated integers (0-indexed).
///
/// Sends: {"SwitchTo":{"x":2,"y":1}}
static SDispatchResult dispatchSwitch(std::string arg) {
    auto result = buildSwitchJson(arg);
    if (!result.ok)
        return SDispatchResult{.success = false, .error = result.value};
    bool ok = sendCommand(result.value);
    return ok ? SDispatchResult{} : SDispatchResult{.success = false, .error = "failed to send command"};
}

//  Swipe hook callbacks 

// Persistent socket FD kept alive for the duration of a single swipe gesture.
// Opened on swipeBegin, closed on swipeEnd.  This avoids connect+close
// overhead on every swipeUpdate (~60 Hz).
static int g_swipeFd = -1;

/// Send a line of JSON over the persistent swipe socket.
static void swipeSend(const std::string& json) {
    if (g_swipeFd < 0)
        return;
    std::string msg = json + "\n";
    // Best-effort write; if the daemon is gone we'll notice on the next event.
    [[maybe_unused]] auto _ = write(g_swipeFd, msg.c_str(), msg.size());
}

/// Open a persistent connection to the hyprgrd socket.
static bool swipeConnect() {
    g_swipeFd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (g_swipeFd < 0)
        return false;

    struct sockaddr_un addr {};
    addr.sun_family = AF_UNIX;
    std::string path = socketPath();
    strncpy(addr.sun_path, path.c_str(), sizeof(addr.sun_path) - 1);

    if (connect(g_swipeFd, reinterpret_cast<struct sockaddr*>(&addr), sizeof(addr)) < 0) {
        close(g_swipeFd);
        g_swipeFd = -1;
        return false;
    }
    return true;
}

/// Close the persistent swipe socket.
static void swipeDisconnect() {
    if (g_swipeFd >= 0) {
        close(g_swipeFd);
        g_swipeFd = -1;
    }
}

// Pointers returned by registerCallbackDynamic — prevent them from being
// garbage-collected by the Hyprland allocator while the plugin is loaded.
static SP<HOOK_CALLBACK_FN> g_swipeBeginCb;
static SP<HOOK_CALLBACK_FN> g_swipeUpdateCb;
static SP<HOOK_CALLBACK_FN> g_swipeEndCb;

/// hyprgrd:movetomonitor <direction>
///
/// Move the focused window to the monitor in the given direction.
/// <direction> is one of: left, right, up, down (case-insensitive).
///
/// Sends: {"MoveWindowToMonitor":"Right"}  (etc.)
static SDispatchResult dispatchMoveToMonitor(std::string arg) {
    auto result = buildMoveToMonitorJson(arg);
    if (!result.ok)
        return SDispatchResult{.success = false, .error = result.value};
    bool ok = sendCommand(result.value);
    return ok ? SDispatchResult{} : SDispatchResult{.success = false, .error = "failed to send command"};
}

/// hyprgrd:movetomonitorindex <n>
///
/// Move the focused window to the monitor at the given index (0-based).
///
/// Sends: {"MoveWindowToMonitorIndex":2}
static SDispatchResult dispatchMoveToMonitorIndex(std::string arg) {
    auto result = buildMoveToMonitorIndexJson(arg);
    if (!result.ok)
        return SDispatchResult{.success = false, .error = result.value};
    bool ok = sendCommand(result.value);
    return ok ? SDispatchResult{} : SDispatchResult{.success = false, .error = "failed to send command"};
}

/// hyprgrd:togglevis
///
/// Toggle a persistent overlay that shows the current grid state without
/// moving workspaces.  This sends the JSON string `"ToggleVisualizer"` to
/// the daemon; the first call shows the overlay and pins it, the second
/// call hides it again.
///
/// Note: This dispatcher takes no arguments. Hyprland will pass an empty
/// string when called without arguments, which we ignore.
static SDispatchResult dispatchToggleVis(std::string arg) {
    // Ignore any arguments (should be empty when called via keybind)
    (void)arg;
    const std::string json = "\"ToggleVisualizer\"";
    bool ok = sendCommand(json);
    return ok ? SDispatchResult{} : SDispatchResult{.success = false, .error = "failed to send command"};
}

//  Plugin entry points 

APICALL EXPORT std::string PLUGIN_API_VERSION() {
    return HYPRLAND_API_VERSION;
}

APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE handle) {
    PHANDLE = handle;

    //  Version check 
    // Compare the hash compiled into this plugin (from the Hyprland
    // headers) with the hash of the running compositor.  A mismatch
    // means the plugin was built against different headers and must not
    // be loaded — Hyprland ABI stability is not guaranteed across
    // commits.
    const std::string HASH = __hyprland_api_get_hash();
    if (HASH != __hyprland_api_get_client_hash()) {
        HyprlandAPI::addNotification(PHANDLE,
            "[hyprgrd] Mismatched Hyprland headers! Plugin was built for a "
            "different version. Dispatchers will NOT be registered.",
            CHyprColor{1.0, 0.2, 0.2, 1.0}, 10000);
        throw std::runtime_error("[hyprgrd] version mismatch: server=" + HASH +
                                 " plugin=" + __hyprland_api_get_client_hash());
    }

    //  Dispatchers (keyboard binds) 

    HyprlandAPI::addDispatcherV2(PHANDLE, "hyprgrd:go",                 dispatchGo);
    HyprlandAPI::addDispatcherV2(PHANDLE, "hyprgrd:movego",             dispatchMoveGo);
    HyprlandAPI::addDispatcherV2(PHANDLE, "hyprgrd:switch",             dispatchSwitch);
    HyprlandAPI::addDispatcherV2(PHANDLE, "hyprgrd:movetomonitor",      dispatchMoveToMonitor);
    HyprlandAPI::addDispatcherV2(PHANDLE, "hyprgrd:movetomonitorindex", dispatchMoveToMonitorIndex);
    HyprlandAPI::addDispatcherV2(PHANDLE, "hyprgrd:togglevis",          dispatchToggleVis);

    //  Swipe gesture hooks 
    // Hook into Hyprland's swipe pipeline, forward events to the
    // daemon, and cancel the default workspace-swipe behaviour only
    // when we successfully take ownership (connected to daemon).
    // Event payloads are IPointer::SSwipe*Event from Hyprland headers.

    g_swipeBeginCb = HyprlandAPI::registerCallbackDynamic(
        PHANDLE, "swipeBegin",
        [](void* /*thisptr*/, SCallbackInfo& info, std::any data) {
            uint32_t fingers = 3;
            if (auto* ev = std::any_cast<IPointer::SSwipeBeginEvent>(&data))
                fingers = ev->fingers;

            if (swipeConnect()) {
                g_swipeFingers = fingers;
                swipeSend(buildSwipeBeginJson(g_swipeFingers));
                info.cancelled = true;
            } else {
                info.cancelled = false;
            }
        });

    g_swipeUpdateCb = HyprlandAPI::registerCallbackDynamic(
        PHANDLE, "swipeUpdate",
        [](void* /*thisptr*/, SCallbackInfo& info, std::any data) {
            if (g_swipeFd < 0) {
                info.cancelled = false;
                return;
            }
            if (auto* ev = std::any_cast<IPointer::SSwipeUpdateEvent>(&data)) {
                swipeSend(buildSwipeUpdateJson(ev->fingers, ev->delta.x, ev->delta.y));
                info.cancelled = true;
                return;
            }
            // Layout mismatch: still cancel so we don't hand gesture back to Hyprland mid-swipe.
            swipeSend(buildSwipeUpdateJson(g_swipeFingers, 0.0, 0.0));
            info.cancelled = true;
        });

    g_swipeEndCb = HyprlandAPI::registerCallbackDynamic(
        PHANDLE, "swipeEnd",
        [](void* /*thisptr*/, SCallbackInfo& info, std::any /*data*/) {
            if (g_swipeFd >= 0) {
                swipeSend(buildSwipeEndJson());
                swipeDisconnect();
                info.cancelled = true;
            } else {
                info.cancelled = false;
            }
        });

    return {"hyprgrd", "Grid workspace switcher dispatchers + gesture forwarding", "hyprgrd", "0.2.0"};
}

APICALL EXPORT void PLUGIN_EXIT() {
    swipeDisconnect();
}

