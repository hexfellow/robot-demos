// This is a demo controling arm to zero torque, while printing data from the arm.

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use kcp_bindings::{HexSocketOpcode, HexSocketParser, KcpPortOwner};
use log::{error, info};
use prost::Message;
use robot_demos::{
    decode_message, decode_websocket_message, proto_public_api, send_api_down_message_to_websocket,
};
use tokio::net::UdpSocket;

static MOTOR_CNT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

#[derive(Parser)]
struct Args {
    #[arg(
        help = "WebSocket URL to connect to (e.g. 127.0.0.1 or [fe80::500d:96ff:fee1:d60b%3]). If you use ipv6, please make sure IPV6's zone id is correct. The zone id must be interface id not interface name. If you don't understand what this means, please use ipv4."
    )]
    url: String,
    #[arg(help = "Port to connect to (e.g. 8439)")]
    port: u16,
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    )
    .init();
    let args = Args::parse();
    let url = args.url.clone();
    let url = format!("ws://{}:{}", url, args.port);
    info!("Try connecting to: {}", url);
    let res = tokio_tungstenite::connect_async(&url).await;
    let ws_stream = match res {
        Ok((ws, _)) => ws,
        Err(e) => {
            error!("Error during websocket handshake: {}", e);
            return;
        }
    };
    info!("Connected to: {}", args.url);
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    let (session_id, mut ws_stream) = {
        let msg = decode_websocket_message(ws_stream.next().await.unwrap().unwrap(), true).unwrap();
        (msg.session_id, ws_stream)
    };

    // Check if args.url is ipv4 or ipv6
    let ip_addr = args.url.parse::<std::net::IpAddr>().unwrap();
    let kcp_socket = if ip_addr.is_ipv4() {
        UdpSocket::bind(std::net::SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
            0,
        ))
        .await
        .unwrap()
    } else {
        UdpSocket::bind(std::net::SocketAddr::new(
            std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
            0,
        ))
        .await
        .unwrap()
    };
    let local_port = kcp_socket.local_addr().unwrap().port();

    // Enable KCP
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::EnableKcp(
                proto_public_api::EnableKcp {
                    client_peer_port: local_port as u32,
                    kcp_config: Some(proto_public_api::KcpConfig {
                        window_size_snd_wnd: 64,
                        window_size_rcv_wnd: 64,
                        interval_ms: 10,
                        no_delay: true,
                        nc: true,
                        resend: 2,
                    }),
                },
            )),
        },
    )
    .await
    .expect("Failed to send enable KCP message");

    let kcp_server_status = loop {
        let msg = decode_websocket_message(ws_stream.next().await.unwrap().unwrap(), true).unwrap();
        if msg.kcp_server_status.is_some() {
            info!("KCP Enabled");
            break msg.kcp_server_status.unwrap();
        }
    };

    // KCP port is in kcp_config
    let kcp_server_addr = format!("{}:{}", args.url, kcp_server_status.server_port)
        .parse()
        .unwrap();

    // Please makesure kcp_port_owner lives long enough.
    // You can consider moving it to Arc
    let (kcp_port_owner, tx, mut rx) =
        kcp_bindings::KcpPortOwner::new_costom_socket(kcp_socket, session_id, kcp_server_addr)
            .await
            .unwrap();

    // Send any message to activate KCP connection.
    // Here we just send a placeholder message.
    KcpPortOwner::send_binary(
        &tx,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::PlaceholderMessage(true)),
        }
        .encode_to_vec(),
    )
    .await
    .expect("Failed to send placeholder message");

    // Set websocket report frequency to 1Hz.
    // Because we will be decoding KCP messages from now on.
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::SetReportFrequency(
                proto_public_api::ReportFrequency::Rf1Hz as i32,
            )),
        },
    )
    .await
    .expect("Failed to send set report frequency message");

    // Unconditionally clear parking stop on first connect.
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::ArmCommand(
                proto_public_api::ArmCommand {
                    command: Some(proto_public_api::arm_command::Command::ArmSharedCommand(
                        proto_public_api::ArmSharedCommand {
                            command: Some(
                                proto_public_api::arm_shared_command::Command::ClearParkingStop(
                                    true,
                                ),
                            ),
                        },
                    )),
                },
            )),
        },
    )
    .await
    .expect("Failed to send clear parking stop message");

    // Spawn the websocket handle task
    // Just ignore all messages from websocket.
    // You can ofc still decode message if want to. Just be aware that you must keep the websocket connection alive.
    tokio::spawn(async move {
        loop {
            let _ = ws_stream.next().await;
        }
    });
    // Spawn KCP data incoming handle task
    tokio::spawn(async move {
        let mut parser = HexSocketParser::new();
        loop {
            let bytes = match rx.recv().await {
                Some(bytes) => bytes,
                None => {
                    println!("KCP connection lost");
                    break;
                }
            };
            if let Some(messages) = parser.parse(&bytes).unwrap() {
                for (optcode, bytes) in messages {
                    if optcode == HexSocketOpcode::Binary {
                        let msg = decode_message(&bytes, true).unwrap();
                        if let Some(status) = msg.status.clone() {
                            match status {
                                proto_public_api::api_up::Status::ArmStatus(arm_status) => {
                                    // Prints motor status
                                    let mut pos = Vec::new();
                                    let len = arm_status.motor_status.len();
                                    MOTOR_CNT.get_or_init(|| len);
                                    for motor_status in arm_status.motor_status {
                                        pos.push(motor_status.position);
                                    }
                                    info!("Position: {:?}", pos);
                                }
                                _ => {
                                    panic!("Expected ArmStatus, got other robot status {:?}", msg)
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    // Change KCP Report Frequency to 250Hz.
    KcpPortOwner::send_binary(
        &tx,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::SetReportFrequency(
                proto_public_api::ReportFrequency::Rf250Hz as i32,
            )),
        }
        .encode_to_vec(),
    )
    .await
    .expect("Failed to send change frequency message");

    let motor_cnt = loop {
        let res = MOTOR_CNT.get();
        if let Some(motor_cnt) = res {
            break *motor_cnt;
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };

    // Before sending move command, we need to set initialize the arm first.
    KcpPortOwner::send_binary(
        &tx,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::ArmCommand(
                proto_public_api::ArmCommand {
                    command: Some(proto_public_api::arm_command::Command::ArmExclusiveCommand(
                        proto_public_api::ArmExclusiveCommand {
                        exclusive_command: Some(proto_public_api::arm_exclusive_command::ExclusiveCommand::ApiControlInitialize(
                            true,
                        )),
                        }
                    )),
                },
            )),
        }
        .encode_to_vec(),
    )
    .await
    .expect("Failed to send initialize message");

    // Calibrate the arm API control.
    KcpPortOwner::send_binary(
        &tx,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::ArmCommand(
                proto_public_api::ArmCommand {
                    command: Some(proto_public_api::arm_command::Command::ArmExclusiveCommand(
                        proto_public_api::ArmExclusiveCommand {
                        exclusive_command: Some(proto_public_api::arm_exclusive_command::ExclusiveCommand::Calibrate(
                            true,
                        )),
                        }
                    )),
                },
            )),
        }
        .encode_to_vec(),
    )
    .await
    .expect("Failed to send initialize message");

    // Down, arm command, command, arm_exclusive_command, exclusive_command, command, motor_targets, targets, torque = 0.0
    let zero_torque_message = proto_public_api::ApiDown {
        down: Some(proto_public_api::api_down::Down::ArmCommand(
            proto_public_api::ArmCommand {
                command: Some(proto_public_api::arm_command::Command::ArmExclusiveCommand(
                    proto_public_api::ArmExclusiveCommand {
                        exclusive_command: Some(proto_public_api::arm_exclusive_command::ExclusiveCommand::ArmApiControlCommand(
                            proto_public_api::ArmApiControlCommand {
                                command: Some(proto_public_api::arm_api_control_command::Command::MotorTargets(
                                    proto_public_api::MotorTargets {
                                        targets: (0..motor_cnt).map(|_i| proto_public_api::SingleMotorTarget {
                                            target: Some(proto_public_api::single_motor_target::Target::Torque(0.0)),
                                        }).collect(),
                                    },
                                )),
                            }
                        )),
                    }
                )),
            }
        )),
    };

    let start_time = std::time::Instant::now();
    while start_time.elapsed() < std::time::Duration::from_secs(10) {
        // You can also use tokio's tick if you want
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        // Send binary messages
        KcpPortOwner::send_binary(&tx, zero_torque_message.encode_to_vec())
            .await
            .expect("Failed to send zero torque message");
    }

    // This is essential because if arm lost control for a long time, it will enter protected state.
    // So lets tell the arm we are finishing our control session.
    // This is the last message we send to the arm, so inorder to make absolutely sure the arm is deinitialized,
    // we will send it over Websocket.
    let deinitialize_bytes = proto_public_api::ApiDown {
        down: Some(proto_public_api::api_down::Down::ArmCommand(
            proto_public_api::ArmCommand {
                command: Some(proto_public_api::arm_command::Command::ArmExclusiveCommand(
                    proto_public_api::ArmExclusiveCommand {
                        exclusive_command: Some(proto_public_api::arm_exclusive_command::ExclusiveCommand::ApiControlInitialize(false)),
                    }
                )),
            }
        )),
    }
    .encode_to_vec();
    if let Err(e) = ws_sink
        .send(tungstenite::Message::Binary(deinitialize_bytes.into()))
        .await
    {
        panic!("Failed to send deinitialize message: {}", e);
    }
    ws_sink.close().await.expect("Failed to close websocket");
    drop(tx);
    drop(kcp_port_owner);
    info!("Successfully deinitialized arm");
}
