cmake_minimum_required(VERSION 3.15)
project(dbt_asdl VERSION 0.3.0 LANGUAGES CXX)

include(GNUInstallDirs)

add_library(dbt_asdl INTERFACE)
add_library(dbt_asdl::dbt_asdl ALIAS dbt_asdl)

target_compile_features(dbt_asdl INTERFACE cxx_std_17)
target_include_directories(
    dbt_asdl
    INTERFACE
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
)

option(DBT_ASDL_CPP_SHIM "Compile the C++ FFI shim used by the Rust integration tests" ON)
if(DBT_ASDL_CPP_SHIM AND EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/tests/flat_cpp.cpp")
    add_library(dbt_asdl_cpp_shim OBJECT tests/flat_cpp.cpp)
    target_link_libraries(dbt_asdl_cpp_shim PRIVATE dbt_asdl::dbt_asdl)
endif()

install(
    DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/include/dbt_asdl"
    DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}"
)
