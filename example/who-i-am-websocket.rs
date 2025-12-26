// Connect to robot, print it's type and exit.
// Use this if you lost QR code of device id.

use clap::Parser;
use futures_util::StreamExt;
use log::{error, info, warn};
use robot_demos::decode_websocket_message;
use tokio_tungstenite::MaybeTlsStream;

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
    let url = args.url;
    let url = format!("ws://{}:{}", url, args.port);
    info!("Try connecting to: {}", url);
    let res = tokio_tungstenite::connect_async(&url).await;
    let ws_stream = match res {
        Ok((ws, _)) => ws,
        Err(e) => {
            error!(
                "Error during websocket handshake: {}. Did you type the correct URL?",
                e
            );
            std::process::exit(1);
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
    let (_, mut ws_stream) = ws_stream.split();

    let msg = ws_stream.next().await.unwrap().unwrap();
    let msg = decode_websocket_message(msg).unwrap();
    info!(
        "I am {}, {}. My protocol major version is {}",
        msg.robot_type().as_str_name(),
        msg.robot_type,
        msg.protocol_major_version
    );
}
