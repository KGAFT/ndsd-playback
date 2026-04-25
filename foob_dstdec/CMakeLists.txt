cmake_minimum_required(VERSION 3.10)

add_compile_options(-Wno-incompatible-pointer-types)

project(dstdec CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Force output into the install prefix so cargo:rustc-link-search finds it
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_INSTALL_PREFIX}/lib)

file(GLOB_RECURSE SRC_FILES sources/*.cpp)
file(GLOB_RECURSE DEF_FILES sources/*.h)

message(STATUS "DST decoder sources: ${SRC_FILES}")

# No hyphens in target name - some linkers choke on libdstdec-rust-ffi.a
add_library(dstdec STATIC ${SRC_FILES} ${DEF_FILES})
target_include_directories(dstdec PUBLIC sources/)
target_compile_features(dstdec PUBLIC cxx_std_17)

install(TARGETS dstdec DESTINATION lib)