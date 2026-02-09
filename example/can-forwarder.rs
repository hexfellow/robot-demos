use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use kcp_bindings::{HexSocketOpcode, HexSocketParser, KcpPortOwner};
use log::info;
use prost::Message;
use robot_demos::{
    confirm_and_continue, connect_websocket, create_kcp_socket, decode_message,
    decode_websocket_message, init_logger, proto_public_api, send_api_down_message_to_websocket,
};
use socketcan::tokio::CanFdSocket;
use socketcan::{CanAnyFrame, CanDataFrame, CanFdFrame, EmbeddedFrame, ExtendedId, Id, StandardId};

const INTRO_TEXT: &str = "Forward CAN bus messages from robot to local CAN bus.";

#[derive(Parser)]
struct Args {
    #[arg(
        help = "WebSocket URL to connect to (e.g. 127.0.0.1 or [fe80::500d:96ff:fee1:d60b%3]). If you use ipv6, please make sure IPV6's zone id is correct. The zone id must be interface id not interface name. If you don't understand what this means, please use ipv4."
    )]
    url: String,
    #[arg(help = "Port to connect to (e.g. 8439)")]
    port: u16,
    #[arg(help = "Remote CAN bus to use. You can only use 0,1,2.")]
    remote_can_bus: u8,
    #[arg(help = "Local CAN bus name to use.")]
    local_can_bus: String,
}

#[tokio::main]
async fn main() {
    init_logger();
    let args = Args::parse();
    let url = format!("ws://{}:{}", args.url, args.port);

    confirm_and_continue(INTRO_TEXT, &args.url, args.port).await;

    let (mut local_can_bus_tx, local_can_bus_rx) =
        CanFdSocket::open(&args.local_can_bus).unwrap().split();

    let remote_can_bus = match args.remote_can_bus {
        0 => proto_public_api::HexCanApiCanBusNumber::Hcan0,
        1 => proto_public_api::HexCanApiCanBusNumber::Hcan1,
        2 => proto_public_api::HexCanApiCanBusNumber::Hcan2,
        _ => panic!("Invalid remote CAN bus number: {}", args.remote_can_bus),
    };

    let ws_stream = connect_websocket(&url)
        .await
        .expect("Error during websocket handshake");
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    let (session_id, mut ws_stream) = {
        let msg = decode_websocket_message(ws_stream.next().await.unwrap().unwrap(), true).unwrap();
        (msg.session_id, ws_stream)
    };

    let (kcp_socket, local_port) = create_kcp_socket(&args.url).await.unwrap();

    // Enable KCP
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::EnableKcp(
                proto_public_api::EnableKcp {
                    client_peer_port: local_port as u32,
                    kcp_config: Some(proto_public_api::KcpConfig {
                        window_size_snd_wnd: 64,
                        window_size_rcv_wnd: 64,
                        interval_ms: 10,
                        no_delay: true,
                        nc: true,
                        resend: 2,
                    }),
                },
            )),
        },
    )
    .await
    .expect("Failed to send enable KCP message");

    let kcp_server_status = loop {
        let msg = decode_websocket_message(ws_stream.next().await.unwrap().unwrap(), true).unwrap();
        if msg.kcp_server_status.is_some() {
            info!("KCP Enabled");
            break msg.kcp_server_status.unwrap();
        }
    };

    // KCP port is in kcp_config
    let kcp_server_addr = format!("{}:{}", args.url, kcp_server_status.server_port)
        .parse()
        .unwrap();

    // Please makesure kcp_port_owner lives long enough.
    // You can consider moving it to Arc
    let (kcp_port_owner, tx, mut rx) =
        kcp_bindings::KcpPortOwner::new_costom_socket(kcp_socket, session_id, kcp_server_addr)
            .await
            .unwrap();

    // Send any message to activate KCP connection.
    // Here we just send a placeholder message.
    KcpPortOwner::send_binary(
        &tx,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::PlaceholderMessage(true)),
        }
        .encode_to_vec(),
    )
    .await
    .expect("Failed to send placeholder message");

    // Set websocket report frequency to 1Hz.
    // Because we will be decoding KCP messages from now on.
    send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::SetReportFrequency(
                proto_public_api::ReportFrequency::Rf1Hz as i32,
            )),
        },
    )
    .await
    .expect("Failed to send set report frequency message");

    // Spawn the websocket handle task
    // Just ignore all messages from websocket.
    // You can ofc still decode message if want to. Just be aware that you must keep the websocket connection alive.
    tokio::spawn(async move {
        loop {
            let _ = ws_stream.next().await;
        }
    });
    // Spawn KCP data incoming handle task
    tokio::spawn(async move {
        let mut parser = HexSocketParser::new();
        loop {
            let bytes = match rx.recv().await {
                Some(bytes) => bytes,
                None => {
                    println!("KCP connection lost");
                    break;
                }
            };
            if let Some(messages) = parser.parse(&bytes).unwrap() {
                for (optcode, bytes) in messages {
                    if optcode == HexSocketOpcode::Binary {
                        let msg = decode_message(&bytes, true).unwrap();
                        if let Some(status) = msg.status.clone() {
                            match status {
                                proto_public_api::api_up::Status::HexCanApiCanAnyFrames(frames) => {
                                    for frame in frames.frames {
                                        if frame.bus_number() == remote_can_bus {
                                            // Unwrap here is OK because it really should not fail.
                                            let (can_frame, _bus_number) =
                                                hex_to_can_any_frame(frame).unwrap();
                                            local_can_bus_tx.send(can_frame).await.unwrap();
                                        }
                                    }
                                }
                                _ => {
                                    panic!("Expected ArmStatus, got other robot status {:?}", msg)
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut local_can_bus_rx = local_can_bus_rx;
        loop {
            let frame = local_can_bus_rx.next().await.unwrap().unwrap();
            let hex_frame = match can_any_frame_to_hex(frame, remote_can_bus) {
                Ok(hex_frame) => hex_frame,
                Err(_) => {
                    continue;
                }
            };
            let message = proto_public_api::ApiDown {
                down: Some(proto_public_api::api_down::Down::HexCanApiCanAnyFrame(
                    hex_frame,
                )),
            };
            KcpPortOwner::send_binary(&tx_clone, message.encode_to_vec())
                .await
                .expect("Failed to send CAN frame via KCP");
        }
    });

    // Wait forever
    std::future::pending::<()>().await;
    drop(kcp_port_owner);
    info!("Successfully deinitialized arm");
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameConversionError {
    UnsupportedFrameType,
    InvalidFrame(String),
}

impl std::fmt::Display for FrameConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameConversionError::UnsupportedFrameType => {
                write!(f, "UnsupportedFrameType")
            }
            FrameConversionError::InvalidFrame(msg) => {
                write!(f, "InvalidFrame: {}", msg)
            }
        }
    }
}

impl std::error::Error for FrameConversionError {}

// Helper function to convert socketcan::Id to HexCanApiCanId
fn id_to_hex_can_api_id(id: &Id) -> crate::proto_public_api::HexCanApiCanId {
    match id {
        Id::Standard(standard_id) => crate::proto_public_api::HexCanApiCanId {
            id: Some(crate::proto_public_api::hex_can_api_can_id::Id::StandardId(
                standard_id.as_raw() as u32,
            )),
        },
        Id::Extended(extended_id) => crate::proto_public_api::HexCanApiCanId {
            id: Some(crate::proto_public_api::hex_can_api_can_id::Id::ExtendedId(
                extended_id.as_raw(),
            )),
        },
    }
}

// Helper function to convert HexCanApiCanId to socketcan::Id
fn hex_can_api_id_to_id(
    hex_id: &crate::proto_public_api::HexCanApiCanId,
) -> Result<Id, FrameConversionError> {
    match hex_id.id.as_ref() {
        Some(crate::proto_public_api::hex_can_api_can_id::Id::StandardId(id)) => {
            let id = *id as u16;
            StandardId::new(id).map(Id::Standard).ok_or_else(|| {
                FrameConversionError::InvalidFrame(format!("Invalid standard ID: {}", id))
            })
        }
        Some(crate::proto_public_api::hex_can_api_can_id::Id::ExtendedId(id)) => {
            ExtendedId::new(*id).map(Id::Extended).ok_or_else(|| {
                FrameConversionError::InvalidFrame(format!("Invalid extended ID: {}", id))
            })
        }
        None => Err(FrameConversionError::InvalidFrame("Missing ID".to_string())),
    }
}

/// Convert a CanAnyFrame to HexCanApiCanAnyFrame with the specified bus number.
///
/// # Arguments
/// * `frame` - The CAN frame to convert
/// * `bus_number` - The CAN bus number to assign to the converted frame
///
/// # Returns
/// * `Ok(HexCanApiCanAnyFrame)` - The converted frame with bus_number set
/// * `Err(FrameConversionError)` - If conversion fails
pub fn can_any_frame_to_hex(
    frame: CanAnyFrame,
    bus_number: proto_public_api::HexCanApiCanBusNumber,
) -> Result<proto_public_api::HexCanApiCanAnyFrame, FrameConversionError> {
    match frame {
        CanAnyFrame::Normal(data_frame) => {
            let id = data_frame.id();
            let data = data_frame.data().to_vec();

            // Validate data length for regular CAN frame (max 8 bytes)
            if data.len() > 8 {
                return Err(FrameConversionError::InvalidFrame(format!(
                    "Regular CAN frame data length {} exceeds maximum of 8 bytes",
                    data.len()
                )));
            }

            Ok(proto_public_api::HexCanApiCanAnyFrame {
                bus_number: bus_number as i32,
                frame: Some(
                    proto_public_api::hex_can_api_can_any_frame::Frame::CanDataFrame(
                        proto_public_api::HexCanApiCanDataFrame {
                            id: Some(id_to_hex_can_api_id(&id)),
                            data,
                        },
                    ),
                ),
            })
        }
        CanAnyFrame::Fd(fd_frame) => {
            let id = fd_frame.id();
            let data = fd_frame.data().to_vec();

            // Validate data length for CAN FD frame (max 64 bytes)
            if data.len() > 64 {
                return Err(FrameConversionError::InvalidFrame(format!(
                    "CAN FD frame data length {} exceeds maximum of 64 bytes",
                    data.len()
                )));
            }

            // Extract BRS flag from FD frame flags
            // Note: socketcan::CanFdFrame doesn't expose flags() method directly.
            // We default to false, which is safe as frames without BRS will work correctly.
            // The BRS flag will be preserved when converting from HexCanApiCanAnyFrame to CanAnyFrame.
            let brs = false;

            Ok(proto_public_api::HexCanApiCanAnyFrame {
                bus_number: bus_number as i32,
                frame: Some(
                    proto_public_api::hex_can_api_can_any_frame::Frame::CanFdFrame(
                        proto_public_api::HexCanApiCanFdFrame {
                            id: Some(id_to_hex_can_api_id(&id)),
                            data,
                            brs,
                        },
                    ),
                ),
            })
        }
        CanAnyFrame::Remote(_) | CanAnyFrame::Error(_) => {
            Err(FrameConversionError::UnsupportedFrameType)
        }
    }
}

/// Convert a HexCanApiCanAnyFrame to CanAnyFrame, preserving the bus number.
///
/// # Arguments
/// * `hex_frame` - The protobuf CAN frame to convert
///
/// # Returns
/// * `Ok((CanAnyFrame, HexCanApiCanBusNumber))` - The converted frame and its bus number
/// * `Err(FrameConversionError)` - If conversion fails
pub fn hex_to_can_any_frame(
    hex_frame: proto_public_api::HexCanApiCanAnyFrame,
) -> Result<(CanAnyFrame, proto_public_api::HexCanApiCanBusNumber), FrameConversionError> {
    // Extract bus_number from the hex_frame
    // prost generates a bus_number() method that returns the enum value
    let bus_number = hex_frame.bus_number();

    let frame = match hex_frame.frame {
        Some(proto_public_api::hex_can_api_can_any_frame::Frame::CanDataFrame(data_frame)) => {
            let id = match data_frame.id {
                Some(hex_id) => hex_can_api_id_to_id(&hex_id)?,
                None => {
                    return Err(FrameConversionError::InvalidFrame(
                        "Missing ID in data frame".to_string(),
                    ))
                }
            };

            let data = data_frame.data;

            // Validate data length for regular CAN frame (max 8 bytes)
            if data.len() > 8 {
                return Err(FrameConversionError::InvalidFrame(format!(
                    "Regular CAN frame data length {} exceeds maximum of 8 bytes",
                    data.len()
                )));
            }

            CanDataFrame::new(id, &data)
                .map(CanAnyFrame::Normal)
                .ok_or_else(|| {
                    FrameConversionError::InvalidFrame(
                        "Failed to create CAN data frame".to_string(),
                    )
                })?
        }
        Some(proto_public_api::hex_can_api_can_any_frame::Frame::CanFdFrame(fd_frame)) => {
            let id = match fd_frame.id {
                Some(hex_id) => hex_can_api_id_to_id(&hex_id)?,
                None => {
                    return Err(FrameConversionError::InvalidFrame(
                        "Missing ID in FD frame".to_string(),
                    ))
                }
            };

            let data = fd_frame.data;

            // Validate data length for CAN FD frame (max 64 bytes)
            if data.len() > 64 {
                return Err(FrameConversionError::InvalidFrame(format!(
                    "CAN FD frame data length {} exceeds maximum of 64 bytes",
                    data.len()
                )));
            }

            // Create FD frame with BRS flag if set
            let fd_frame_result = if fd_frame.brs {
                CanFdFrame::with_flags(id, &data, socketcan::id::FdFlags::BRS)
            } else {
                CanFdFrame::new(id, &data)
            };

            fd_frame_result.map(CanAnyFrame::Fd).ok_or_else(|| {
                FrameConversionError::InvalidFrame("Failed to create CAN FD frame".to_string())
            })?
        }
        None => {
            return Err(FrameConversionError::InvalidFrame(
                "Missing frame data".to_string(),
            ))
        }
    };

    Ok((frame, bus_number))
}
