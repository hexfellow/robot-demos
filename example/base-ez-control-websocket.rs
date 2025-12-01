// This is a demo controling base to move at 0.1 m/s forward, while printing data from the base.
// Based on this code, we make some nice control showcase, like:
// https://github.com/orgs/hexfellow/discussions/1
// https://github.com/orgs/hexfellow/discussions/2

use clap::Parser;
use futures_util::StreamExt;
use log::{error, info, warn};
use robot_demos::{decode_websocket_message, proto_public_api, send_api_down_message_to_websocket};
use std::net::SocketAddrV4;
use tokio_tungstenite::MaybeTlsStream;

#[derive(Parser)]
struct Args {
    #[arg(help = "IpV4 address to connect to (e.g. 127.0.0.1:8439)")]
    url: SocketAddrV4,
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    )
    .init();
    let args = Args::parse();
    let url = format!("ws://{}", args.url);
    let res = tokio_tungstenite::connect_async(&url).await;
    let ws_stream = match res {
        Ok((ws, _)) => ws,
        Err(e) => {
            error!("Error during websocket handshake: {}", e);
            return;
        }
    };
    info!("Connected to: {}", args.url);
    // Remember to set tcp_nodelay to true, to get better performance.
    match ws_stream.get_ref() {
        MaybeTlsStream::Plain(stream) => {
            stream.set_nodelay(true).unwrap();
        }
        _ => warn!("set_nodelay not implemented for TLS streams"),
    }
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // Spawn the print task
    tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            let msg = decode_websocket_message(msg).unwrap();
            match msg.status {
                Some(proto_public_api::api_up::Status::BaseStatus(base_status)) => {
                    if let Some(estimated_odometry) = base_status.estimated_odometry {
                        info!("Estimated odometry: {:?}", estimated_odometry);
                    }
                }
                _ => {}
            }
        }
    });

    // Down, base command, command, api_control_initialize = true
    // Set report frequency to 50Hz; Since its a simple demo using simple_move_command, we don't need to hear from base too often.
    // If not changed, it will spam Estimated odometry at 1000Hz, which is too much for a simple demo.
    // This will only work for the current session, different sessions have independent report frequency settings.
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::SetReportFrequency(
                proto_public_api::ReportFrequency::Rf50Hz as i32,
            )),
        },
    )
    .await
    .expect("Failed to send set report frequency message");

    // Before sending move command, we need to set initialize the base first.
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::BaseCommand(
                proto_public_api::BaseCommand {
                    command: Some(
                        proto_public_api::base_command::Command::ApiControlInitialize(true),
                    ),
                },
            )),
        },
    )
    .await
    .expect("Failed to send initialize message");
    let start_time = std::time::Instant::now();
    while start_time.elapsed() < std::time::Duration::from_secs(10) {
        // You can also use tokio's tick if you want
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Down, base command, command, simple_move_command, vx = 0.0, vy = 0, w = 0.1
        send_api_down_message_to_websocket(
            &mut ws_sink,
            proto_public_api::ApiDown {
                down: Some(proto_public_api::api_down::Down::BaseCommand(
                    proto_public_api::BaseCommand {
                        command: Some(proto_public_api::base_command::Command::SimpleMoveCommand(
                            proto_public_api::SimpleBaseMoveCommand {
                                command: Some(
                                    proto_public_api::simple_base_move_command::Command::XyzSpeed(
                                        proto_public_api::XyzSpeed {
                                            speed_x: 0.0,
                                            speed_y: 0.0,
                                            speed_z: 0.1,
                                        },
                                    ),
                                ),
                            },
                        )),
                    },
                )),
            },
        )
        .await
        .expect("Failed to send move message");
    }
    // This is essential because if base lost control for a long time, it will enter protected state.
    // So lets tell the base we are finishing our control session.
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::BaseCommand(
                proto_public_api::BaseCommand {
                    command: Some(
                        proto_public_api::base_command::Command::ApiControlInitialize(false),
                    ),
                },
            )),
        },
    )
    .await
    .expect("Failed to send deinitialize message");
    info!("Successfully deinitialized base");
}
