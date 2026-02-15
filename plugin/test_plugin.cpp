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
// buildGoJson() — plugin forwards raw arg; daemon parses/validates.
// ═══════════════════════════════════════════════════════════════════════════

TEST(go_json_forwards_arg) {
    auto r = buildGoJson("right");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":"right"})"));
}

TEST(go_json_trimmed) {
    auto r = buildGoJson("  left  ");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":"left"})"));
}

TEST(go_json_forwards_any_string) {
    auto r = buildGoJson("diagonal");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":"diagonal"})"));
}

TEST(go_json_empty_forwarded) {
    auto r = buildGoJson("");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"Go":""})"));
}

// ═══════════════════════════════════════════════════════════════════════════
// buildMoveGoJson() — forwards raw arg.
// ═══════════════════════════════════════════════════════════════════════════

TEST(movego_json_forwards_arg) {
    auto r = buildMoveGoJson("right");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowAndGo":"right"})"));
}

TEST(movego_json_trimmed) {
    auto r = buildMoveGoJson("\tup\n");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowAndGo":"up"})"));
}

// ═══════════════════════════════════════════════════════════════════════════
// buildSwitchJson() — forwards raw arg; daemon parses "col row".
// ═══════════════════════════════════════════════════════════════════════════

TEST(switch_json_forwards_arg) {
    auto r = buildSwitchJson("0 0");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":"0 0"})"));
}

TEST(switch_json_trimmed) {
    auto r = buildSwitchJson("  2   1  ");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":"2   1"})"));
}

TEST(switch_json_forwards_any_string) {
    auto r = buildSwitchJson("abc def");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":"abc def"})"));
}

// ═══════════════════════════════════════════════════════════════════════════
// buildMoveToMonitorJson() — forwards raw arg.
// ═══════════════════════════════════════════════════════════════════════════

TEST(movetomonitor_json_forwards_arg) {
    auto r = buildMoveToMonitorJson("right");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitor":"right"})"));
}

TEST(movetomonitor_json_up) {
    auto r = buildMoveToMonitorJson("up");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitor":"up"})"));
}

TEST(movetomonitor_json_trimmed) {
    auto r = buildMoveToMonitorJson("  right  ");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitor":"right"})"));
}

// ═══════════════════════════════════════════════════════════════════════════
// buildMoveToMonitorIndexJson()
// ═══════════════════════════════════════════════════════════════════════════

TEST(movetomonitorindex_json_forwards_arg) {
    auto r = buildMoveToMonitorIndexJson("0");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":"0"})"));
}

TEST(movetomonitorindex_json_trimmed) {
    auto r = buildMoveToMonitorIndexJson("  3  ");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":"3"})"));
}

TEST(movetomonitorindex_json_forwards_any_string) {
    auto r = buildMoveToMonitorIndexJson("abc");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":"abc"})"));
}

// ═══════════════════════════════════════════════════════════════════════════
// JSON ↔ Rust serde round-trip compatibility
//
// The Rust daemon deserialises commands with serde_json.  Make sure the JSON
// the plugin produces matches exactly what serde expects.
// ═══════════════════════════════════════════════════════════════════════════

TEST(json_compat_go_forwards_daemon_parses) {
    const char* dirs[] = {"left", "right", "up", "down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildGoJson(dirs[i]);
        ASSERT_TRUE(r.ok);
        ASSERT_EQ(r.value, std::string(R"({"Go":")") + dirs[i] + "\"}");
    }
}

TEST(json_compat_switch_to_forwards_string) {
    auto r = buildSwitchJson("5 3");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":"5 3"})"));
}

TEST(json_compat_movetomonitorindex_forwards_string) {
    auto r = buildMoveToMonitorIndexJson("1");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":"1"})"));
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

TEST(dispatcher_go_forwards_arg) {
    const char* args[] = {"left", "right", "up", "down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildGoJson(args[i]);
        ASSERT_TRUE(r.ok);
        ASSERT_EQ(r.value, std::string(R"({"Go":")") + args[i] + "\"}");
    }
}

TEST(dispatcher_movego_forwards_arg) {
    const char* args[] = {"left", "right", "up", "down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildMoveGoJson(args[i]);
        ASSERT_TRUE(r.ok);
        ASSERT_EQ(r.value, std::string(R"({"MoveWindowAndGo":")") + args[i] + "\"}");
    }
}

TEST(dispatcher_switch_forwards_arg) {
    auto r = buildSwitchJson("0 0");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"SwitchTo":"0 0"})"));
}

TEST(dispatcher_movetomonitor_forwards_arg) {
    const char* args[] = {"left", "right", "up", "down"};
    for (int i = 0; i < 4; ++i) {
        auto r = buildMoveToMonitorJson(args[i]);
        ASSERT_TRUE(r.ok);
        ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitor":")") + args[i] + "\"}");
    }
}

TEST(dispatcher_movetomonitorindex_forwards_arg) {
    auto r = buildMoveToMonitorIndexJson("0");
    ASSERT_TRUE(r.ok);
    ASSERT_EQ(r.value, std::string(R"({"MoveWindowToMonitorIndex":"0"})"));
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

