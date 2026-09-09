cmake_minimum_required(VERSION 3.24)
project(urban_ipc LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Network RemoteObjects)

add_executable(urban-ipc urban-ipc.cpp)
target_link_libraries(urban-ipc PRIVATE
    Qt6::Core
    Qt6::Network
    Qt6::RemoteObjects
    iphlpapi
    ws2_32
)

set_target_properties(urban-ipc PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
    RUNTIME_OUTPUT_DIRECTORY_RELEASE "${CMAKE_BINARY_DIR}/bin"
)
