# Robot Example

This repo is meant to: 
- Provide examples to help developers understand and use the [WebSocket API](https://github.com/hexfellow/proto-public-api)
- Provide a minimal example to control a robot.

This repo is NOT meant to:
- Let the developers skip reading the CODE. PLEASE UNDERSTAND THE CODE AND ITS COMMENTS.
- Demonstrate the full capabilities of the robot. For that purpose, check the community showcases.

Remember to clone this repo recursively since there are submodules in this repo.
```bash
git clone --recursive https://github.com/hexfellow/robot-demos
```

## Python demo
Go to [python](python) folder to see the python demos.

## Rust demo

### Base

Minimum control demo for base. Just command the base to rotate at 0.1 rad/s for 10 seconds while printing estimated odometry. In the end, deinitialize the base correctly. 

#### Usage

```bash
cargo run --bin base-ez-control ws://172.18.23.92:8439
```

Remember to change the IP address to the actual IP address of the base.
