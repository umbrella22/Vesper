cmake_minimum_required(VERSION 3.16)
project(vesper_gain_cpp LANGUAGES CXX)

set(VESPER_PLUGIN_HEADERS "" CACHE PATH "Directory containing vesper_plugin.h")
if(NOT EXISTS "${VESPER_PLUGIN_HEADERS}/vesper_plugin.h")
    message(FATAL_ERROR "Set VESPER_PLUGIN_HEADERS to the directory containing the native plugin ABI header")
endif()

add_library(vesper_gain_cpp SHARED gain.cpp)
target_include_directories(vesper_gain_cpp PRIVATE "${VESPER_PLUGIN_HEADERS}")
target_compile_definitions(vesper_gain_cpp PRIVATE VESPER_PLUGIN_BUILD)
target_compile_features(vesper_gain_cpp PRIVATE cxx_std_17)
set_target_properties(vesper_gain_cpp PROPERTIES CXX_VISIBILITY_PRESET hidden CXX_EXTENSIONS OFF)
