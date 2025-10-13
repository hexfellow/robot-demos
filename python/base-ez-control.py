#!/usr/bin/env python3
# -*- coding:utf-8 -*-
################################################################
# Copyright 2025 Jecjune. All rights reserved.
# Author: Jecjune zejun.chen@hexfellow.com
# Date  : 2025-8-1
################################################################

# A Simple Test for HexDeviceApi

import sys
import argparse
import numpy as np
import logging
import hex_device
from hex_device import HexDeviceApi
import time
from hex_device.chassis import Chassis
from hex_device.motor_base import CommandType
from hex_device.arm_archer import ArmArcher
from hex_device.motor_base import MitMotorCommand
from hex_device.hands import Hands
from hex_device.motor_base import public_api_types_pb2

def main():
    # Parse command line arguments
    parser = argparse.ArgumentParser(
        description='Hexapod robotic arm trajectory planning and execution test',
        formatter_class=argparse.ArgumentDefaultsHelpFormatter
    )
    parser.add_argument(
        '--url', 
        metavar='URL',
        default="ws://172.18.28.201:8439",
        help='WebSocket URL for HEX device connection'
    )
    args = parser.parse_args()
    
    # Init HexDeviceApi
    api = HexDeviceApi(ws_url=args.url, control_hz=250)

    try:
        while True:
            if api.is_api_exit():
                print("Public API has exited.")
                break
            else:                
                for device in api.device_list:
                    if isinstance(device, Chassis):
                        if device.has_new_data():
                            print(f"vehicle position: {device.get_vehicle_position()}")
                            device.start() # Actully only have to call once, but lets be lazy and call it every time XD
                            device.set_vehicle_speed(0.0, 0.0, 0.1)
            time.sleep(0.002)

    except KeyboardInterrupt:
        print("Received Ctrl-C.")
        for device in api.device_list:
            if isinstance(device, Chassis):
                device.stop() # Saves base from timeout error
        time.sleep(0.1)
        api.close()
    finally:
        pass

    print("Resources have been cleaned up.")
    exit(0)


if __name__ == "__main__":
    main()