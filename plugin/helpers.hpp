// helpers.hpp — Pure helper functions used by the hyprgrd Hyprland plugin.
//
// These are split out so the test suite can exercise them without pulling in
// the Hyprland SDK headers.

#pragma once

#include <cctype>
#include <cstdlib>
#include <sstream>
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
//
// Each builder validates its input and returns either a JSON payload string
// or an error message.  The dispatchers in main.cpp wrap these results into
// the Hyprland-specific SDispatchResult type.

/// Result of building a dispatcher command.
struct CommandResult {
    bool        ok;
    std::string value; ///< JSON payload if `ok`, error message otherwise.
};

/// Validate a direction string and return `true` when it is one of the four
/// accepted values (after trim + capitalize).
inline bool isValidDirection(const std::string& dir) {
    return dir == "Left" || dir == "Right" || dir == "Up" || dir == "Down";
}

/// Build the JSON for `hyprgrd:go <direction>`.
///
/// Produces: `{"Go":"Right"}`  (etc.)
inline CommandResult buildGoJson(const std::string& arg) {
    std::string dir = capitalize(trim(arg));
    if (!isValidDirection(dir))
        return {false, "invalid direction: " + arg};
    return {true, "{\"Go\":\"" + dir + "\"}"};
}

/// Build the JSON for `hyprgrd:movego <direction>`.
///
/// Produces: `{"MoveWindowAndGo":"Right"}`  (etc.)
inline CommandResult buildMoveGoJson(const std::string& arg) {
    std::string dir = capitalize(trim(arg));
    if (!isValidDirection(dir))
        return {false, "invalid direction: " + arg};
    return {true, "{\"MoveWindowAndGo\":\"" + dir + "\"}"};
}

/// Build the JSON for `hyprgrd:switch <col> <row>`.
///
/// Produces: `{"SwitchTo":{"x":2,"y":1}}`
inline CommandResult buildSwitchJson(const std::string& arg) {
    std::istringstream iss(trim(arg));
    int col = 0, row = 0;
    if (!(iss >> col >> row) || col < 0 || row < 0)
        return {false, "expected: <col> <row> (non-negative integers)"};
    return {true, "{\"SwitchTo\":{\"x\":" + std::to_string(col) +
                   ",\"y\":" + std::to_string(row) + "}}"};
}

/// Build the JSON for `hyprgrd:movetomonitor <direction>`.
///
/// Produces: `{"MoveWindowToMonitor":"Right"}`  (etc.)
inline CommandResult buildMoveToMonitorJson(const std::string& arg) {
    std::string dir = capitalize(trim(arg));
    if (!isValidDirection(dir))
        return {false, "invalid direction: " + arg};
    return {true, "{\"MoveWindowToMonitor\":\"" + dir + "\"}"};
}

/// Build the JSON for `hyprgrd:movetomonitorindex <n>`.
///
/// Produces: `{"MoveWindowToMonitorIndex":2}`
inline CommandResult buildMoveToMonitorIndexJson(const std::string& arg) {
    std::istringstream iss(trim(arg));
    int idx = -1;
    if (!(iss >> idx) || idx < 0)
        return {false, "expected: <n> (non-negative integer)"};
    // Make sure there's nothing extra after the number.
    std::string leftover;
    if (iss >> leftover)
        return {false, "expected a single non-negative integer"};
    return {true, "{\"MoveWindowToMonitorIndex\":" + std::to_string(idx) + "}"};
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
    // Use enough precision for sub-pixel deltas.
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

