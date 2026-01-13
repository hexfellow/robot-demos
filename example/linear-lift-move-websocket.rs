#![allow(clippy::clone_on_copy)]
// This is a demo controling lift to move to a certain percentage of the max position.
// Be aware that, all numbers stated as intxx can be negative. Do not assume they are always positive.
use clap::Parser;
use futures_util::StreamExt;
use log::{error, info, warn};
use robot_demos::{decode_websocket_message, proto_public_api, send_api_down_message_to_websocket};
use tokio_tungstenite::MaybeTlsStream;

#[derive(Parser)]
struct Args {
    #[arg(
        help = "WebSocket URL to connect to (e.g. 127.0.0.1 or [fe80::500d:96ff:fee1:d60b%3]). If you use ipv6, please make sure IPV6's zone id is correct. The zone id must be interface id not interface name. If you don't understand what this means, please use ipv4."
    )]
    url: String,
    #[arg(help = "Port to connect to (e.g. 8439)")]
    port: u16,
    #[arg(help = "Percentage of max position to move to (e.g. 0.5)")]
    percentage: f64,
    #[arg(
        help = "How fast to move (e.g. 0.9), default is 0.9",
        default_value = "0.9"
    )]
    speed_factor: f64,
    #[arg(
        long,
        help = "If calibrate before moving, default is false",
        action = clap::ArgAction::SetTrue
    )]
    re_calibrate: bool,
}

lazy_static::lazy_static! {
    static ref LINEAR_LIFT_MAX_POS: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    static ref LINEAR_LIFT_MAX_SPEED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    )
    .init();
    let args = Args::parse();
    assert!(
        args.speed_factor > 0.0 && args.speed_factor <= 1.0,
        "Speed factor must be between 0.0 and 1.0, got: {}",
        args.speed_factor
    );
    if args.percentage < 0.0 || args.percentage > 1.0 {
        panic!(
            "Percentage must be between 0.0 and 1.0, got: {}",
            args.percentage
        );
    }
    let url = args.url;
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
    info!("Connected to: {}", url);
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
            #[allow(clippy::single_match)]
            match msg.status {
                Some(proto_public_api::api_up::Status::LinearLiftStatus(linear_lift_status)) => {
                    if linear_lift_status.calibrated {
                        let max = linear_lift_status.max_pos;
                        // We don't care if this fails to make code simple
                        let _ = LINEAR_LIFT_MAX_POS.set(max);
                        let _ = LINEAR_LIFT_MAX_SPEED.set(linear_lift_status.max_speed);
                        let current = linear_lift_status.current_pos;
                        let percentage = current as f64 / max as f64;
                        let pulse_per_meter = linear_lift_status.pulse_per_rotation as f64;
                        let current_meter = current as f64 / pulse_per_meter;
                        let max_meter = max as f64 / pulse_per_meter;
                        info!(
                        "Current position: {:?}m, Max position: {:?}m, Percentage: {:?}, Raw Current Position: {:?}, Raw Max Position: {:?}",
                        current_meter, max_meter, percentage, current, max
                    );
                    } else if linear_lift_status.state()
                        == proto_public_api::LiftState::LsCalibrating
                    {
                        info!("Lift is calibrating");
                    } else {
                        error!("Lift is not yet calibrated, and is not calibrating. This should only happen if you've triggered a clearable parking stop, or pressed the emergency stop button, or motor has error. You should send calibrate command, or restart the robot. Or you can use with `--re-calibrate` flag to handle this case.");
                    }
                }
                // Add other handles yourself
                _ => {}
            }
            if let Some(log) = msg.log {
                warn!("Log from robot: {}", log);
            }
        }
    });
    // Set report frequency to 50Hz; Since its a simple demo.
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

    // Send calibrate command if required
    if args.re_calibrate {
        send_api_down_message_to_websocket(
            &mut ws_sink,
            proto_public_api::ApiDown {
                down: Some(proto_public_api::api_down::Down::LinearLiftCommand(
                    proto_public_api::LinearLiftCommand {
                        command: Some(proto_public_api::linear_lift_command::Command::Calibrate(
                            true,
                        )),
                    },
                )),
            },
        )
        .await
        .expect("Failed to send calibrate message");
    }

    let start_time = std::time::Instant::now();
    let (max, max_speed) = {
        loop {
            if let Some(max) = LINEAR_LIFT_MAX_POS.get() {
                if let Some(max_speed) = LINEAR_LIFT_MAX_SPEED.get() {
                    break (max.clone(), max_speed.clone());
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };
    let move_target = (args.percentage * max as f64) as i64;

    // Set speed to 90% of max speed
    let speed = (max_speed as f64 * args.speed_factor) as u32;
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::LinearLiftCommand(
                proto_public_api::LinearLiftCommand {
                    command: Some(proto_public_api::linear_lift_command::Command::SetSpeed(
                        speed,
                    )),
                },
            )),
        },
    )
    .await
    .expect("Failed to send set speed message");

    // To keep this demo simple, we quit after 5 seconds no matter what.
    while start_time.elapsed() < std::time::Duration::from_secs(5) {
        // You can also use tokio's tick if you want
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        send_api_down_message_to_websocket(
            &mut ws_sink,
            proto_public_api::ApiDown {
                down: Some(proto_public_api::api_down::Down::LinearLiftCommand(
                    proto_public_api::LinearLiftCommand {
                        command: Some(proto_public_api::linear_lift_command::Command::TargetPos(
                            move_target,
                        )),
                    },
                )),
            },
        )
        .await
        .expect("Failed to send move message");
    }
}
