use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use prost::Message;
use tokio_tungstenite::MaybeTlsStream;

#[derive(Parser)]
struct Args {
    #[arg(help = "WebSocket URL to connect to (e.g. ws://localhost:8080)")]
    url: String,
}

pub mod base_backend {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    )
    .init();
    let args = Args::parse();
    let res = tokio_tungstenite::connect_async(&args.url).await;
    let ws_stream = match res {
        Ok((ws, _)) => ws,
        Err(e) => {
            error!("Error during websocket handshake: {}", e);
            return;
        }
    };
    info!("Connected to: {}", args.url);
    match ws_stream.get_ref() {
        MaybeTlsStream::Plain(stream) => {
            stream.set_nodelay(true).unwrap();
        }
        _ => warn!("set_nodelay not implemented for TLS streams"),
    }
    let (mut ws_sink, mut ws_stream) = ws_stream.split();
    tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            // protobuf binary, type is ApiUp
            // Parse the message, and check if it is a BaseStatus. If so, check for parking stop. Is has parking stop, print it.
            // Else, just do nothing.
            let msg = msg.unwrap();
            match msg {
                tungstenite::Message::Binary(bytes) => {
                    let msg = base_backend::ApiUp::decode(bytes).unwrap();
                    info!("Got message: {:?}", msg);
                }
                _ => {}
            };
        }
    });
    // Disable the base.
    let disable_message = base_backend::ApiDown {
        down: Some(base_backend::api_down::Down::BaseCommand(
            base_backend::BaseCommand {
                command: Some(base_backend::base_command::Command::ApiControlInitialize(
                    false,
                )),
            },
        )),
    };
    ws_sink
        .send(tungstenite::Message::Binary(
            disable_message.encode_to_vec().into(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Down, base command, command, api_control_initialize = true
        let enable_message = base_backend::ApiDown {
            down: Some(base_backend::api_down::Down::BaseCommand(
                base_backend::BaseCommand {
                    command: Some(base_backend::base_command::Command::ApiControlInitialize(
                        true,
                    )),
                },
            )),
        };

        // Down, base command, command, simple_move_command, vx = 1, vy = 0, w = 0
        let move_message = base_backend::ApiDown {
            down: Some(base_backend::api_down::Down::BaseCommand(
                base_backend::BaseCommand {
                    command: Some(base_backend::base_command::Command::SimpleMoveCommand(
                        base_backend::SimpleBaseMoveCommand {
                            command: Some(
                                base_backend::simple_base_move_command::Command::XyzSpeed(
                                    base_backend::XyzSpeed {
                                        speed_x: 0.1,
                                        speed_y: 0.0,
                                        speed_z: 0.0,
                                    },
                                ),
                            ),
                        },
                    )),
                },
            )),
        };

        // Send binary messages
        let enable_bytes = enable_message.encode_to_vec();
        let move_bytes = move_message.encode_to_vec();

        if let Err(e) = ws_sink
            .send(tungstenite::Message::Binary(enable_bytes.into()))
            .await
        {
            error!("Failed to send enable message: {}", e);
            break;
        }

        if let Err(e) = ws_sink
            .send(tungstenite::Message::Binary(move_bytes.into()))
            .await
        {
            error!("Failed to send move message: {}", e);
            break;
        }
    }
}
