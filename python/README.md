# Python Demo

## Base

### Pre-requisites

1. Make sure you have latest hex_device installed. Python sometimes lazy check the version, so we need to force upgrade.
```bash
pip install hex_device --upgrade
pip install hex_device --upgrade
```

### Base Ez Control

Minimum control demo for base. Just commands base to rotate at 0.1 rad/s, and print estimated odometry. Nothing else.

#### Usage

1. Run the script. `python3 base-ez-control.py --url ws://172.18.28.201:8439`
