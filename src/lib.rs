use futures_util::stream::SplitSink;
use futures_util::SinkExt;
use log::warn;
use prost::Message;
use tokio_tungstenite::WebSocketStream;
#[path = "proto-public-api/version.rs"]
pub mod proto_public_api_version;
pub const ACCEPTABLE_PROTOCOL_MAJOR_VERSION: u32 = 1;
pub const MINIMUM_PROTOCOL_MINOR_VERSION: u32 = 0;

// Protobuf generated code.
pub mod proto_public_api {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

pub fn decode_message_with_minimum_protocol_minor_version(
    bytes: &[u8],
    log: bool,
    minimum_protocol_minor_version: u32,
) -> Result<proto_public_api::ApiUp, anyhow::Error> {
    let msg = proto_public_api::ApiUp::decode(bytes).unwrap();
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
    if msg.protocol_minor_version < minimum_protocol_minor_version {
        let w = format!(
            "Protocol minor version is less than {}, current version: {}. This might cause compatibility issues. Consider upgrading the base firmware.",
            minimum_protocol_minor_version, msg.protocol_minor_version
        );
        warn!("{}", w);
        // If protocol minor version does not match, lets just stop printing odometry.
        return Err(anyhow::anyhow!(w));
    }
    Ok(ret)
}

pub fn decode_message(bytes: &[u8], log: bool) -> Result<proto_public_api::ApiUp, anyhow::Error> {
    decode_message_with_minimum_protocol_minor_version(bytes, log, MINIMUM_PROTOCOL_MINOR_VERSION)
}

pub fn decode_websocket_message_with_minimum_protocol_minor_version(
    msg: tungstenite::Message,
    log: bool,
    minimum_protocol_minor_version: u32,
) -> Result<proto_public_api::ApiUp, anyhow::Error> {
    match msg {
        tungstenite::Message::Binary(bytes) => decode_message_with_minimum_protocol_minor_version(
            &bytes,
            log,
            minimum_protocol_minor_version,
        ),
        _ => Err(anyhow::anyhow!("Unexpected message type")),
    }
}

pub fn decode_websocket_message(
    msg: tungstenite::Message,
    log: bool,
) -> Result<proto_public_api::ApiUp, anyhow::Error> {
    match msg {
        tungstenite::Message::Binary(bytes) => decode_message(&bytes, log),
        _ => Err(anyhow::anyhow!("Unexpected message type")),
    }
}

pub async fn send_api_down_message_to_websocket(
    ws_sink: &mut SplitSink<
        WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        tungstenite::Message,
    >,
    msg: proto_public_api::ApiDown,
) -> Result<(), anyhow::Error> {
    ws_sink
        .send(tungstenite::Message::Binary(msg.encode_to_vec().into()))
        .await?;
    Ok(())
}
