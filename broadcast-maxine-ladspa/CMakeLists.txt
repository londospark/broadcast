cmake_minimum_required(VERSION 3.20)
project(maxine_ladspa C)

set(CMAKE_C_STANDARD 11)

# ── Find CUDA (provides nvcc and cuda runtime headers) ──────────────────
find_package(CUDAToolkit REQUIRED)

# ── NVIDIA Audio Effects SDK ─────────────────────────────────────────────
# Set NVAFX_SDK to the root of the unpacked Maxine SDK, e.g.:
#   cmake -DNVAFX_SDK=/opt/nvidia-afx-sdk ..
set(NVAFX_SDK "" CACHE PATH "Path to NVIDIA Audio Effects SDK root")

if(NOT NVAFX_SDK)
    # Common install locations on CachyOS/Arch
    foreach(candidate
        "/opt/nvidia-maxine-sdk"
        "/usr/lib/nvidia/nvidia-afx-sdk"
        "$ENV{HOME}/nvidia-maxine-sdk"
    )
        if(EXISTS "${candidate}/include/nvAFX.h")
            set(NVAFX_SDK "${candidate}")
            break()
        endif()
    endforeach()
endif()

if(NOT NVAFX_SDK OR NOT EXISTS "${NVAFX_SDK}/include/nvAFX.h")
    message(FATAL_ERROR
        "NVIDIA Audio Effects SDK not found.\n"
        "Download from https://developer.nvidia.com/nvidia-audio-effects-sdk\n"
        "Then set -DNVAFX_SDK=/path/to/sdk")
endif()

message(STATUS "Using NVAFX SDK at: ${NVAFX_SDK}")

# ── LADSPA headers ───────────────────────────────────────────────────────
find_path(LADSPA_INCLUDE_DIR ladspa.h
    HINTS /usr/include /usr/local/include)
if(NOT LADSPA_INCLUDE_DIR)
    message(FATAL_ERROR "ladspa.h not found — install ladspa-sdk: pacman -S ladspa")
endif()

# ── Build the shared library ─────────────────────────────────────────────
add_library(maxine_ladspa SHARED src/maxine_ladspa.c)

target_include_directories(maxine_ladspa PRIVATE
    ${LADSPA_INCLUDE_DIR}
    ${NVAFX_SDK}/include
    ${CUDAToolkit_INCLUDE_DIRS}
)

target_link_directories(maxine_ladspa PRIVATE
    ${NVAFX_SDK}/lib/x64
    ${CUDAToolkit_LIBRARY_DIR}
)

target_link_libraries(maxine_ladspa PRIVATE
    NvAFX
    cudart
    ${CMAKE_DL_LIBS}
)

# Strip 'lib' prefix so the LADSPA host sees it as 'maxine_ladspa.so'
set_target_properties(maxine_ladspa PROPERTIES
    PREFIX ""
    OUTPUT_NAME "maxine_ladspa"
)

# ── Install ──────────────────────────────────────────────────────────────
include(GNUInstallDirs)
install(TARGETS maxine_ladspa
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}/ladspa)
