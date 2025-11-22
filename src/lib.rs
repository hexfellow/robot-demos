use clap::{Parser, Subcommand};
use log::warn;
use prost::Message;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub const ACCEPTABLE_PROTOCOL_MAJOR_VERSION: u32 = 1;

// Protobuf generated code.
pub mod proto_public_api {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

pub fn decode_message(bytes: &[u8], log: bool) -> Result<proto_public_api::ApiUp, anyhow::Error> {
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
    return Ok(ret);
}

pub fn decode_websocket_message(
    msg: tungstenite::Message,
) -> Result<proto_public_api::ApiUp, anyhow::Error> {
    match msg {
        tungstenite::Message::Binary(bytes) => return decode_message(&bytes, false),
        _ => {
            return Err(anyhow::anyhow!("Unexpected message type"));
        }
    };
}
