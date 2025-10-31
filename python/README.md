# Python Demo

## Base

### Pre-requisites

#### **Install `protoc`**

1. Install protoc from package manager (Recommended only for Debian13/Ubuntu24.04)
    ```bash
    sudo apt install protobuf-compiler
    ```

2. Install protoc from Github Releases (Recommended Ubuntu22.04 and below)
    
    Just choose a suitable version and install it. Here below is an example of installing `protoc-27.1`. 

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
    protoc --version # Should be or more than 3.21.12
    ```
#### Install python dependencies
   
```bash
python3 -m pip install -r requirements.txt
```

### Base Ez Control

Minimum control demo for base. Just command the base to rotate at 0.1 rad/s for 10 seconds while printing estimated odometry. In the end, deinitialize the base correctly.


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