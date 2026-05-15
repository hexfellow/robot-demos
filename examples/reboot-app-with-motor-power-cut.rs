use clap::Parser;
use futures_util::StreamExt;
use log::info;
use robot_demos::{
    confirm_and_continue, connect_websocket, decode_websocket_message, init_logger,
    proto_public_api, proto_public_api_version, send_api_down_message_to_websocket,
};

const INTRO_TEXT: &str = "Reboot the robot with a ensured power cut to motors, but not output.";

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
    let (mut ws_sink, _) = ws_stream.split();

    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::RebootRequest(
                proto_public_api::RebootRequest {
                    reboot_magic_number: 0x0065686e6f73696b,
                    reboot_mode: proto_public_api::RebootMode::RmAppWithMotorPowerCut as i32,
                },
            )),
            protocol_major_version: proto_public_api_version::CURRENT_PROTOCOL_MAJOR_VERSION,
            protocol_minor_version: proto_public_api_version::CURRENT_PROTOCOL_MINOR_VERSION,
        },
    )
    .await
    .expect("Failed to send clear parking stop message");

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}
