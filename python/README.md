# Python Demo

## Base

### Pre-requisites

1. **Install `protoc`**

    We highly recommend you to use **`protoc-27.1`** since we have fully tested it in both `x86_64` and `arm64` archs.

    You can use the binary installation method below to install **`protoc-27.1`**.

    ```bash
    # For Linux x86_64
    wget https://github.com/protocolbuffers/protobuf/releases/download/v27.1/protoc-27.1-linux-x86_64.zip
    sudo unzip protoc-27.1-linux-x86_64.zip -d /usr/local
    rm protoc-27.1-linux-x86_64.zip
    
    # For Linux arm64
    wget https://github.com/protocolbuffers/protobuf/releases/download/v27.1/protoc-27.1-linux-aarch_64.zip
    sudo unzip protoc-27.1-linux-aarch_64.zip -d /usr/local
    rm protoc-27.1-linux-aarch_64.zip
    
    # Verify installation
    protoc --version  # Should display libprotoc 27.1
    ```
2. **Install dependencies in your environment..**
   
    ```bash
    python3 -m pip install -r requirements.txt
    ```

### Base Ez Control

Minimum control demo for base. Just commands base to rotate at 0.1 rad/s, and print estimated odometry. Nothing else.

### Usage

1. **Compile Protocol Buffer messages**
   
    ```bash
    python3 setup.py build_py
    ```

2. **Run the demo**
   
    ```bash
    python3 base-ez-control.py --url ws://172.18.26.115:8439
    ```
    Remember to change the IP address to the actual IP address of the base.