// helpers.hpp — Pure helper functions used by the hyprgrd Hyprland plugin.
//
// These are split out so the test suite can exercise them without pulling in
// the Hyprland SDK headers.

#pragma once

#include <cctype>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>

//  String helpers 

/// Trim leading and trailing whitespace from a string.
inline std::string trim(const std::string& s) {
    auto start = s.find_first_not_of(" \t\r\n");
    if (start == std::string::npos)
        return "";
    auto end = s.find_last_not_of(" \t\r\n");
    return s.substr(start, end - start + 1);
}

/// Capitalize the first letter of a string ("right" → "Right").
inline std::string capitalize(std::string s) {
    if (!s.empty())
        s[0] = static_cast<char>(toupper(static_cast<unsigned char>(s[0])));
    return s;
}

/// Resolve the hyprgrd socket path ($XDG_RUNTIME_DIR/hyprgrd.sock).
inline std::string socketPath() {
    const char* runtime = getenv("XDG_RUNTIME_DIR");
    if (runtime)
        return std::string(runtime) + "/hyprgrd.sock";
    return "/tmp/hyprgrd.sock";
}

//  Command builders 

/// Result of building a dispatcher command.
struct CommandResult {
    bool        ok;
    std::string value; ///< JSON payload.
};

/// Escape a string for use as a JSON string value (backslash and quote).
inline std::string escapeJsonString(const std::string& s) {
    std::string out;
    out.reserve(s.size() + 8);
    for (unsigned char c : s) {
        if (c == '\\') out += "\\\\";
        else if (c == '"') out += "\\\"";
        else out += static_cast<char>(c);
    }
    return out;
}

/// Build the JSON for `hyprgrd:go <direction>`.
inline CommandResult buildGoJson(const std::string& arg) {
    return {true, "{\"Go\":\"" + escapeJsonString(arg) + "\"}"};
}

/// Build the JSON for `hyprgrd:movego <direction>`.
inline CommandResult buildMoveGoJson(const std::string& arg) {
    return {true, "{\"MoveWindowAndGo\":\"" + escapeJsonString(arg) + "\"}"};
}

/// Build the JSON for `hyprgrd:switch <col> <row>`.
inline CommandResult buildSwitchJson(const std::string& arg) {
    return {true, "{\"SwitchTo\":\"" + escapeJsonString(arg) + "\"}"};
}

/// Build the JSON for `hyprgrd:movetomonitor <direction>`.
inline CommandResult buildMoveToMonitorJson(const std::string& arg) {
    return {true, "{\"MoveWindowToMonitor\":\"" + escapeJsonString(arg) + "\"}"};
}

/// Build the JSON for `hyprgrd:movetomonitorindex <n>`.
inline CommandResult buildMoveToMonitorIndexJson(const std::string& arg) {
    return {true, "{\"MoveWindowToMonitorIndex\":\"" + escapeJsonString(arg) + "\"}"};
}

//  Swipe event builders (sent by the swipe hooks) 

/// Build JSON for a swipe-begin event.
///
/// Produces: `{"SwipeBegin":{"fingers":3}}`
inline std::string buildSwipeBeginJson(uint32_t fingers) {
    return "{\"SwipeBegin\":{\"fingers\":" + std::to_string(fingers) + "}}";
}

/// Build JSON for a swipe-update event.
///
/// Produces: `{"SwipeUpdate":{"fingers":3,"dx":10.5,"dy":-2.3}}`
inline std::string buildSwipeUpdateJson(uint32_t fingers, double dx, double dy) {
    char buf[256];
    snprintf(buf, sizeof(buf),
             R"({"SwipeUpdate":{"fingers":%u,"dx":%.6f,"dy":%.6f}})",
             fingers, dx, dy);
    return buf;
}

/// Build JSON for a swipe-end event.
///
/// Produces: `"SwipeEnd"`
inline std::string buildSwipeEndJson() {
    return "\"SwipeEnd\"";
}
