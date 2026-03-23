use clap::Parser;
use futures_util::StreamExt;
use log::info;
use robot_demos::{confirm_and_continue, connect_websocket, decode_websocket_message, init_logger};

const INTRO_TEXT: &str = "Print it's basic information.";

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
        .expect("Error during websocket handshake. Did you type the correct URL?");
    let (_, mut ws_stream) = ws_stream.split();

    // Spawn the print task
    tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            let msg = decode_websocket_message(msg, true).unwrap();
            info!("The robot type is: {}. Robot protocol major version: {}. Robot protocol minor version: {}. Session ID: {}. Current report frequency: {}.", msg.robot_type().as_str_name(), msg.protocol_major_version, msg.protocol_minor_version, msg.session_id, msg.report_frequency().as_str_name());
        }
    });

    // Keep printing basic information from the robot.
    std::future::pending::<()>().await;
}
