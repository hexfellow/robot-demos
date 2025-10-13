# Robot Examples

This repo provides various examples of hexfellow's products.

For python demo, go to [python-examples](./python/README.md). Everything below is for rust examples.

## Base

### Base Ez Control

Minimum control demo for base. Just commands base to move forward at 0.1 m/s, and print estimated odometry. Nothing else.

#### Usage

```bash
cargo run --bin base-ez-control ws://172.18.23.92:8439
```

Remember to change the IP address to the actual IP address of the base.
