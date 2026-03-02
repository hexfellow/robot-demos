cmake_minimum_required(VERSION 3.10)
project(robot_control_demo)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)


find_package(Protobuf REQUIRED)
find_package(Boost REQUIRED COMPONENTS system thread)

find_path(WEBSOCKETPP_INCLUDE_DIR NAMES websocketpp/client.hpp)
if(NOT WEBSOCKETPP_INCLUDE_DIR)
    message(FATAL_ERROR "WebSocket++ header files not found.")
endif()

include_directories(

    ${WEBSOCKETPP_INCLUDE_DIR}
    ${Protobuf_INCLUDE_DIRS}
    ${Boost_INCLUDE_DIRS}
    ${CMAKE_CURRENT_BINARY_DIR}
)

add_executable(robot_demo_ws 
    base-ez-control.cpp 
    public_api_up.pb.h 
    public_api_up.pb.cc 
    public_api_types.pb.h 
    public_api_types.pb.cc 
    public_api_down.pb.h 
    public_api_down.pb.cc
)

target_link_libraries(robot_demo_ws
    ${Protobuf_LIBRARIES}
    Boost::system
    Boost::thread
    pthread
)

add_executable(robot_demo_kcp 
    base-ez-control-kcp.cpp   # KCP 主程序
    ikcp.c         # KCP 核心库
    public_api_up.pb.h 
    public_api_up.pb.cc 
    public_api_types.pb.h 
    public_api_types.pb.cc 
    public_api_down.pb.h 
    public_api_down.pb.cc
)

target_link_libraries(robot_demo_kcp
    ${Protobuf_LIBRARIES}
    Boost::system
    Boost::thread
    pthread
)