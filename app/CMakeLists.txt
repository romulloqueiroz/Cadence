cmake_minimum_required(VERSION 3.22)
project(Cadence VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_compile_definitions(CADENCE_NATIVE_AUTOMATION=1)
include_directories("${CMAKE_CURRENT_SOURCE_DIR}/Source")
set(CMAKE_OSX_DEPLOYMENT_TARGET 11.0 CACHE STRING "Minimum macOS")

set(JUCE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/../vendor/JUCE" CACHE PATH "JUCE source directory")
if(NOT EXISTS "${JUCE_PATH}/CMakeLists.txt")
    message(FATAL_ERROR "JUCE not found at ${JUCE_PATH}. Set JUCE_PATH to a JUCE 8 checkout.")
endif()

add_subdirectory("${JUCE_PATH}" juce EXCLUDE_FROM_ALL)

find_program(CARGO_EXECUTABLE cargo REQUIRED)
set(PLATFORM_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/..")
set(CADENCE_RUST_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/rust")
if(CMAKE_OSX_ARCHITECTURES MATCHES ";")
    message(FATAL_ERROR "The Rust bridge currently requires a single macOS architecture per build")
endif()
if(CMAKE_OSX_ARCHITECTURES STREQUAL "x86_64" OR (NOT CMAKE_OSX_ARCHITECTURES AND CMAKE_HOST_SYSTEM_PROCESSOR STREQUAL "x86_64"))
    set(CADENCE_RUST_TRIPLE x86_64-apple-darwin)
else()
    set(CADENCE_RUST_TRIPLE aarch64-apple-darwin)
endif()
set(CADENCE_RUST_OUTPUT "${CADENCE_RUST_TARGET_DIR}/${CADENCE_RUST_TRIPLE}/release")
file(GLOB_RECURSE CADENCE_RUST_SOURCES CONFIGURE_DEPENDS
    "${PLATFORM_ROOT}/crates/*/src/*.rs" "${PLATFORM_ROOT}/crates/*/Cargo.toml"
    "${PLATFORM_ROOT}/crates/*/src/*.bin"
    "${PLATFORM_ROOT}/plugins/*/src/*.rs" "${PLATFORM_ROOT}/plugins/*/Cargo.toml"
    "${CMAKE_CURRENT_SOURCE_DIR}/Tests/rust-fixture/src/*.rs" "${CMAKE_CURRENT_SOURCE_DIR}/Tests/rust-fixture/Cargo.toml")
set(CADENCE_RUST_STAMP "${CADENCE_RUST_TARGET_DIR}/build.stamp")
add_custom_command(OUTPUT "${CADENCE_RUST_STAMP}"
    BYPRODUCTS "${CADENCE_RUST_OUTPUT}/libcadence_bridge.a"
    "${CADENCE_RUST_OUTPUT}/plugin-worker" "${CADENCE_RUST_OUTPUT}/libgain.dylib"
    "${CADENCE_RUST_OUTPUT}/libcadence_test_plugin.dylib"
    COMMAND ${CMAKE_COMMAND} -E env --unset=MAKEFLAGS --unset=MFLAGS "CARGO_TARGET_DIR=${CADENCE_RUST_TARGET_DIR}"
        ${CARGO_EXECUTABLE} build --release --locked --target ${CADENCE_RUST_TRIPLE}
        -p cadence-bridge -p plugin-sandbox -p gain -p cadence-test-plugin
    COMMAND ${CMAKE_COMMAND} -E touch "${CADENCE_RUST_STAMP}"
    WORKING_DIRECTORY "${PLATFORM_ROOT}"
    DEPENDS ${CADENCE_RUST_SOURCES} "${PLATFORM_ROOT}/Cargo.toml" "${PLATFORM_ROOT}/Cargo.lock"
    VERBATIM)
add_custom_target(CadenceRust DEPENDS "${CADENCE_RUST_STAMP}")
add_library(cadence_rust STATIC IMPORTED GLOBAL)
set_target_properties(cadence_rust PROPERTIES
    IMPORTED_LOCATION "${CADENCE_RUST_OUTPUT}/libcadence_bridge.a"
    INTERFACE_INCLUDE_DIRECTORIES "${PLATFORM_ROOT}/crates/cadence-bridge/include")
add_dependencies(cadence_rust CadenceRust)

function(cadence_embed_rust target)
    add_dependencies(${target} CadenceRust)
    set_property(TARGET ${target} APPEND PROPERTY LINK_DEPENDS
        "${CADENCE_RUST_OUTPUT}/plugin-worker" "${CADENCE_RUST_OUTPUT}/libgain.dylib")
    add_custom_command(TARGET ${target} POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_BUNDLE_DIR:${target}>/Contents/Helpers"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CADENCE_RUST_OUTPUT}/plugin-worker"
            "$<TARGET_BUNDLE_DIR:${target}>/Contents/Helpers/plugin-worker"
        COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_BUNDLE_DIR:${target}>/Contents/Resources/ProtocolPlugins"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CADENCE_RUST_OUTPUT}/libgain.dylib"
            "$<TARGET_BUNDLE_DIR:${target}>/Contents/Resources/ProtocolPlugins/gain.dylib"
        COMMAND /usr/bin/codesign --force --deep --sign - "$<TARGET_BUNDLE_DIR:${target}>"
        VERBATIM)
endfunction()

juce_add_plugin(Cadence
    COMPANY_NAME "Cadence"
    BUNDLE_ID "audio.cadence.player"
    PLUGIN_MANUFACTURER_CODE Cadn
    PLUGIN_CODE Cdn1
    FORMATS AU VST3 Standalone
    PRODUCT_NAME "Cadence"
    IS_SYNTH TRUE
    NEEDS_MIDI_INPUT TRUE
    NEEDS_MIDI_OUTPUT TRUE
    IS_MIDI_EFFECT FALSE
    EDITOR_WANTS_KEYBOARD_FOCUS FALSE
    AU_MAIN_TYPE kAudioUnitType_MusicDevice
    AU_SANDBOX_SAFE FALSE
    VST3_CATEGORIES Instrument Synth
    COPY_PLUGIN_AFTER_BUILD FALSE)

juce_generate_juce_header(Cadence)

target_sources(Cadence PRIVATE
    Source/Main.cpp
    Source/PluginProcessor.cpp Source/PluginLoader.cpp Source/Automation.cpp
    Source/PluginScanner.cpp
    Source/RustPluginProcessor.cpp
    Source/PluginEditor.cpp
    Source/Theme.cpp)

target_include_directories(Cadence PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/Source")
target_compile_definitions(Cadence PUBLIC
    CADENCE_HOST_AUTOMATION=1
    JUCE_PLUGINHOST_VST3=1
    JUCE_PLUGINHOST_AU=1
    JUCE_WEB_BROWSER=0
    JUCE_USE_CURL=0
    JUCE_VST3_CAN_REPLACE_VST2=0)

target_link_libraries(Cadence PRIVATE
    juce::juce_audio_processors
    juce::juce_audio_utils
    juce::juce_gui_extra
    cadence_rust
    "-framework CoreFoundation" "-framework Security" iconv resolv
    PUBLIC
    juce::juce_recommended_config_flags
    juce::juce_recommended_warning_flags)


juce_add_gui_app(CadencePreview
    PRODUCT_NAME "Cadence Preview"
    COMPANY_NAME "Cadence")
juce_generate_juce_header(CadencePreview)
target_sources(CadencePreview PRIVATE
    Source/Preview.cpp
    Source/PluginProcessor.cpp Source/PluginLoader.cpp Source/Automation.cpp
    Source/PluginScanner.cpp
    Source/RustPluginProcessor.cpp
    Source/PluginEditor.cpp
    Source/Theme.cpp)
target_compile_definitions(CadencePreview PRIVATE
    JUCE_PLUGINHOST_VST3=1
    JUCE_PLUGINHOST_AU=1
    JUCE_WEB_BROWSER=0
    JUCE_USE_CURL=0
    JUCE_STANDALONE_APPLICATION=1)
target_link_libraries(CadencePreview PRIVATE
    juce::juce_audio_processors
    juce::juce_audio_utils
    juce::juce_gui_extra
    cadence_rust
    "-framework CoreFoundation" "-framework Security" iconv resolv
    PUBLIC
    juce::juce_recommended_config_flags
    juce::juce_recommended_warning_flags)

juce_add_gui_app(CadenceScanner
    PRODUCT_NAME "Cadence Scanner"
    BUNDLE_ID "audio.cadence.scanner"
    COMPANY_NAME "Cadence"
    PLIST_TO_MERGE "<plist><dict><key>LSUIElement</key><true/></dict></plist>")
target_sources(CadenceScanner PRIVATE Source/ScanWorker.cpp Source/RustPluginProcessor.cpp Source/Theme.cpp)
target_compile_definitions(CadenceScanner PRIVATE
    JUCE_PLUGINHOST_VST3=1 JUCE_PLUGINHOST_AU=1
    JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0)
target_link_libraries(CadenceScanner PRIVATE
    juce::juce_audio_processors
    "-framework CoreAudioKit"
    cadence_rust
    "-framework CoreFoundation" "-framework Security" iconv resolv
    juce::juce_recommended_config_flags
    juce::juce_recommended_warning_flags)
cadence_embed_rust(CadenceScanner)

# Every distributable carries its scanner, including when loaded inside a DAW.
# Resolve it relative to Cadence's module, never relative to the DAW executable.
foreach(wrapper Cadence_Standalone Cadence_AU Cadence_VST3 CadencePreview)
    if(TARGET ${wrapper})
        add_dependencies(${wrapper} CadenceScanner)
        cadence_embed_rust(${wrapper})
        set_property(TARGET ${wrapper} APPEND PROPERTY LINK_DEPENDS "$<TARGET_FILE:CadenceScanner>")
        add_custom_command(TARGET ${wrapper} POST_BUILD
            COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_BUNDLE_DIR:${wrapper}>/Contents/Helpers"
            COMMAND ${CMAKE_COMMAND} -E copy_directory
                "$<TARGET_BUNDLE_DIR:CadenceScanner>"
                "$<TARGET_BUNDLE_DIR:${wrapper}>/Contents/Helpers/Cadence Scanner.app"
            COMMAND /usr/bin/codesign --force --deep --sign - "$<TARGET_BUNDLE_DIR:${wrapper}>"
            VERBATIM)
    endif()
endforeach()

include(CTest)
option(CADENCE_BUILD_KONTAKT_CAPTURE "Build the native Kontakt comparison host" OFF)
if(CADENCE_BUILD_KONTAKT_CAPTURE)
    juce_add_gui_app(CadenceKontaktCapture PRODUCT_NAME "Cadence Kontakt Capture"
        BUNDLE_ID "audio.cadence.kontakt-capture")
    juce_generate_juce_header(CadenceKontaktCapture)
    target_sources(CadenceKontaktCapture PRIVATE Tools/KontaktCapture.cpp Tools/KontaktCaptureMac.mm)
    target_compile_definitions(CadenceKontaktCapture PRIVATE
        JUCE_PLUGINHOST_VST3=1 JUCE_PLUGINHOST_AU=0 JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0
        JUCE_MODAL_LOOPS_PERMITTED=1
        CADENCE_CAPTURE_BUILD_ROOT="${CMAKE_CURRENT_BINARY_DIR}")
    target_link_libraries(CadenceKontaktCapture PRIVATE
        juce::juce_audio_utils juce::juce_gui_extra
        juce::juce_recommended_config_flags juce::juce_recommended_warning_flags)
    add_custom_command(TARGET CadenceKontaktCapture POST_BUILD
        COMMAND /usr/bin/codesign --force --deep --sign - "$<TARGET_BUNDLE_DIR:CadenceKontaktCapture>"
        VERBATIM)
endif()
if(BUILD_TESTING)
    juce_add_console_app(CadenceScanTests PRODUCT_NAME "Cadence Scan Tests")
    juce_generate_juce_header(CadenceScanTests)
    target_sources(CadenceScanTests PRIVATE
        Tests/ScanTests.cpp Tests/BridgeTests.cpp Tests/PipelineTests.cpp Tests/LoadingTests.cpp Tests/AutomationTests.cpp Tests/WorkgroupTests.cpp Tests/StressTests.cpp Source/PluginScanner.cpp Source/PluginProcessor.cpp Source/PluginLoader.cpp Source/Automation.cpp
        Source/PluginEditor.cpp Source/Theme.cpp Source/RustPluginProcessor.cpp)
    target_include_directories(CadenceScanTests PRIVATE Source)
    target_compile_definitions(CadenceScanTests PRIVATE
        JUCE_PLUGINHOST_VST3=1 JUCE_PLUGINHOST_AU=1
        JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0
        JUCE_STANDALONE_APPLICATION=1 JUCE_MODAL_LOOPS_PERMITTED=1)
    target_link_libraries(CadenceScanTests PRIVATE
        juce::juce_audio_processors juce::juce_audio_utils juce::juce_gui_extra
        cadence_rust "-framework CoreFoundation" "-framework Security" iconv resolv
        juce::juce_recommended_config_flags juce::juce_recommended_warning_flags)
    add_custom_command(TARGET CadenceScanTests POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_FILE_DIR:CadenceScanTests>/Helpers"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CADENCE_RUST_OUTPUT}/plugin-worker"
            "$<TARGET_FILE_DIR:CadenceScanTests>/Helpers/plugin-worker"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CADENCE_RUST_OUTPUT}/libgain.dylib"
            "$<TARGET_FILE_DIR:CadenceScanTests>/Helpers/gain.dylib"
        COMMAND ${CMAKE_COMMAND} -E make_directory "$<TARGET_FILE_DIR:CadenceScanTests>/Resources/ProtocolPlugins"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CADENCE_RUST_OUTPUT}/libgain.dylib"
            "$<TARGET_FILE_DIR:CadenceScanTests>/Resources/ProtocolPlugins/gain.dylib"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CADENCE_RUST_OUTPUT}/libcadence_test_plugin.dylib"
            "$<TARGET_FILE_DIR:CadenceScanTests>/Helpers/bridge-fixture.clap"
        VERBATIM)
    set_property(TARGET CadenceScanTests APPEND PROPERTY LINK_DEPENDS
        "${CADENCE_RUST_OUTPUT}/plugin-worker" "${CADENCE_RUST_OUTPUT}/libgain.dylib" "${CADENCE_RUST_OUTPUT}/libcadence_test_plugin.dylib")
    add_dependencies(CadenceScanTests Cadence_VST3 Cadence_AU)
    add_test(NAME cadence-scan-and-bridge COMMAND CadenceScanTests
        "$<TARGET_BUNDLE_DIR:Cadence_VST3>" "$<TARGET_BUNDLE_DIR:Cadence_AU>" "$<TARGET_FILE:CadenceScanner>")
    set_tests_properties(cadence-scan-and-bridge PROPERTIES TIMEOUT 90)
endif()
