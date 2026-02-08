// Tests that the built hyprgrd.so exports every symbol Hyprland needs.
//
// Hyprland loads plugins with dlopen() and then resolves a set of required
// symbols via dlsym().  If any symbol is missing the plugin is silently
// rejected and its dispatchers are never registered — the user sees
// "dispatcher hyprgrd:go does not exist" with no other error.
//
// We cannot dlopen the .so in a test harness (it depends on Hyprland
// symbols that only exist inside the running compositor), so instead we
// read the dynamic symbol table via `nm -D` and verify every required
// symbol is present and defined (not undefined).
//
// Usage:  ./test_plugin_symbols /path/to/hyprgrd.so

#include <array>
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <string>

static int tests_run    = 0;
static int tests_failed = 0;

/// Run a shell command and capture stdout.
static std::string exec(const std::string& cmd) {
    std::array<char, 256> buf;
    std::string result;
    auto deleter = [](FILE* f) { if (f) pclose(f); };
    std::unique_ptr<FILE, decltype(deleter)> pipe(popen(cmd.c_str(), "r"), deleter);
    if (!pipe)
        return "";
    while (fgets(buf.data(), static_cast<int>(buf.size()), pipe.get()))
        result += buf.data();
    return result;
}

/// Check that a symbol is exported (defined, type T or W) in the .so.
static void check_defined_symbol(const std::string& nm_output,
                                 const char* symbol) {
    ++tests_run;
    // nm -D output format:  "0000000000004070 T symbolName"
    // We look for " T symbol" or " W symbol" (weak).
    // An undefined symbol shows as "                 U symbol".
    std::string needle_T = std::string(" T ") + symbol;
    std::string needle_W = std::string(" W ") + symbol;
    if (nm_output.find(needle_T) != std::string::npos ||
        nm_output.find(needle_W) != std::string::npos) {
        std::cout << "  OK:   " << symbol << " (exported)\n";
    } else if (nm_output.find(symbol) != std::string::npos) {
        std::cerr << "  FAIL: " << symbol
                  << " found but NOT defined (undefined reference?)\n";
        ++tests_failed;
    } else {
        std::cerr << "  FAIL: " << symbol << " NOT found in symbol table\n";
        ++tests_failed;
    }
}

/// Check that a symbol is referenced (either defined or undefined).
static void check_symbol_exists(const std::string& nm_output,
                                const char* symbol) {
    ++tests_run;
    if (nm_output.find(symbol) != std::string::npos) {
        std::cout << "  OK:   " << symbol << " (present)\n";
    } else {
        std::cerr << "  FAIL: " << symbol << " NOT found in symbol table\n";
        ++tests_failed;
    }
}

int main(int argc, char* argv[]) {
    if (argc < 2) {
        std::cerr << "usage: " << argv[0] << " <path-to-hyprgrd.so>\n";
        return 1;
    }

    const char* path = argv[1];
    std::cout << "\nhyprgrd plugin symbol tests\n"
              << "\n"
              << "inspecting: " << path << "\n\n";

    std::string nm_output = exec(std::string("nm -D ") + path + " 2>&1");
    if (nm_output.empty()) {
        std::cerr << "FATAL: failed to run nm -D on " << path << "\n";
        return 1;
    }

    //  Required DEFINED symbols 
    //
    // Hyprland resolves these by name (dlsym) when loading a plugin.
    // They must be defined (T/W) in the .so, not just referenced.
    //
    //  pluginAPIVersion            — returns HYPRLAND_API_VERSION string
    //  pluginInit                  — called to initialise the plugin
    //  pluginExit                  — called on unload
    //  __hyprland_api_get_client_hash
    //      — version fingerprint compiled into the plugin from
    //        PluginAPI.hpp.  Compared against the server's
    //        __hyprland_api_get_hash() to detect mismatches.
    //
    //        This is an `inline` function from the header.  If the
    //        plugin never references it, the compiler omits it and
    //        Hyprland silently rejects the plugin.  The version check
    //        in PLUGIN_INIT forces the compiler to emit it.

    std::cout << " Required exported symbols \n";
    check_defined_symbol(nm_output, "pluginAPIVersion");
    check_defined_symbol(nm_output, "pluginInit");
    check_defined_symbol(nm_output, "pluginExit");
    check_defined_symbol(nm_output, "__hyprland_api_get_client_hash");

    //  Hyprland API references 
    //
    // These symbols are provided by Hyprland at runtime (undefined in
    // the plugin).  Their presence confirms the plugin actually calls
    // the dispatcher registration API.

    std::cout << "\n Expected Hyprland API references \n";
    check_symbol_exists(nm_output, "addDispatcherV2");
    check_symbol_exists(nm_output, "addNotification");
    check_symbol_exists(nm_output, "__hyprland_api_get_hash");

    //  Summary 
    std::cout << "\n\n"
              << (tests_run - tests_failed) << "/" << tests_run
              << " symbol checks passed.\n\n";

    return tests_failed > 0 ? 1 : 0;
}
