# Robot Examples

This repo is meant to: 
- Provide examples to help developers understand and use the [WebSocket API](https://github.com/hexfellow/proto-public-api)
- Provide a minimal (usually just 100-200 lines of code) example to control a robot.

This repo is NOT meant to:
- Developers to skip reading the CODE. PLEASE UNDERSTAND THE CODE AND ITS COMMENTS.
- Be the only way to control the robot. You can always choose to write your own code or use the community provided libraries.
- To demonstrate the full capabilities of the robot. For that purpose, check the community showcases.

## Base

### Base Ez Control

Minimum control demo for base. Just command the base to rotate at 0.1 rad/s for 10s while printing estimated odometry, lastly deinitialize the base. Nothing else.

#### Usage

```bash
cargo run --bin base-ez-control ws://172.18.23.92:8439
```

Remember to change the IP address to the actual IP address of the base.
