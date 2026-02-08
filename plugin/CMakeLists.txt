cmake_minimum_required(VERSION 3.19)
project(hyprgrd-plugin
    VERSION     0.1.0
    DESCRIPTION "Hyprland plugin — native dispatchers for hyprgrd"
    LANGUAGES   CXX
)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

#  Locate Hyprland headers 
# The plugin is dlopen'd into Hyprland at runtime so it only needs headers,
# not link-time libraries.  We look for the PluginAPI header directly.
find_path(HYPRLAND_INCLUDE_DIR
    NAMES hyprland/src/plugins/PluginAPI.hpp
    HINTS
        ${HYPRLAND_HEADERS}              # explicit override
        ENV HYPRLAND_HEADERS
    REQUIRED
)

#  Shared library (.so) 
add_library(hyprgrd-plugin SHARED main.cpp)

set_target_properties(hyprgrd-plugin PROPERTIES
    OUTPUT_NAME "hyprgrd"
)

target_compile_options(hyprgrd-plugin PRIVATE
    -Wall -Wextra -O2
)

target_include_directories(hyprgrd-plugin PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${HYPRLAND_INCLUDE_DIR}
)

target_compile_definitions(hyprgrd-plugin PRIVATE
    WLR_USE_UNSTABLE
)

#  Install 
include(GNUInstallDirs)
install(TARGETS hyprgrd-plugin
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}/
)

#  Tests 
# Build with: cmake -B build -DHYPRGRD_BUILD_TESTS=ON && cmake --build build
# Run  with: ./build/test_plugin    (or: ctest --test-dir build)
option(HYPRGRD_BUILD_TESTS "Build plugin unit tests" OFF)

if(HYPRGRD_BUILD_TESTS)
    enable_testing()

    #  Unit tests (helpers / JSON builders) 
    add_executable(test_plugin test_plugin.cpp)
    target_compile_options(test_plugin PRIVATE -Wall -Wextra)
    # helpers.hpp is in the same directory — no extra include dirs needed.
    add_test(NAME plugin_tests COMMAND test_plugin)

    #  Symbol export tests 
    # Verify the .so exports every symbol that Hyprland expects when
    # loading a plugin via dlopen + dlsym.  A missing symbol (especially
    # __hyprland_api_get_client_hash) causes Hyprland to silently
    # reject the plugin and none of the dispatchers will be registered.
    add_executable(test_plugin_symbols test_plugin_symbols.cpp)
    target_compile_options(test_plugin_symbols PRIVATE -Wall -Wextra)
    add_test(NAME plugin_symbol_tests COMMAND test_plugin_symbols
             $<TARGET_FILE:hyprgrd-plugin>)
endif()
