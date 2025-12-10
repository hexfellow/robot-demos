# Robot Example

## About this repo

This repo is meant to: 
- Provide examples to help developers understand and use the [WebSocket API](https://github.com/hexfellow/proto-public-api)
- Provide a minimal example to control a robot.

This repo is NOT meant to:
- Let the developers skip reading the CODE. PLEASE UNDERSTAND THE CODE AND ITS COMMENTS YOURSELF FIRST. We will not explain any code in this repo. 
- Demonstrate the full capabilities of the robot. For that purpose, check the community showcases.


## Protocol differences

You might see demos named as `xxx` and `xxx-websocket`. The difference is the protocol used to transport the binary messages.

Currently there are two ways of transporting these binary messages:
- Websocket: Easy to implement, but not the lowest latency.
- KCP: More complex to implement, but can has lowest latency.

In most cases, websocket is good enough. **If you didn't encounter any latency issues, just use websocket.**

## Getting started

Clone this repo recursively `git clone --recursive https://github.com/hexfellow/robot-demos`.

The repo contains demos written in following languages:
- [Python](python)
- [C](c)
- [Rust](#rust-demo)

## Rust demo

### Base

Minimum control demo for base. Just command the base to rotate at 0.1 rad/s for 10 seconds while printing estimated odometry. In the end, deinitialize the base correctly. 

#### Usage

```bash
# KCP, ipv4
cargo run --bin base-ez-control -- 172.18.23.92:8439
```

```bash
# Websocket, ipv4
cargo run --bin base-ez-control-websocket -- ws://172.18.23.92:8439
```

```bash
# Websocket, ipv6
cargo run --bin base-ez-control-websocket -- ws://[fe80::500d:96ff:fee1:d60b%3]:8439
```

Same as above, but using websocket instead of KCP.

Remember to change the IP address to the actual IP address of the base.

### Linear Lift

Move lift to certain percentage off the zero position.

#### Usage

Move lift to 50% off the zero position.
```bash
cargo run --bin linear-lift-move -- 172.18.23.92:8439 0.5
```
