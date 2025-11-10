// This is a demo controling base to move at 0.1 m/s forward, while printing data from the base.
// Based on this code, we make some nice control showcase, like:
// https://github.com/orgs/hexfellow/discussions/1
// https://github.com/orgs/hexfellow/discussions/2

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use kcp_bindings::{HexSocketOpcode, HexSocketParser, KcpPortOwner};
use log::{error, info, warn};
use prost::Message;
use std::net::{SocketAddr, SocketAddrV4};
use tokio::net::UdpSocket;

const ACCEPTABLE_PROTOCOL_MAJOR_VERSION: u32 = 1;

#[derive(Parser)]
struct Args {
    #[arg(help = "IpV4 address to connect to (e.g. 127.0.0.1:8439)")]
    url: SocketAddrV4,
}
// Protobuf generated code.
pub mod base_backend {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

fn decode_message(bytes: &[u8], log: bool) -> Result<base_backend::ApiUp, anyhow::Error> {
    let msg = base_backend::ApiUp::decode(bytes).unwrap();
    let ret = msg.clone();
    if log {
        if let Some(log) = msg.log {
            warn!("Log from base: {:?}", log); // Having a log usually means something went boom, so lets print it.
        }
    }
    if msg.protocol_major_version != ACCEPTABLE_PROTOCOL_MAJOR_VERSION {
        let w = format!(
            "Protocol major version is not {}, current version: {}. This might cause compatibility issues. Consider upgrading the base firmware.",
            ACCEPTABLE_PROTOCOL_MAJOR_VERSION, msg.protocol_major_version
        );
        warn!("{}", w);
        // If protocol major version does not match, lets just stop printing odometry.
        return Err(anyhow::anyhow!(w));
    }
    return Ok(ret);
}

fn decode_websocket_message(
    msg: tungstenite::Message,
) -> Result<base_backend::ApiUp, anyhow::Error> {
    match msg {
        tungstenite::Message::Binary(bytes) => return decode_message(&bytes, false),
        _ => {
            return Err(anyhow::anyhow!("Unexpected message type"));
        }
    };
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    )
    .init();
    let args = Args::parse();
    let urlstr = format!("ws://{}", args.url);
    let res = tokio_tungstenite::connect_async(&urlstr).await;
    let ws_stream = match res {
        Ok((ws, _)) => ws,
        Err(e) => {
            error!("Error during websocket handshake: {}", e);
            return;
        }
    };
    info!("Connected to: {}", args.url);
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    let (session_id, mut ws_stream) = loop {
        let msg = decode_websocket_message(ws_stream.next().await.unwrap().unwrap()).unwrap();
        break (msg.session_id, ws_stream);
    };

    let kcp_socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    let local_port = kcp_socket.local_addr().unwrap().port();

    // Enable KCP
    ws_sink
        .send(tungstenite::Message::Binary(
            base_backend::ApiDown {
                down: Some(base_backend::api_down::Down::EnableKcp(
                    base_backend::EnableKcp {
                        client_peer_port: local_port as u32,
                        kcp_config: Some(base_backend::KcpConfig {
                            window_size_snd_wnd: 64,
                            window_size_rcv_wnd: 64,
                            interval_ms: 10,
                            no_delay: true,
                            nc: true,
                            resend: 2,
                        }),
                    },
                )),
            }
            .encode_to_vec()
            .into(),
        ))
        .await
        .expect("Failed to send enable KCP message");

    let kcp_server_status = loop {
        let msg = decode_websocket_message(ws_stream.next().await.unwrap().unwrap()).unwrap();
        if msg.kcp_server_status.is_some() {
            info!("KCP Enabled");
            break msg.kcp_server_status.unwrap();
        }
    };

    // KCP port is in kcp_config
    let kcp_server_addr = SocketAddr::V4(SocketAddrV4::new(
        args.url.ip().clone(),
        kcp_server_status.server_port as u16,
    ));

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
        base_backend::ApiDown {
            down: Some(base_backend::api_down::Down::PlaceholderMessage(true)),
        }
        .encode_to_vec(),
    )
    .await
    .expect("Failed to send placeholder message");

    // Set websocket report frequency to 1Hz.
    // Because we will be decoding KCP messages from now on.
    ws_sink
        .send(tungstenite::Message::Binary(
            base_backend::ApiDown {
                down: Some(base_backend::api_down::Down::SetReportFrequency(
                    base_backend::ReportFrequency::Rf1Hz as i32,
                )),
            }
            .encode_to_vec()
            .into(),
        ))
        .await
        .expect("Failed to send set report frequency message");

    // Spawn the websocket handle task
    // Just ignore all messages from websocket.
    // You can ofc still decode message if want to.
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
                                base_backend::api_up::Status::BaseStatus(base_status) => {
                                    // Prints Odom
                                    if let Some(estimated_odometry) = base_status.estimated_odometry
                                    {
                                        info!("Estimated odometry: {:?}", estimated_odometry);
                                    }
                                }
                                _ => {
                                    panic!("Expected BaseStatus, got other robot status {:?}", msg)
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
        base_backend::ApiDown {
            down: Some(base_backend::api_down::Down::SetReportFrequency(
                base_backend::ReportFrequency::Rf250Hz as i32,
            )),
        }
        .encode_to_vec(),
    )
    .await
    .expect("Failed to send initialize message");

    // Before sending move command, we need to set initialize the base first.
    KcpPortOwner::send_binary(
        &tx,
        base_backend::ApiDown {
            down: Some(base_backend::api_down::Down::BaseCommand(
                base_backend::BaseCommand {
                    command: Some(base_backend::base_command::Command::ApiControlInitialize(
                        true,
                    )),
                },
            )),
        }
        .encode_to_vec(),
    )
    .await
    .expect("Failed to send initialize message");

    // Down, base command, command, simple_move_command, vx = 0.0, vy = 0, w = 0.1
    let move_message = base_backend::ApiDown {
        down: Some(base_backend::api_down::Down::BaseCommand(
            base_backend::BaseCommand {
                command: Some(base_backend::base_command::Command::SimpleMoveCommand(
                    base_backend::SimpleBaseMoveCommand {
                        command: Some(base_backend::simple_base_move_command::Command::XyzSpeed(
                            base_backend::XyzSpeed {
                                speed_x: 0.0,
                                speed_y: 0.0,
                                speed_z: 0.1,
                            },
                        )),
                    },
                )),
            },
        )),
    };

    let start_time = std::time::Instant::now();
    while start_time.elapsed() < std::time::Duration::from_secs(10) {
        // You can also use tokio's tick if you want
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        // Send binary messages
        KcpPortOwner::send_binary(&tx, move_message.encode_to_vec())
            .await
            .expect("Failed to send move message");
    }

    // This is essential because if base lost control for a long time, it will enter protected state.
    // So lets tell the base we are finishing our control session.
    // This is the last message we send to the base, so inorder to make absolutely sure the base is deinitialized,
    // we will send it over Websocket.
    let deinitialize_bytes = base_backend::ApiDown {
        down: Some(base_backend::api_down::Down::BaseCommand(
            base_backend::BaseCommand {
                command: Some(base_backend::base_command::Command::ApiControlInitialize(
                    false,
                )),
            },
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
    info!("Successfully deinitialized base");
}
