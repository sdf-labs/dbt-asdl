cmake_minimum_required(VERSION 3.15)

# Cargo.toml is the single source of truth for the package version, so that
# release-plz keeps the C++ package version in step with the crate version.
set(DBT_ASDL_CARGO_TOML "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml")
if(NOT EXISTS "${DBT_ASDL_CARGO_TOML}")
    message(FATAL_ERROR "DBT_ASDL: missing ${DBT_ASDL_CARGO_TOML}, which holds the package version")
endif()
file(READ "${DBT_ASDL_CARGO_TOML}" _dbt_asdl_cargo_toml)
string(FIND "${_dbt_asdl_cargo_toml}" "[package]" _dbt_asdl_package_offset)
if(_dbt_asdl_package_offset EQUAL -1)
    message(FATAL_ERROR "DBT_ASDL: no [package] table in ${DBT_ASDL_CARGO_TOML}")
endif()
string(SUBSTRING "${_dbt_asdl_cargo_toml}" ${_dbt_asdl_package_offset} -1 _dbt_asdl_package_table)
string(REGEX MATCH "\nversion[ \t]*=[ \t]*\"([0-9]+\\.[0-9]+\\.[0-9]+)[^\"]*\"" _dbt_asdl_version_match "${_dbt_asdl_package_table}")
if(NOT _dbt_asdl_version_match)
    message(FATAL_ERROR "DBT_ASDL: cannot read the package version from ${DBT_ASDL_CARGO_TOML}")
endif()
set(DBT_ASDL_VERSION "${CMAKE_MATCH_1}")

project(dbt_asdl VERSION "${DBT_ASDL_VERSION}" LANGUAGES CXX)

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
    set(DBT_ASDL_GEN_DIR "${CMAKE_CURRENT_BINARY_DIR}/generated")
    set(DBT_ASDL_GEN_FLAT_HPP "${DBT_ASDL_GEN_DIR}/gen_flat.hpp")
    if(NOT EXISTS "${DBT_ASDL_GEN_FLAT_HPP}")
        execute_process(
            COMMAND cargo run --quiet --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
                -- --cpp-flat
                "${CMAKE_CURRENT_SOURCE_DIR}/tests/fixtures/mini_sql.asdl"
                "${DBT_ASDL_GEN_FLAT_HPP}"
            RESULT_VARIABLE _dbt_asdl_gen_rc
        )
    endif()
    add_custom_command(
        OUTPUT "${DBT_ASDL_GEN_FLAT_HPP}"
        COMMAND cargo run --quiet --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
            -- --cpp-flat
            "${CMAKE_CURRENT_SOURCE_DIR}/tests/fixtures/mini_sql.asdl"
            "${DBT_ASDL_GEN_FLAT_HPP}"
        DEPENDS
            "${CMAKE_CURRENT_SOURCE_DIR}/tests/fixtures/mini_sql.asdl"
            "${CMAKE_CURRENT_SOURCE_DIR}/src/flat.rs"
            "${CMAKE_CURRENT_SOURCE_DIR}/src/main.rs"
        VERBATIM
    )
    add_library(dbt_asdl_cpp_shim OBJECT tests/flat_cpp.cpp "${DBT_ASDL_GEN_FLAT_HPP}")
    target_include_directories(dbt_asdl_cpp_shim PRIVATE "${DBT_ASDL_GEN_DIR}")
    target_link_libraries(dbt_asdl_cpp_shim PRIVATE dbt_asdl::dbt_asdl)
endif()

install(
    DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/include/dbt_asdl"
    DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}"
)
