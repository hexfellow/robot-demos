# C Demo

This demo only works on Linux.

This demo use mongoose as the WebSocket client, nanopb as the protobuf library, and cmake as the build system.

This demo is provided as a pure C demo, for those who want to use C++, read the code and implement it in your own way, using e.g. Websocketpp and Google Protobuf.

How to build:
```bash
mkdir build
cd build
cmake ..
make
```

How to run:
```bash
./base-ez-control ws://172.18.23.92:8439
```

## Dev log

Run `gen.bash` everytime protobuf files are changed.

## Warning 

Currently there is no KCP support in C demo.

Only websocket is supported.
