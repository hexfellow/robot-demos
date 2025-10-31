cmake_minimum_required(VERSION 3.20)
project(robot-examples C)

add_executable(base-ez-control
    main.c
    mongoose.c
    pb_encode.c
    pb_decode.c
    pb_common.c
    generated/src/public_api_types.pb.c
    generated/src/public_api_up.pb.c
    generated/src/public_api_down.pb.c
)

# Enable PB_ENABLE_MALLOC, set macro
add_definitions(-DPB_ENABLE_MALLOC=1)
add_definitions(-DMG_IO_SIZE=65536)

target_include_directories(base-ez-control PRIVATE ${CMAKE_CURRENT_SOURCE_DIR} ${CMAKE_CURRENT_SOURCE_DIR}/generated/inc)
