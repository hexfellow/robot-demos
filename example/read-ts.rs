// This is a demo controling base to move at 0.1 m/s forward, while printing data from the base.
// Based on this code, we make some nice control showcase, like:
// https://github.com/orgs/hexfellow/discussions/1
// https://github.com/orgs/hexfellow/discussions/2

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use prost::Message;
use robot_examples::{
    decode_message, decode_websocket_message, proto_public_api, ACCEPTABLE_PROTOCOL_MAJOR_VERSION,
};
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

    while let Some(Ok(msg)) = ws_stream.next().await {
        match msg {
            tungstenite::Message::Binary(bytes) => {
                let msg = proto_public_api::ApiUp::decode(bytes).unwrap();
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
                if let Some(time_stamp) = msg.time_stamp {
                    info!("Time stamp: {:?}", time_stamp);
                }
            }
            _ => {}
        };
    }
}
