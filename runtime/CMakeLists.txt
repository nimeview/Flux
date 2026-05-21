cmake_minimum_required(VERSION 3.20)
project(runtime)

set(CMAKE_CXX_STANDARD 20)

include_directories(.)
include_directories(..)

add_executable(runtime
        main.cpp
        ../stdlib/std.cpp
        ffi/ffi.cpp
        ffi/ffi.h
        graph/graph.cpp
        graph/graph.h
        memory/memory.cpp
        memory/memory.h
        objects/objects.cpp
        objects/objects.h
        vm/vm.cpp
        vm/vm.h
        scheduler/scheduler.cpp
        scheduler/scheduler.h
        runtime_api/runtime_api.cpp
        runtime_api/runtime_api.h)

enable_testing()
add_executable(runtime_scheduler_tests
        tests/scheduler_tests.cpp
        graph/graph.cpp
        scheduler/scheduler.cpp
        ../stdlib/std.cpp)
add_test(NAME runtime_scheduler_tests COMMAND runtime_scheduler_tests)
