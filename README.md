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
- [Rust](#rust-demo) The demo for the Rust language is the highest priority in development and the most comprehensive.
- [Python](python) 
- [C](c) C demos have a relatively low priority. C demos are only offered as a demo to use websocket, kcp and protobuf. You can always port Rust demos to C.


## Rust demo

Finding HexFellow devices using mDNS.

```bash
cargo run --bin robot-demos
```

Will output all HexFellow devices found on the network, example output:

```text
kisonhe@HEXBeast1 ~/robot-demos (main) [SIGINT]> cargo run --release --bin robot-demos
    Finished `release` profile [optimized] target(s) in 0.11s
     Running `target/release/robot-demos`
Found HexFellow Device "hexfellow-c2149b7bf5fb9a49.local.": {V6(ScopedIpV6 { addr: fe80::500d:96ff:fee1:d60b, scope_id: InterfaceId { name: "enp98s0", index: 4 } }), V4(ScopedIpV4 { addr: 172.18.9.145 })}
Found HexFellow Device "hexfellow-390be859a9d694d8.local.": {V4(ScopedIpV4 { addr: 172.18.7.230 }), V6(ScopedIpV6 { addr: fe80::1089:c3ff:fe97:9e5f, scope_id: InterfaceId { name: "enp98s0", index: 4 } })}
Found HexFellow Device "hexfellow-4ede314e9b0023b3.local.": {V4(ScopedIpV4 { addr: 172.18.6.42 }), V6(ScopedIpV6 { addr: fe80::ac05:9fff:feeb:f87f, scope_id: InterfaceId { name: "enp98s0", index: 4 } })}
```

### IPV6
> If you don't plan to use IPV6, you can SKIP this section.

You can connect to our devices using IPV6. Making it possible to use without router, like using a single cable to connect the robot and PC. However, we assume you have basic knowledge about IPV6. If you don't, please use the robot with IPV4. We will not explain IPV6 in any detail.

Without DHCP6, devices can still have a link-local address. To use them, you have to tell OS the zone id of the interface. (The `%` symbol)

You can find the zone id of the interface by running `ip a`. In all of our examples, you have to use the number, not interface name. Things like `[fe80::500d:96ff:fee1:d60b%3]` will work, while `[fe80::500d:96ff:fee1:d60b%enp98s0]` will not.

### Base

Minimum control demo for base. Just command the base to rotate at 0.1 rad/s for 10 seconds while printing estimated odometry. In the end, deinitialize the base correctly. 

#### Usage

```bash
# KCP, ipv4. Change IP Address to your own.
cargo run --bin base-ez-control -- 172.18.23.92 8439
```

```bash
# KCP, ipv6. Change IP Address and Zone id to your own.
cargo run --bin base-ez-control -- "[fe80::500d:96ff:fee1:d60b%3]" 8439
```

```bash
# Websocket, ipv4. Change IP Address to your own.
cargo run --bin base-ez-control-websocket -- 172.18.23.92 8439
```

```bash
# Websocket, ipv6. Change IP Address and Zone id to your own.
cargo run --bin base-ez-control-websocket -- "[fe80::500d:96ff:fee1:d60b%3]" 8439
```

Same as above, but using websocket instead of KCP.

Remember to change the IP address to the actual IP address of the base.

### Linear Lift

Move lift to certain percentage off the zero position.

#### Usage

Move lift to 50% off the zero position.
```bash
# IPV4. Change IP Address to your own.
cargo run --bin linear-lift-move-websocket -- 172.18.23.92 8439 0.5
```

```bash
# IPV6. Change IP Address and Zone id to your own.
cargo run --bin linear-lift-move-websocket -- "[fe80::c44b:a4ff:fe06:a944%4]" 8439 0.5
```
