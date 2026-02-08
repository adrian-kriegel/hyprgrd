// Tests for the hyprgrd plugin helper functions.
//
// These exercise the pure logic (string helpers, JSON builders, argument
// parsing) without requiring the Hyprland SDK.
//
// Build & run:
//   cd plugin && cmake -B build-test -DHYPRGRD_BUILD_TESTS=ON && cmake --build build-test
//   ./build-test/test_plugin

#include "helpers.hpp"

#include <cassert>
#include <cstdlib>
#include <iostream>
#include <string>

static int tests_run    = 0;
static int tests_passed = 0;

#define TEST(name)                                 \
    static void test_##name();                     \
    static struct Register_##name {                \
        Register_##name() {                        \
            std::cout << "  " #name "...";         \
            ++tests_run;                           \
            test_##name();                         \
            ++tests_passed;                        \
            std::cout << " ok\n";                  \
        }                                          \
    } register_##name;                             \
    static void test_##name()

#define ASSERT_EQ(a, b)                                                        \
    do {                                                                        \
        auto _a = (a);                                                          \
        auto _b = (b);                                                          \
        if (_a != _b) {                                                         \
            std::cerr << "\n    ASSERT_EQ failed at " << __FILE__ << ":"        \
                      << __LINE__ << "\n      left:  " << _a                    \
                      << "\n      right: " << _b << "\n";                       \
            std::abort();                                                       \
        }                                                                       \
    } while (0)

#define ASSERT_TRUE(expr)                                                      \
    do {                                                                        \
        if (!(expr)) {                                                          \
            std::cerr << "\n    ASSERT_TRUE failed at " << __FILE__ << ":"      \
                      << __LINE__ << "\n      " #expr "\n";                     \
            std::abort();                                                       \
        }                                                                       \
    } while (0)

#define ASSERT_FALSE(expr)                                                     \
    do {                                                                        \
        if ((expr)) {                                                           \
            std::cerr << "\n    ASSERT_FALSE failed at " << __FILE__ << ":"     \
                      << __LINE__ << "\n      " #expr "\n";                     \
            std::abort();                                                       \
        }                                                                       \
    } while (0)

// ═══════════════════════════════════════════════════════════════════════════
// trim()
// ═══════════════════════════════════════════════════════════════════════════

TEST(trim_plain_string) {
    ASSERT_EQ(trim("hello"), std::string("hello"));
}

TEST(trim_leading_spaces) {
    ASSERT_EQ(trim("   hello"), std::string("hello"));
}

TEST(trim_trailing_spaces) {
    ASSERT_EQ(trim("hello   "), std::string("hello"));
}

TEST(trim_both_sides) {
    ASSERT_EQ(trim("  hello  "), std::string("hello"));
}

TEST(trim_tabs_and_newlines) {
    ASSERT_EQ(trim("\t\nhello\r\n"), std::string("hello"));
}

TEST(trim_empty_string) {
    ASSERT_EQ(trim(""), std::string(""));
}

TEST(trim_only_whitespace) {
    ASSERT_EQ(trim("   \t\n  "), std::string(""));
}

TEST(trim_preserves_inner_spaces) {
    ASSERT_EQ(trim("  hello world  "), std::string("hello world"));
}

// ═══════════════════════════════════════════════════════════════════════════
// capitalize()
// ═══════════════════════════════════════════════════════════════════════════

TEST(capitalize_lowercase) {
    ASSERT_EQ(capitalize("right"), std::string("Right"));
}

TEST(capitalize_already_upper) {
    ASSERT_EQ(capitalize("Right"), std::string("Right"));
}

TEST(capitalize_all_upper) {
    // Only the first char is touched; the rest stay as-is.
    ASSERT_EQ(capitalize("RIGHT"), std::string("RIGHT"));
}

TEST(capitalize_empty) {
    ASSERT_EQ(capitalize(""), std::string(""));
}

TEST(capitalize_single_char) {
    ASSERT_EQ(capitalize("a"), std::string("A"));
}

// ═══════════════════════════════════════════════════════════════════════════
// isValidDirection()
// ═══════════════════════════════════════════════════════════════════════════

TEST(valid_direction_left)  { ASSERT_TRUE(isValidDirection("Left")); }
TEST(valid_direction_right) { ASSERT_TRUE(isValidDirection("Right")); }
TEST(valid_direction_up)    { ASSERT_TRUE(isValidDirection("Up")); }
TEST(valid_direction_down)  { ASSERT_TRUE(isValidDirection("Down")); }

TEST(invalid_direction_lowercase) { ASSERT_FALSE(isValidDirection("left")); }
TEST(invalid_direction_garbage)   { ASSERT_FALSE(isValidDirection("diagonal")); }
TEST(invalid_direction_empty)     { ASSERT_FALSE(isValidDirection("")); }

// ═══════════════════════════════════════════════════════════════════════════
// socketPath()
// ═══════════════════════════════════════════════════════════════════════════

TEST(socket_path_with_xdg) {
    setenv("XDG_RUNTIME_DIR", "/run/user/1000", 1);
    ASSERT_EQ(socketPath(), std::string("/run/user/1000/hyprgrd.sock"));
}

TEST(socket_path_fallback) {
    unsetenv("XDG_RUNTIME_DIR");
    ASSERT_EQ(socketPath(), std::string("/tmp/hyprgrd.sock"));
}

// ═══════════════════════════════════════════════════════════════════════════
// buildGoJson()
// ═══════════════════════════════════════════════════════════════════════════

TEST(go_json_right) {
    auto r = buildGoJson("right");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":"Right"})"));
}

TEST(go_json_left) {
    auto r = buildGoJson("left");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":"Left"})"));
}

TEST(go_json_up) {
    auto r = buildGoJson("up");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":"Up"})"));
}

TEST(go_json_down) {
    auto r = buildGoJson("down");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":"Down"})"));
}

TEST(go_json_trimmed_input) {
    auto r = buildGoJson("  right  ");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":"Right"})"));
}

TEST(go_json_already_capitalized) {
    auto r = buildGoJson("Down");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":"Down"})"));
}

TEST(go_json_invalid_direction) {
    auto r = buildGoJson("diagonal");
    ASSERT_FALSE(r.ok);
}

TEST(go_json_empty_arg) {
    auto r = buildGoJson("");
    ASSERT_FALSE(r.ok);
}

TEST(go_json_whitespace_only) {
    auto r = buildGoJson("   ");
    ASSERT_FALSE(r.ok);
}

// ═══════════════════════════════════════════════════════════════════════════
// buildMoveGoJson()
// ═══════════════════════════════════════════════════════════════════════════

TEST(movego_json_right) {
    auto r = buildMoveGoJson("right");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowAndGo":"Right"})"));
}

TEST(movego_json_left) {
    auto r = buildMoveGoJson("left");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowAndGo":"Left"})"));
}

TEST(movego_json_up) {
    auto r = buildMoveGoJson("up");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowAndGo":"Up"})"));
}

TEST(movego_json_down) {
    auto r = buildMoveGoJson("down");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowAndGo":"Down"})"));
}

TEST(movego_json_trimmed_input) {
    auto r = buildMoveGoJson("\tup\n");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowAndGo":"Up"})"));
}

TEST(movego_json_invalid) {
    auto r = buildMoveGoJson("sideways");
    ASSERT_FALSE(r.ok);
}

// ═══════════════════════════════════════════════════════════════════════════
// buildSwitchJson()
// ═══════════════════════════════════════════════════════════════════════════

TEST(switch_json_origin) {
    auto r = buildSwitchJson("0 0");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":{"x":0,"y":0}})"));
}

TEST(switch_json_positive) {
    auto r = buildSwitchJson("2 1");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":{"x":2,"y":1}})"));
}

TEST(switch_json_large_values) {
    auto r = buildSwitchJson("99 42");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":{"x":99,"y":42}})"));
}

TEST(switch_json_extra_whitespace) {
    auto r = buildSwitchJson("  3   4  ");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":{"x":3,"y":4}})"));
}

TEST(switch_json_negative_col) {
    auto r = buildSwitchJson("-1 0");
    ASSERT_FALSE(r.ok);
}

TEST(switch_json_negative_row) {
    auto r = buildSwitchJson("0 -1");
    ASSERT_FALSE(r.ok);
}

TEST(switch_json_missing_row) {
    auto r = buildSwitchJson("2");
    ASSERT_FALSE(r.ok);
}

TEST(switch_json_empty) {
    auto r = buildSwitchJson("");
    ASSERT_FALSE(r.ok);
}

TEST(switch_json_non_numeric) {
    auto r = buildSwitchJson("abc def");
    ASSERT_FALSE(r.ok);
}

TEST(switch_json_float_input) {
    // "1.5 2" — iss >> int reads 1, then tries to read ".5 2" which is not an int
    auto r = buildSwitchJson("1.5 2");
    ASSERT_FALSE(r.ok);
}

// ═══════════════════════════════════════════════════════════════════════════
// buildMoveToMonitorJson()
// ═══════════════════════════════════════════════════════════════════════════

TEST(movetomonitor_json_right) {
    auto r = buildMoveToMonitorJson("right");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitor":"Right"})"));
}

TEST(movetomonitor_json_left) {
    auto r = buildMoveToMonitorJson("left");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitor":"Left"})"));
}

TEST(movetomonitor_json_up) {
    auto r = buildMoveToMonitorJson("up");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitor":"Up"})"));
}

TEST(movetomonitor_json_down) {
    auto r = buildMoveToMonitorJson("down");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitor":"Down"})"));
}

TEST(movetomonitor_json_trimmed) {
    auto r = buildMoveToMonitorJson("  right  ");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitor":"Right"})"));
}

TEST(movetomonitor_json_invalid) {
    auto r = buildMoveToMonitorJson("diagonal");
    ASSERT_FALSE(r.ok);
}

TEST(movetomonitor_json_empty) {
    auto r = buildMoveToMonitorJson("");
    ASSERT_FALSE(r.ok);
}

// ═══════════════════════════════════════════════════════════════════════════
// buildMoveToMonitorIndexJson()
// ═══════════════════════════════════════════════════════════════════════════

TEST(movetomonitorindex_json_zero) {
    auto r = buildMoveToMonitorIndexJson("0");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":0})"));
}

TEST(movetomonitorindex_json_positive) {
    auto r = buildMoveToMonitorIndexJson("2");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":2})"));
}

TEST(movetomonitorindex_json_large) {
    auto r = buildMoveToMonitorIndexJson("42");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":42})"));
}

TEST(movetomonitorindex_json_trimmed) {
    auto r = buildMoveToMonitorIndexJson("  3  ");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":3})"));
}

TEST(movetomonitorindex_json_negative) {
    auto r = buildMoveToMonitorIndexJson("-1");
    ASSERT_FALSE(r.ok);
}

TEST(movetomonitorindex_json_empty) {
    auto r = buildMoveToMonitorIndexJson("");
    ASSERT_FALSE(r.ok);
}

TEST(movetomonitorindex_json_non_numeric) {
    auto r = buildMoveToMonitorIndexJson("abc");
    ASSERT_FALSE(r.ok);
}

TEST(movetomonitorindex_json_extra_arg) {
    auto r = buildMoveToMonitorIndexJson("1 2");
    ASSERT_FALSE(r.ok);
}

TEST(movetomonitorindex_json_float) {
    auto r = buildMoveToMonitorIndexJson("1.5");
    ASSERT_FALSE(r.ok);
}

// ═══════════════════════════════════════════════════════════════════════════
// JSON ↔ Rust serde round-trip compatibility
//
// The Rust daemon deserialises commands with serde_json.  Make sure the JSON
// the plugin produces matches exactly what serde expects.
// ═══════════════════════════════════════════════════════════════════════════

TEST(json_compat_go_all_directions) {
    // serde(Deserialize) for Command::Go(Direction::Right) expects:
    //   {"Go":"Right"}
    const std::string expected[] = {
        R"({"Go":"Left"})",
        R"({"Go":"Right"})",
        R"({"Go":"Up"})",
        R"({"Go":"Down"})",
    };
    const char* dirs[] = {"left", "right", "up", "down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildGoJson(dirs[i]);
        ASSERT_TRUE(r.ok);
        ASSERT_EQ(r.value, expected[i]);
    }
}

TEST(json_compat_movego_all_directions) {
    const std::string expected[] = {
        R"({"MoveWindowAndGo":"Left"})",
        R"({"MoveWindowAndGo":"Right"})",
        R"({"MoveWindowAndGo":"Up"})",
        R"({"MoveWindowAndGo":"Down"})",
    };
    const char* dirs[] = {"left", "right", "up", "down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildMoveGoJson(dirs[i]);
        ASSERT_TRUE(r.ok);
        ASSERT_EQ(r.value, expected[i]);
    }
}

TEST(json_compat_switch_to) {
    auto r = buildSwitchJson("5 3");
    ASSERT_TRUE(r.ok);
    // serde expects: {"SwitchTo":{"x":5,"y":3}}
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":{"x":5,"y":3}})"));
}

TEST(json_compat_movetomonitor_all_directions) {
    const std::string expected[] = {
        R"({"MoveWindowToMonitor":"Left"})",
        R"({"MoveWindowToMonitor":"Right"})",
        R"({"MoveWindowToMonitor":"Up"})",
        R"({"MoveWindowToMonitor":"Down"})",
    };
    const char* dirs[] = {"left", "right", "up", "down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildMoveToMonitorJson(dirs[i]);
        ASSERT_TRUE(r.ok);
        ASSERT_EQ(r.value, expected[i]);
    }
}

TEST(json_compat_movetomonitorindex) {
    auto r = buildMoveToMonitorIndexJson("1");
    ASSERT_TRUE(r.ok);
    // serde expects: {"MoveWindowToMonitorIndex":1}
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":1})"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Swipe event JSON builders
// ═══════════════════════════════════════════════════════════════════════════

TEST(swipe_begin_json) {
    auto j = buildSwipeBeginJson(3);
    ASSERT_EQ(j, std::string(R"({"SwipeBegin":{"fingers":3}})"));
}

TEST(swipe_begin_json_4_fingers) {
    auto j = buildSwipeBeginJson(4);
    ASSERT_EQ(j, std::string(R"({"SwipeBegin":{"fingers":4}})"));
}

TEST(swipe_end_json) {
    auto j = buildSwipeEndJson();
    ASSERT_EQ(j, std::string(R"("SwipeEnd")"));
}

TEST(swipe_update_json_positive) {
    auto j = buildSwipeUpdateJson(3, 10.5, -2.3);
    // Verify it contains the expected keys and values
    ASSERT_TRUE(j.find("\"SwipeUpdate\"") != std::string::npos);
    ASSERT_TRUE(j.find("\"fingers\":3") != std::string::npos);
    ASSERT_TRUE(j.find("\"dx\":10.5") != std::string::npos);
    ASSERT_TRUE(j.find("\"dy\":-2.3") != std::string::npos);
}

TEST(swipe_update_json_zero_deltas) {
    auto j = buildSwipeUpdateJson(3, 0.0, 0.0);
    ASSERT_TRUE(j.find("\"SwipeUpdate\"") != std::string::npos);
    ASSERT_TRUE(j.find("\"fingers\":3") != std::string::npos);
}

// ═══════════════════════════════════════════════════════════════════════════
// Dispatcher name constants
//
// The names used in addDispatcherV2() must exactly match what users put in
// hyprland.conf.  These tests document and enforce the expected names.
// ═══════════════════════════════════════════════════════════════════════════

// The dispatcher names are string literals in main.cpp.  We can't
// import them directly without pulling in Hyprland headers, but we
// can at least verify the documented names produce valid JSON for
// the expected command types — i.e. the mapping from dispatcher to
// daemon command is correct.

TEST(dispatcher_go_all_directions_roundtrip) {
    // hyprgrd:go <dir> → {"Go":"<Dir>"}
    const char* args[] = {"left", "right", "up", "down"};
    const char* expected_dirs[] = {"Left", "Right", "Up", "Down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildGoJson(args[i]);
        ASSERT_TRUE(r.ok);
        std::string expected = std::string(R"({"Go":")") + expected_dirs[i] + "\"}";
        ASSERT_EQ(r.value, expected);
    }
}

TEST(dispatcher_movego_all_directions_roundtrip) {
    // hyprgrd:movego <dir> → {"MoveWindowAndGo":"<Dir>"}
    const char* args[] = {"left", "right", "up", "down"};
    const char* expected_dirs[] = {"Left", "Right", "Up", "Down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildMoveGoJson(args[i]);
        ASSERT_TRUE(r.ok);
        std::string expected = std::string(R"({"MoveWindowAndGo":")") + expected_dirs[i] + "\"}";
        ASSERT_EQ(r.value, expected);
    }
}

TEST(dispatcher_switch_grid_position) {
    // hyprgrd:switch 0 0 → {"SwitchTo":{"x":0,"y":0}}
    auto r = buildSwitchJson("0 0");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":{"x":0,"y":0}})"));
}

TEST(dispatcher_movetomonitor_all_directions_roundtrip) {
    // hyprgrd:movetomonitor <dir> → {"MoveWindowToMonitor":"<Dir>"}
    const char* args[] = {"left", "right", "up", "down"};
    const char* expected_dirs[] = {"Left", "Right", "Up", "Down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildMoveToMonitorJson(args[i]);
        ASSERT_TRUE(r.ok);
        std::string expected = std::string(R"({"MoveWindowToMonitor":")") + expected_dirs[i] + "\"}";
        ASSERT_EQ(r.value, expected);
    }
}

TEST(dispatcher_movetomonitorindex_value) {
    // hyprgrd:movetomonitorindex 0 → {"MoveWindowToMonitorIndex":0}
    auto r = buildMoveToMonitorIndexJson("0");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":0})"));
}

// ═══════════════════════════════════════════════════════════════════════════

int main() {
    std::cout << "\nhyprgrd plugin tests\n"
              << "\n";

    // All TEST() blocks have already run via static initialisation.

    std::cout << "\n"
              << tests_passed << "/" << tests_run << " tests passed.\n\n";

    return (tests_passed == tests_run) ? 0 : 1;
}

