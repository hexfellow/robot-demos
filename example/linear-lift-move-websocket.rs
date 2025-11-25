// This is a demo controling base to move at 0.1 m/s forward, while printing data from the base.
// Based on this code, we make some nice control showcase, like:
// https://github.com/orgs/hexfellow/discussions/1
// https://github.com/orgs/hexfellow/discussions/2

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use prost::Message;
use std::net::SocketAddrV4;
use tokio_tungstenite::MaybeTlsStream;

const ACCEPTABLE_PROTOCOL_MAJOR_VERSION: u32 = 1;

#[derive(Parser)]
struct Args {
    #[arg(help = "IpV4 address to connect to (e.g. 172.18.23.92:8439)")]
    url: SocketAddrV4,
    #[arg(help = "Percentage of max position to move to (e.g. 0.5)")]
    percentage: f64,
}

// Protobuf generated code.
pub mod proto_api {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

lazy_static::lazy_static! {
    static ref LINEAR_LIFT_MAX_POS: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    )
    .init();
    let args = Args::parse();
    if args.percentage < 0.0 || args.percentage > 1.0 {
        panic!(
            "Percentage must be between 0.0 and 1.0, got: {}",
            args.percentage
        );
    }
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
            match msg {
                tungstenite::Message::Binary(bytes) => {
                    let msg = proto_api::ApiUp::decode(bytes).unwrap();
                    if let Some(log) = msg.log {
                        warn!("Log from base: {:?}", log); // Having a log usually means something went boom, so lets print it.
                    }
                    if msg.protocol_major_version != ACCEPTABLE_PROTOCOL_MAJOR_VERSION {
                        warn!(
                            "Protocol major version is not {}, current version: {}. This might cause compatibility issues. Consider upgrading the base firmware.",
                            ACCEPTABLE_PROTOCOL_MAJOR_VERSION, msg.protocol_major_version
                        );
                        // If protocol major version does not match, lets just stop printing odometry.
                        return;
                    }
                    match msg.status {
                        Some(proto_api::api_up::Status::LinearLiftStatus(linear_lift_status)) => {
                            if linear_lift_status.calibrated {
                                let max = linear_lift_status.max_pos;
                                // We don't care if this fails to make code simple
                                let _ = LINEAR_LIFT_MAX_POS.set(max);
                                let current = linear_lift_status.current_pos;
                                let percentage = current as f64 / max as f64;
                                let pulse_per_meter = linear_lift_status.pulse_per_rotation as f64;
                                let current_meter = current as f64 / pulse_per_meter;
                                let max_meter = max as f64 / pulse_per_meter;
                                info!(
                                "Current position: {:?}m, Max position: {:?}m, Percentage: {:?}, Raw Current Position: {:?}, Raw Max Position: {:?}",
                                current_meter, max_meter, percentage, current, max
                            );
                            } else {
                                info!("Lift is not yet calibrated");
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            };
        }
    });
    // Set report frequency to 50Hz; Since its a simple demo.
    ws_sink
        .send(tungstenite::Message::Binary(
            proto_api::ApiDown {
                down: Some(proto_api::api_down::Down::SetReportFrequency(
                    proto_api::ReportFrequency::Rf50Hz as i32,
                )),
            }
            .encode_to_vec()
            .into(),
        ))
        .await
        .expect("Failed to send set report frequency message");

    let start_time = std::time::Instant::now();
    let max = {
        loop {
            if let Some(max) = LINEAR_LIFT_MAX_POS.get() {
                break max.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };
    let move_target = (args.percentage * max as f64) as i64;
    // To keep this demo simple, we quit after 10 seconds no matter what.
    while start_time.elapsed() < std::time::Duration::from_secs(10) {
        // You can also use tokio's tick if you want
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        ws_sink
            .send(tungstenite::Message::Binary(
                proto_api::ApiDown {
                    down: Some(proto_api::api_down::Down::LinearLiftCommand(
                        proto_api::LinearLiftCommand {
                            command: Some(proto_api::linear_lift_command::Command::TargetPos(
                                move_target,
                            )),
                        },
                    )),
                }
                .encode_to_vec()
                .into(),
            ))
            .await
            .expect("Failed to send move message");
    }
}
