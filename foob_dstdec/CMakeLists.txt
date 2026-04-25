cmake_minimum_required(VERSION 3.10)

add_compile_options(-Wno-incompatible-pointer-types)

project(dstdec CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_INSTALL_PREFIX}/lib)

file(GLOB_RECURSE SRC_FILES sources/*.cpp)
file(GLOB_RECURSE DEF_FILES sources/*.h)
file(GLOB_RECURSE BIND_SRC_FILES binding/*.cpp)
file(GLOB_RECURSE BIND_SDEF_FILES binding/*.h)


message(STATUS "DST decoder sources: ${SRC_FILES}")

add_library(dstdecbackend STATIC ${SRC_FILES} ${DEF_FILES})
target_include_directories(dstdecbackend PUBLIC sources/)

add_library(dstdec STATIC ${BIND_SRC_FILES} ${BIND_DEF_FILES})
target_include_directories(dstdec PUBLIC sources/ binding/)


install(TARGETS dstdec DESTINATION lib)