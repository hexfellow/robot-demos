#![allow(clippy::clone_on_copy)]
use clap::Parser;
use futures_util::StreamExt;
use log::info;
use robot_demos::{
    confirm_and_continue, connect_websocket, decode_websocket_message, init_logger,
    proto_public_api, send_api_down_message_to_websocket,
};

const INTRO_TEXT: &str = "Control lift to move back zero.";

lazy_static::lazy_static! {
    static ref MOTOR_STATUS: std::sync::Mutex<Vec<proto_public_api::MotorStatus>> = std::sync::Mutex::new(Vec::new());
}

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

    let ws_stream = connect_websocket(&url)
        .await
        .expect("Error during websocket handshake");
    let (mut ws_sink, mut ws_stream) = ws_stream.split();
    // Spawn the print task
    tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            let msg = decode_websocket_message(msg, true).unwrap();
            #[allow(clippy::single_match)]
            match msg.status {
                Some(proto_public_api::api_up::Status::RotateLiftStatus(lift_status)) => {
                    // Collect motor status and print them
                    {
                        *MOTOR_STATUS.lock().unwrap() = lift_status.motor_status.clone();
                    }
                    info!("Motor status: {:?}", lift_status.motor_status);
                }
                // Add other handles yourself
                _ => {}
            }
        }
    });
    // Set report frequency to 250Hz; Since its a simple demo.
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::SetReportFrequency(
                proto_public_api::ReportFrequency::Rf250Hz as i32,
            )),
        },
    )
    .await
    .expect("Failed to send set report frequency message");

    loop {
        let motor_status = MOTOR_STATUS.lock().unwrap().clone();
        if motor_status.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            continue;
        }
        // Read current position of each motor. Get pulse per rotation from motor status. If error is with in 1Deg, break the loop.
        let mut break_loop = true;
        'inner: for motor in &motor_status {
            let pulse_per_rotation = motor.pulse_per_rotation;
            let current_position = motor.position;
            let err = current_position as f64 / pulse_per_rotation as f64 * 360.0;
            info!("error: {}", err);
            if err.abs() > 0.2f64 {
                break_loop = false;
                break 'inner;
            }
        }
        if break_loop {
            break;
        }
        send_api_down_message_to_websocket(
            &mut ws_sink,
            proto_public_api::ApiDown {
                down: Some(proto_public_api::api_down::Down::RotateLiftCommand(
                    proto_public_api::RotateLiftCommand {
                        command: Some(proto_public_api::rotate_lift_command::Command::MotorTargets(
                            proto_public_api::MotorTargets {
                                targets: (0..motor_status.len())
                                    .map(|_| proto_public_api::SingleMotorTarget {
                                        target: Some(
                                            proto_public_api::single_motor_target::Target::Position(0),
                                        ),
                                    })
                                    .collect(),
                            },
                        )),
                    },
                )),
            },
        )
        .await
        .expect("Failed to send move to zero position message");
    }
}
