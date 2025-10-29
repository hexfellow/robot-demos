#!/usr/bin/env python3
# -*- coding:utf-8 -*-

import argparse
import asyncio
from websockets.asyncio.client import connect
from generated import public_api_down_pb2, public_api_up_pb2, public_api_types_pb2

async def receive(websocket):
    try:
        async for message in websocket:
            if isinstance(message, bytes):
                api_up = public_api_up_pb2.APIUp()
                api_up.ParseFromString(message)

                if api_up.HasField('base_status'):
                    base_status = api_up.base_status

                    if base_status.HasField('estimated_odometry'):
                        odometry = base_status.estimated_odometry
                        print(f"spd=({odometry.speed_x}, {odometry.speed_y}, {odometry.speed_z})")
    except asyncio.CancelledError:
        pass

async def send(websocket):
    try:
        api_down = public_api_down_pb2.APIDown()
        api_down.set_report_frequency = public_api_types_pb2.ReportFrequency.Rf50Hz
        await websocket.send(api_down.SerializeToString())
        await asyncio.sleep(0.1)

        api_down = public_api_down_pb2.APIDown()
        api_down.base_command.api_control_initialize = True
        await websocket.send(api_down.SerializeToString())
        await asyncio.sleep(0.1)

        while True:
            api_down = public_api_down_pb2.APIDown()
            api_down.base_command.simple_move_command.xyz_speed.speed_x = 0.0
            api_down.base_command.simple_move_command.xyz_speed.speed_y = 0.0
            api_down.base_command.simple_move_command.xyz_speed.speed_z = 0.1

            await websocket.send(api_down.SerializeToString())
            await asyncio.sleep(0.02)
    except asyncio.CancelledError:
        pass

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
        print("Connected")

        recv_task = asyncio.create_task(receive(websocket))
        send_task = asyncio.create_task(send(websocket))

        try:
            await asyncio.gather(recv_task, send_task)
        except asyncio.CancelledError:
            print("\nReceived Ctrl-C")
            recv_task.cancel()
            send_task.cancel()
            await send_close(websocket)
            await websocket.close()
            print("Connection closed")
 
if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass