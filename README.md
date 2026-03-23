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
- [Rust](#rust-demo) Rust demos are the most comprehensive.
- [Python](python) Minimal Python demos to establish connection and decode protobuf messages. It is not recommended to use these raw Python demos. It is usually better to use [https://github.com/hexfellow/hex_device_python](https://github.com/hexfellow/hex_device_python)
- [C/C++](c) C/C++ demos have a relatively low priority. C/C++ demos are only offered as a demo to use websocket, kcp and protobuf. You can always port Rust demos to C/C++.


## Rust demos

### Install Rust

Go to [https://rust-lang.org/tools/install/](https://rust-lang.org/tools/install/) to install Rust.

### About IPV6
> If you don't plan to use IPV6, you can SKIP this section.

You can connect to our devices using IPV6. Making it possible to use without router, like using a single cable to connect the robot and PC. However, we assume you have basic knowledge about IPV6. If you don't, please use the robot with IPV4. We will not explain IPV6 in any detail.

Without DHCP6, devices can still have a link-local address. To use them, you have to tell OS the zone id of the interface. (The `%` symbol)

You can find the zone id of the interface by running `ip a`. In all of our examples, you have to use the number, not interface name. Things like `[fe80::500d:96ff:fee1:d60b%3]` will work, while `[fe80::500d:96ff:fee1:d60b%enp98s0]` will not.

### Demo: Finding All Local HexFellow devices using mDNS.

```bash
cargo run
```

Will output all HexFellow devices found on the network, example output:

```text
~/d/rust-robot-demos (main|✚14) $ cargo run
   Compiling robot-demos v0.1.0 (/Users/kisonhe/dev/rust-robot-demos)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.07s
     Running `target/debug/robot-demos`
Found "hexfellow-2d59f595ba5ad967.local.": {V6(ScopedIpV6 { addr: fe80::904b:4ff:fea3:aa80, scope_id: InterfaceId { name: "en0", index: 11 } })}, TxtProperties { properties: [TxtProperty {key: "MainRobotType", val: Some("RtMaverX4D")}, TxtProperty {key: "SecondaryRobotType", val: Some("RtUnknown")}] }
Found "hexfellow-2d59f595ba5ad967.local.": {V4(ScopedIpV4 { addr: 172.18.1.76 }), V6(ScopedIpV6 { addr: fe80::904b:4ff:fea3:aa80, scope_id: InterfaceId { name: "en0", index: 11 } })}, TxtProperties { properties: [TxtProperty {key: "MainRobotType", val: Some("RtMaverX4D")}, TxtProperty {key: "SecondaryRobotType", val: Some("RtUnknown")}] }
Found "hexfellow-ba59fe0cd6bb7849.local.": {V6(ScopedIpV6 { addr: fe80::48fd:70ff:fee9:b4bd, scope_id: InterfaceId { name: "en0", index: 11 } })}, TxtProperties { properties: [TxtProperty {key: "MainRobotType", val: Some("RtArmArcherY6_H1")}, TxtProperty {key: "SecondaryRobotType", val: Some("RtUnknown")}] }
Found "hexfellow-ba59fe0cd6bb7849.local.": {V4(ScopedIpV4 { addr: 172.18.6.162 }), V6(ScopedIpV6 { addr: fe80::48fd:70ff:fee9:b4bd, scope_id: InterfaceId { name: "en0", index: 11 } })}, TxtProperties { properties: [TxtProperty {key: "MainRobotType", val: Some("RtArmArcherY6_H1")}, TxtProperty {key: "SecondaryRobotType", val: Some("RtUnknown")}] }
Found "hexfellow-95cbaa28b46184bc.local.": {V6(ScopedIpV6 { addr: fe80::589f:93ff:fe16:f5be, scope_id: InterfaceId { name: "en0", index: 11 } })}, TxtProperties { properties: [TxtProperty {key: "MainRobotType", val: Some("RtUnknown")}, TxtProperty {key: "SecondaryRobotType", val: Some("RtUnknown")}] }
Found "hexfellow-95cbaa28b46184bc.local.": {V4(ScopedIpV4 { addr: 172.18.17.136 }), V6(ScopedIpV6 { addr: fe80::589f:93ff:fe16:f5be, scope_id: InterfaceId { name: "en0", index: 11 } })}, TxtProperties { properties: [TxtProperty {key: "MainRobotType", val: Some("RtUnknown")}, TxtProperty {key: "SecondaryRobotType", val: Some("RtUnknown")}] }
```

### Demo: Base Ez Control

Minimum control demo for base. Just command the base to rotate at 0.1 rad/s for 10 seconds while printing estimated odometry. In the end, deinitialize the base correctly. 

#### Usage

```bash
# KCP, ipv4. Change IP Address to your own.
cargo run --features="kcp" --example base-ez-control -- 172.18.23.92 8439
```

```bash
# KCP, ipv6. Change IP Address and Zone id to your own.
cargo run --features="kcp" --example base-ez-control -- "[fe80::500d:96ff:fee1:d60b%3]" 8439
```

```bash
# Websocket, ipv4. Change IP Address to your own.
cargo run --example base-ez-control-websocket -- 172.18.23.92 8439
```

```bash
# Websocket, ipv6. Change IP Address and Zone id to your own.
cargo run --example base-ez-control-websocket -- "[fe80::500d:96ff:fee1:d60b%3]" 8439
```

Same as above, but using websocket instead of KCP.

Remember to change the IP address to the actual IP address of the base.

### Demo: Linear Lift move

Move lift to certain percentage off the zero position. This demo is websocket only.

#### Usage

Move lift to 50% off the zero position.
```bash
# IPV4. Change IP Address to your own.
cargo run --example linear-lift-move-websocket -- 172.18.23.92 8439 0.5
```

```bash
# IPV6. Change IP Address and Zone id to your own.
cargo run --example linear-lift-move-websocket -- "[fe80::c44b:a4ff:fe06:a944%4]" 8439 0.5
```

Move lift to 50% off the zero position, at 10% of max speed. (Full speed might be a little too noisy for some lifts.)
```bash
# IPV4. Change IP Address to your own.
cargo run --example linear-lift-move-websocket -- 172.18.23.92 8439 0.5 0.1
```

```bash
# IPV6. Change IP Address and Zone id to your own.
cargo run --example linear-lift-move-websocket -- "[fe80::c44b:a4ff:fe06:a944%4]" 8439 0.5 0.1
```


Move lift to 50% off the zero position, at 10% of max speed, and calibrate before moving.
```bash
# IPV4. Change IP Address to your own.
cargo run --example linear-lift-move-websocket -- 172.18.23.92 8439 0.5 0.1 --re-calibrate
```

```bash
# IPV6. Change IP Address and Zone id to your own.
cargo run --example linear-lift-move-websocket -- "[fe80::c44b:a4ff:fe06:a944%4]" 8439 0.5 0.1 --re-calibrate
```

### Demo: Read Time Stamp from PTP Clock
> This is an advanced demo. There will be no explanation for this demo.

Read time stamp from PTP clock, and print the difference between the time stamp and the local time. You have to setup an PTP master on your network for this to work.

#### Usage

```bash
cargo run --example read-time-stamp-websocket -- 172.18.23.92 8439 /dev/ptp0
```

### Demo: Arm Ez Control

Robotic arms are complex devices. This demo will only send command `Torque:0.0` to the arm, and print the motor status.

WARNING: This will cause arm to move due to gravity. 

#### Usage

```bash
# KCP, ipv4. Change IP Address to your own.
cargo run --features="kcp" --example arm-ez-control -- 172.18.23.92 8439
```

```bash
# KCP, ipv6. Change IP Address and Zone id to your own.
cargo run --features="kcp" --example arm-ez-control -- "[fe80::500d:96ff:fee1:d60b%3]" 8439
```

### Demo: Rotational Lift move to zero position

Move rotational lift to zero position. This demo is websocket only.

#### Usage

```bash
# IPV4. Change IP Address to your own.
cargo run --example rotational-lift-move-to-zero-position-websocket -- 172.18.23.92 8439
```
