# Testing Plan

This project contains demos that target different transports and operating systems. Run the checks below from the repository root after installing Rust with `rustup`.

## Toolchain

```bash
. "$HOME/.cargo/env"
cargo --version
rustc --version
```

The current verified local toolchain is:

```text
cargo 1.96.0
rustc 1.96.0
```

## Automated Checks

Format only touched Rust files unless the whole repository is being reformatted intentionally:

```bash
rustfmt --edition 2021 --check examples/base-gui-websocket.rs
```

Compile and test the default Rust targets:

```bash
cargo test --lib --bins
cargo check --examples
```

Compile the GUI example directly:

```bash
cargo check --example base-gui-websocket
```

Compile optional non-Linux feature coverage on macOS:

```bash
cargo check --features kcp --examples
cargo check --features tui --example zenoh-read
cargo check --all-targets --features kcp,tui
```

`cargo check --all-targets --all-features` is expected to fail on macOS because the `socketcan` feature depends on Linux-only `socketcan`/`libudev`. Run that command on Linux instead.

## GUI Hardware Integration Test

Start the GUI bridge with the real base IP and port:

```bash
cargo run --example base-gui-websocket -- <robot-ip-or-ipv6> 8439
```

Open `http://127.0.0.1:8080` in a browser.

Verify status visualization:

- The connection badge changes from `offline` to `robot connected`.
- Battery voltage, base state, session holder, and raw JSON update continuously.
- Estimated odometry speed and position update when the robot moves.
- The top-down robot heading follows odometry `pos_z`.

Verify keyboard control:

- Click `Enable API Control`.
- Hold `W`/`S` or arrow up/down and confirm forward/backward motion.
- Hold `A`/`D` or arrow left/right and confirm lateral motion on supported bases.
- Hold `Q`/`E` and confirm rotation.
- Hold `Shift` with a movement key and confirm faster command values.
- Press `Space` and confirm command values return to zero.
- Click `Disable API Control` and confirm the base stops accepting GUI drive commands.

Verify shutdown behavior:

- Close the browser tab while API control is enabled.
- Confirm the backend sends a zero move command and deinitializes API control.
- Stop the Rust process and confirm the base is not left in an active API-control session.

## Linux SocketCAN Coverage

Run this only on a Linux host with SocketCAN/libudev development packages available:

```bash
cargo check --all-targets --all-features
```

If the host has CAN hardware or `vcan` configured, also run the SocketCAN examples against the configured CAN interface.
