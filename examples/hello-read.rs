use clap::Parser;
use futures_util::StreamExt;
use kcp_bindings::{HexSocketOpcode, HexSocketParser, KcpPortOwner};
use log::info;
use prost::Message;
use robot_demos::{
    confirm_and_continue, connect_websocket, create_kcp_socket, decode_message,
    decode_websocket_message, init_logger, proto_public_api, send_api_down_message_to_websocket,
};

const INTRO_TEXT: &str = "Read info from HELLO, and make the controller's leds green.";

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
    init_logger();
    let args = Args::parse();
    let url = format!("ws://{}:{}", args.url, args.port);

    confirm_and_continue(INTRO_TEXT, &args.url, args.port).await;

    let ws_stream = connect_websocket(&url).await.expect("Error during websocket handshake");
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    let (session_id, mut ws_stream) = {
        let msg = decode_websocket_message(ws_stream.next().await.unwrap().unwrap(), true).unwrap();
        (msg.session_id, ws_stream)
    };

    let (kcp_socket, local_port) = create_kcp_socket(&args.url).await.unwrap();

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
                                    // Find the secondary device status with device_id 1
                                    let secondary_device_status = msg.secondary_device_status.iter().find(|status| status.device_id == 1);
                                    if let Some(secondary_device_status) = secondary_device_status {
                                        info!("Secondary device status: {:?}", secondary_device_status);
                                    }
                                    // Collect only position and velocity of each motor
                                    let motor_data: Vec<(i64, f64, Vec<i32>)> = arm_status
                                        .motor_status
                                        .iter()
                                        .map(|motor| (motor.position, motor.speed, motor.error.clone()))
                                        .collect();
                                    let formatted: Vec<String> = motor_data
                                        .iter()
                                        .map(|(pos, speed, error)| format!("({}, {:.2}, {:?})", pos, speed, error))
                                        .collect();
                                    info!("Motor positions and velocities and errors: [{}]", formatted.join(", "));
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

    // Makes all 6 leds on the controller green.
    // Must not send this command too often. Every command sent will use the CAN bus bandwidth.
    // If you send this command too often, expect things to go boom.
    // RGB color format: little-endian bytes [R, G, B, ignored]
    // For green: R=0, G=255, B=0 -> (255 << 8) = 65280
    let green_color = 255 << 8;
    let green_light_command = proto_public_api::ApiDown {
        down: Some(proto_public_api::api_down::Down::SecondaryDeviceCommand(
            proto_public_api::SecondaryDeviceCommand {
                device_id: 1,
                command: Some(proto_public_api::secondary_device_command::Command::Hello1j1t4bControllerCommand(
                    proto_public_api::Hello1J1t4bCmd { 
                        command: Some(proto_public_api::hello1_j1t4b_cmd::Command::RgbStripeCommand(
                            proto_public_api::RgbStripeCommand {
                                rgbs: vec![green_color; 6], // 6 LEDs all set to green
                            }
                        )) 
                    }
                )),
            }
        )),
    };

    let start_time = std::time::Instant::now();
    while start_time.elapsed() < std::time::Duration::from_secs(10) {
        // You can also use tokio's tick if you want
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        // Send binary messages
        KcpPortOwner::send_binary(&tx, green_light_command.encode_to_vec())
            .await
            .expect("Failed to send zero torque message");
    }
    drop(tx);
    drop(kcp_port_owner);
}
