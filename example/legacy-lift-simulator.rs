#![allow(clippy::clone_on_copy)]
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use robot_demos::{
    confirm_and_continue, connect_websocket, decode_websocket_message, init_logger,
    proto_public_api, send_api_down_message_to_websocket,
};
use socketcan::tokio::CanFdSocket;
use socketcan::{CanAnyFrame, CanDataFrame, EmbeddedFrame, ExtendedId, Id};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI32, AtomicU16, AtomicU8, Ordering};
use tokio::sync::mpsc;

const INTRO_TEXT: &str =
    "Do not run this demo unless you are told to do so by HexFellow. This demo will connect to a current protocol lift, then simulate as an old protocol one.";

// CAN IDs from https://odocs.hexfellow.com/books/fibot-docs/page/can-protocol-lifting-platform
// Controller -> Lifting Platform (we receive)
const CAN_ID_ENABLE: u32 = 0x03020103;
const CAN_ID_STATUS_CMD: u32 = 0x03020111;
const CAN_ID_REQUEST_MOVE_RANGE: u32 = 0x03020112;
const CAN_ID_POSITION_SET: u32 = 0x03020114;
// Lifting Platform -> Controller (we send)
const CAN_ID_HEARTBEAT: u32 = 0x030201B0;
const CAN_ID_STATUS_FEEDBACK: u32 = 0x030201B1;
const CAN_ID_MOVE_RANGE_FEEDBACK: u32 = 0x030201B2;
const CAN_ID_VELOCITY_FEEDBACK: u32 = 0x030201B3;
const CAN_ID_POSITION_FEEDBACK: u32 = 0x030201B4;

/// Number of (position, timestamp) samples to keep for velocity calculation.
const SPEED_AVERAGE_WINDOW: usize = 25;

#[derive(Parser)]
struct Args {
    #[arg(
        help = "WebSocket URL to connect to (e.g. 127.0.0.1 or [fe80::500d:96ff:fee1:d60b%3]). If you use ipv6, please make sure IPV6's zone id is correct. The zone id must be interface id not interface name. If you don't understand what this means, please use ipv4."
    )]
    url: String,
    #[arg(help = "Port to connect to (e.g. 8439)")]
    port: u16,
    #[arg(help = "Local CAN bus name to use.")]
    local_can_bus: String,
}

/// Commands from CAN handler to be sent to robot via WebSocket
enum LiftCommand {
    TargetPos(i64),
    SetSpeed(u32),
    Calibrate,
}

#[tokio::main]
async fn main() {
    init_logger();
    let args = Args::parse();
    let url = format!("ws://{}:{}", args.url, args.port);

    confirm_and_continue(INTRO_TEXT, &args.url, args.port).await;

    let (local_can_tx, local_can_rx) = CanFdSocket::open(&args.local_can_bus)
        .expect("Failed to open local CAN bus")
        .split();

    let (can_frame_tx, can_frame_rx) = mpsc::channel::<CanAnyFrame>(64);

    // Shared state for legacy protocol (updated from WebSocket, read by CAN TX tasks).
    // Position and velocity can be negative (reverse); use signed types so we don't overflow.
    let enabled = std::sync::Arc::new(AtomicU8::new(0));
    let current_pos_mm = std::sync::Arc::new(AtomicI32::new(0));
    let current_velocity_mm_s = std::sync::Arc::new(AtomicI32::new(0));
    let max_pos_mm = std::sync::Arc::new(AtomicI32::new(0));
    let max_speed_from_cmd = std::sync::Arc::new(AtomicU16::new(0));
    let calibrating = std::sync::Arc::new(AtomicU8::new(0));
    let status_abnormal = std::sync::Arc::new(AtomicU8::new(0));
    let error_code = std::sync::Arc::new(AtomicU16::new(0));
    let pulse_per_rotation = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    // Max velocity (mm/s) from robot protobuf LinearLiftStatus.max_speed; used in CAN_ID_STATUS_FEEDBACK.
    let robot_max_speed_mm_s = std::sync::Arc::new(AtomicU16::new(0));

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<LiftCommand>(16);

    let ws_stream = connect_websocket(&url)
        .await
        .expect("Error during websocket handshake");
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // Set report frequency to 250Hz so we get frequent position/velocity updates
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

    // Task: receive WebSocket status and update shared state; receive commands and send to robot
    let current_pos_mm_ws = current_pos_mm.clone();
    let current_velocity_mm_ws = current_velocity_mm_s.clone();
    let max_pos_mm_ws = max_pos_mm.clone();
    let calibrating_ws = calibrating.clone();
    let pulse_per_rotation_ws = pulse_per_rotation.clone();
    let status_abnormal_ws = status_abnormal.clone();
    let robot_max_speed_mm_s_ws = robot_max_speed_mm_s.clone();
    tokio::spawn(async move {
        let mut ws_sink = ws_sink;
        let mut speed_dq: VecDeque<(i64, u64)> = VecDeque::new();
        loop {
            tokio::select! {
                Some(cmd) = cmd_rx.recv() => {
                    let down = match cmd {
                        LiftCommand::TargetPos(pos) => proto_public_api::api_down::Down::LinearLiftCommand(
                            proto_public_api::LinearLiftCommand {
                                command: Some(proto_public_api::linear_lift_command::Command::TargetPos(pos)),
                            },
                        ),
                        LiftCommand::SetSpeed(speed) => proto_public_api::api_down::Down::LinearLiftCommand(
                            proto_public_api::LinearLiftCommand {
                                command: Some(proto_public_api::linear_lift_command::Command::SetSpeed(speed)),
                            },
                        ),
                        LiftCommand::Calibrate => proto_public_api::api_down::Down::LinearLiftCommand(
                            proto_public_api::LinearLiftCommand {
                                command: Some(proto_public_api::linear_lift_command::Command::Calibrate(true)),
                            },
                        ),
                    };
                    if let Err(e) = send_api_down_message_to_websocket(
                        &mut ws_sink,
                        proto_public_api::ApiDown { down: Some(down) },
                    )
                    .await
                    {
                        error!("Failed to send command to robot: {}", e);
                    }
                }
                msg_opt = ws_stream.next() => {
                    match msg_opt {
                        Some(Ok(msg)) => {
                            let msg = match decode_websocket_message(msg, true) {
                                Ok(m) => m,
                                Err(e) => {
                                    warn!("Failed to decode WebSocket message: {}", e);
                                    continue;
                                }
                            };
                            if let Some(proto_public_api::api_up::Status::LinearLiftStatus(s)) = msg.status.clone() {
                                let ppr = s.pulse_per_rotation;
                                if ppr > 0 {
                                    pulse_per_rotation_ws.store(ppr, Ordering::Relaxed);
                                    // position in mm: current_pos (pulses, can be negative) * 1000 / pulse_per_rotation
                                    let pos_mm = (s.current_pos * 1000 / ppr as i64)
                                        .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                                    current_pos_mm_ws.store(pos_mm, Ordering::Relaxed);
                                    let max_mm = (s.max_pos * 1000 / ppr as i64)
                                        .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                                    max_pos_mm_ws.store(max_mm, Ordering::Relaxed);
                                    // Robot max_speed (physical max, pulses/s) -> mm/s for CAN_ID_STATUS_FEEDBACK
                                    let max_speed_mm_s =
                                        ((s.max_speed as u64 * 1000) / ppr as u64).min(u16::MAX as u64) as u16;
                                    robot_max_speed_mm_s_ws.store(max_speed_mm_s, Ordering::Relaxed);
                                    // Velocity from position delta / time delta (protobuf has no current velocity field)
                                    if let Some(ts) = msg.time_stamp.as_ref() {
                                        let mono = ts.monotonic_time_stamp.as_ref().unwrap();
                                        let ts_us = mono.seconds * 1_000_000 + (mono.nanoseconds / 1000) as u64;
                                        speed_dq.push_back((s.current_pos, ts_us));
                                        if speed_dq.len() > SPEED_AVERAGE_WINDOW {
                                            let last = speed_dq.pop_front().unwrap();
                                            let time_diff_us = ts_us.overflowing_sub(last.1).0;
                                            let time_diff_s = time_diff_us as f64 / 1_000_000.0;
                                            if time_diff_s > 0.0 {
                                                let position_diff_pulses = s.current_pos - last.0;
                                                let position_diff_mm =
                                                    (position_diff_pulses * 1000 / ppr as i64) as f64;
                                                let velocity_mm_s = (position_diff_mm / time_diff_s)
                                                    .clamp(i32::MIN as f64, i32::MAX as f64) as i32;
                                                // info!("time diff: {} us, position diff: {} pulses, velocity: {} mm/s", time_diff_us, position_diff_pulses, velocity_mm_s);
                                                current_velocity_mm_ws.store(velocity_mm_s, Ordering::Relaxed);
                                            }
                                        }
                                    }
                                    calibrating_ws.store(
                                        if s.state() == proto_public_api::LiftState::LsCalibrating {
                                            1
                                        } else {
                                            0
                                        },
                                        Ordering::Relaxed,
                                    );
                                    // 0: normal, 1: abnormal per protocol; treat only moving states as normal
                                    let normal = matches!(
                                        s.state(),
                                        proto_public_api::LiftState::LsAlgrithmControl
                                            | proto_public_api::LiftState::LsOvertakeControl
                                    );
                                    status_abnormal_ws.store(if normal { 0 } else { 1 }, Ordering::Relaxed);
                                }
                            }
                            if let Some(log_msg) = msg.log {
                                warn!("Log from robot: {}", log_msg);
                            }
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => break,
                    }
                }
            }
        }
    });

    // Task: receive CAN frames (controller -> lifting platform) and update state / send commands
    let enabled_can = enabled.clone();
    let max_speed_from_cmd_can = max_speed_from_cmd.clone();
    let pulse_per_rotation_can = pulse_per_rotation.clone();
    let max_pos_mm_can = max_pos_mm.clone();
    let mut local_can_rx = local_can_rx;
    let can_frame_tx_reply = can_frame_tx.clone();
    tokio::spawn(async move {
        while let Some(Ok(frame)) = local_can_rx.next().await {
            let id = match frame.id() {
                Id::Extended(ext) => ext.as_raw(),
                Id::Standard(_) => continue,
            };
            let data = match &frame {
                CanAnyFrame::Normal(df) => df.data(),
                CanAnyFrame::Fd(fd) => fd.data(),
                _ => continue,
            };
            match id {
                CAN_ID_ENABLE if data.len() >= 4 => {
                    let en = data[3];
                    enabled_can.store(if en != 0 { 1 } else { 0 }, Ordering::Relaxed);
                    info!("Legacy enable: {}", en);
                }
                CAN_ID_STATUS_CMD if data.len() >= 3 => {
                    let return_to_zero = data[0];
                    let max_vel_mm_s = u16::from_le_bytes([data[1], data[2]]);
                    max_speed_from_cmd_can.store(max_vel_mm_s, Ordering::Relaxed);
                    if return_to_zero != 0 {
                        let _ = cmd_tx.send(LiftCommand::Calibrate).await;
                    }
                    // Set robot speed from legacy max move velocity (mm/s -> pulses/s)
                    let ppr = pulse_per_rotation_can.load(Ordering::Relaxed);
                    if ppr > 0 {
                        let speed_pulses_s = (max_vel_mm_s as u32 * ppr) / 1000;
                        let _ = cmd_tx.send(LiftCommand::SetSpeed(speed_pulses_s)).await;
                    }
                }
                CAN_ID_REQUEST_MOVE_RANGE => {
                    let max_mm = max_pos_mm_can.load(Ordering::Relaxed);
                    // Payload: move_range u16 (mm, unsigned) + reverse u8 → 3 bytes
                    let move_range_u = max_mm.abs().min(u16::MAX as i32) as u16;
                    let reverse = if max_mm < 0 { 1u8 } else { 0u8 };
                    let payload = [
                        move_range_u.to_le_bytes()[0],
                        move_range_u.to_le_bytes()[1],
                        reverse,
                    ];
                    if let Some(f) = CanDataFrame::new(
                        Id::Extended(ExtendedId::new(CAN_ID_MOVE_RANGE_FEEDBACK).unwrap()),
                        &payload,
                    ) {
                        let _ = can_frame_tx_reply.send(f.into()).await;
                    }
                }
                CAN_ID_POSITION_SET if data.len() >= 2 => {
                    // Payload: position u16 (mm, unsigned) + optional reverse u8 → 2 or 3 bytes
                    let pos_u = u16::from_le_bytes([data[0], data[1]]);
                    let reverse = data.get(2).copied().unwrap_or(0);
                    let pos_mm = if reverse != 0 {
                        -(pos_u as i32)
                    } else {
                        pos_u as i32
                    };
                    let ppr = pulse_per_rotation_can.load(Ordering::Relaxed);
                    if ppr > 0 {
                        let target_pulses = (pos_mm as i64 * ppr as i64) / 1000;
                        let _ = cmd_tx.send(LiftCommand::TargetPos(target_pulses)).await;
                    }
                }
                _ => {}
            }
        }
    });

    // Single task: drain CAN frame channel to the socket
    let mut local_can_tx = local_can_tx;
    tokio::spawn(async move {
        let mut rx = can_frame_rx;
        while let Some(frame) = rx.recv().await {
            let _ = local_can_tx.send(frame).await;
        }
    });

    // Periodic CAN TX: heartbeat 500ms, status 100ms, velocity 50ms, position 50ms
    let heartbeat_tx = can_frame_tx.clone();
    let enabled_hb = enabled.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            interval.tick().await;
            let en = enabled_hb.load(Ordering::Relaxed);
            let data = [en];
            if let Some(f) = CanDataFrame::new(
                Id::Extended(ExtendedId::new(CAN_ID_HEARTBEAT).unwrap()),
                &data,
            ) {
                let _ = heartbeat_tx.send(f.into()).await;
            }
        }
    });

    let status_tx = can_frame_tx.clone();
    let status_abnormal_s = status_abnormal.clone();
    let calibrating_s = calibrating.clone();
    let error_code_s = error_code.clone();
    let robot_max_speed_mm_s_fb = robot_max_speed_mm_s.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            // Payload: status u8 + return_to_zero_status u8 + errorcode u16 + max_velocity u16 (from protobuf max_speed) → 6 bytes
            let mut data = [0u8; 6];
            data[0] = status_abnormal_s.load(Ordering::Relaxed);
            data[1] = calibrating_s.load(Ordering::Relaxed);
            let ec = error_code_s.load(Ordering::Relaxed);
            data[2..4].copy_from_slice(&ec.to_le_bytes());
            let max_vel = robot_max_speed_mm_s_fb.load(Ordering::Relaxed);
            data[4..6].copy_from_slice(&max_vel.to_le_bytes());
            if let Some(f) = CanDataFrame::new(
                Id::Extended(ExtendedId::new(CAN_ID_STATUS_FEEDBACK).unwrap()),
                &data,
            ) {
                let _ = status_tx.send(f.into()).await;
            }
        }
    });

    let velocity_tx = can_frame_tx.clone();
    let current_velocity_v = current_velocity_mm_s.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(50));
        loop {
            interval.tick().await;
            let vel = current_velocity_v.load(Ordering::Relaxed);
            // Payload: velocity u16 (mm/s, unsigned) + reverse u8 → 3 bytes
            let vel_u = vel.abs().min(u16::MAX as i32) as u16;
            let reverse = if vel < 0 { 1u8 } else { 0u8 };
            let data = [vel_u.to_le_bytes()[0], vel_u.to_le_bytes()[1], reverse];
            if let Some(f) = CanDataFrame::new(
                Id::Extended(ExtendedId::new(CAN_ID_VELOCITY_FEEDBACK).unwrap()),
                &data,
            ) {
                let _ = velocity_tx.send(f.into()).await;
            }
        }
    });

    let position_tx = can_frame_tx.clone();
    let current_pos_p = current_pos_mm.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(50));
        loop {
            interval.tick().await;
            let pos = current_pos_p.load(Ordering::Relaxed);
            // Payload: real position u16 (mm, unsigned) + reverse u8 → 3 bytes
            let pos_u = pos.abs().min(u16::MAX as i32) as u16;
            let reverse = if pos < 0 { 1u8 } else { 0u8 };
            let data = [pos_u.to_le_bytes()[0], pos_u.to_le_bytes()[1], reverse];
            if let Some(f) = CanDataFrame::new(
                Id::Extended(ExtendedId::new(CAN_ID_POSITION_FEEDBACK).unwrap()),
                &data,
            ) {
                let _ = position_tx.send(f.into()).await;
            }
        }
    });

    info!(
        "Legacy lift simulator running: WebSocket {} local CAN {}",
        url, args.local_can_bus
    );
    std::future::pending::<()>().await;
}
