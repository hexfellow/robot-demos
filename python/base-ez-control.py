#!/usr/bin/env python3
# -*- coding:utf-8 -*-
import sys
import argparse
import asyncio
import socket
from websockets.asyncio.client import connect
from generated import public_api_down_pb2, public_api_up_pb2, public_api_types_pb2

ACCEPTABLE_PROTOCOL_MAJOR_VERSION = 1;
async def receive(websocket):
    version_check = False
    try:
        async for message in websocket:
            if isinstance(message, bytes):
                api_up = public_api_up_pb2.APIUp()
                api_up.ParseFromString(message)
                if api_up.HasField("log"):
                    print(f"\033[33mWARN:Log from base: {api_up.log}\033[0m") #Having a log usually means something went boom, so lets print it.

                if api_up.protocol_major_version != ACCEPTABLE_PROTOCOL_MAJOR_VERSION:
                    if version_check == False:
                        print(f"\033[33mWARN:Protocol major version is not {ACCEPTABLE_PROTOCOL_MAJOR_VERSION}, current version: {api_up.protocol_major_version}. This might cause compatibility issues. Consider upgrading the base firmware.\033[0m")
                        version_check = True
                    continue
                # If protocol major version does not match, lets just stop printing odometry.
                if api_up.HasField('base_status'):
                    base_status = api_up.base_status

                    if base_status.HasField('estimated_odometry'):
                        odometry = base_status.estimated_odometry
                        print(f"spd=({odometry.speed_x}, {odometry.speed_y}, {odometry.speed_z})")

    except asyncio.CancelledError:
        pass

async def send(websocket):
    try:
        # Set report frequency to 50Hz; Since its a simple demo using simple_move_command, we don't need to hear from base too often.
        # If not changed, it will spam Estimated odometry at 1000Hz, which is too much for a simple demo.
        # This will only work for the current session, different sessions have independent report frequency settings.
        api_down = public_api_down_pb2.APIDown()
        api_down.set_report_frequency = public_api_types_pb2.ReportFrequency.Rf50Hz
        await websocket.send(api_down.SerializeToString()) #Send binary messages
        await asyncio.sleep(0.1)

        # Before sending move command, we need to set initialize the base first.
        api_down = public_api_down_pb2.APIDown()
        api_down.base_command.api_control_initialize = True
        await websocket.send(api_down.SerializeToString()) #Send binary messages
        await asyncio.sleep(0.1)

        while True:
            # Down, base command, command, simple_move_command, vx = 0.0, vy = 0, w = 0.1
            api_down = public_api_down_pb2.APIDown()
            api_down.base_command.simple_move_command.xyz_speed.speed_x = 0.0
            api_down.base_command.simple_move_command.xyz_speed.speed_y = 0.0
            api_down.base_command.simple_move_command.xyz_speed.speed_z = 0.1

            await websocket.send(api_down.SerializeToString()) #Send binary messages
            await asyncio.sleep(0.02)
    except asyncio.CancelledError:
        pass

# This is essential because if base lost control for a long time, it will enter protected state.
# So lets tell the base we are finishing our control session
async def send_close(websocket):
    try:
        api_down = public_api_down_pb2.APIDown()
        api_down.base_command.api_control_initialize = False
        await websocket.send(api_down.SerializeToString()) 

    except asyncio.CancelledError:
        pass

async def main():
    parser = argparse.ArgumentParser(
        description='base-ez-control',
        formatter_class=argparse.ArgumentDefaultsHelpFormatter
    )
    parser.add_argument(
        '--url',
        metavar='URL',
        default="ws://0.0.0.0:8439",
        help='WebSocket url for robot connection'
    )
    args = parser.parse_args()

    print(f"Connecting to {args.url}")
    async with connect(args.url) as websocket:
        # Remember to set tcp_nodelay to true, to get better performance.
        websocket.transport.get_extra_info('socket').setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

        tasks = [
            asyncio.create_task(receive(websocket)),
            asyncio.create_task(send(websocket))
        ]

        try:
            await asyncio.wait(tasks, timeout=10.0)
        except KeyboardInterrupt:
            print("Received Ctrl-C")
        else:
            print("10 seconds timeout reached")
        finally:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            
            await send_close(websocket)
            print("Successfully deinitialized base")

if __name__ == "__main__":
    if sys.version_info < (3, 10):
        print("This script requires Python 3.10 or higher")
        sys.exit(1)
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass