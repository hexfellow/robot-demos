use clap::Parser;
use env_logger::fmt::Timestamp;
use futures_util::stream::SplitSink;
use futures_util::SinkExt;
use futures_util::StreamExt;
use robot_demos::proto_public_api::ApiUp;
use robot_demos::{confirm_and_continue, connect_websocket, decode_websocket_message, init_logger};
// use log::debug;
use serde::Serialize;
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};

const INTRO_TEXT: &str = "Show motor status in PlotJuggler.";

#[derive(Parser)]
struct Args {
    #[arg(
        help = "WebSocket URL to connect to (e.g. 127.0.0.1 or [fe80::500d:96ff:fee1:d60b%3]). If you use ipv6, please make sure IPV6's zone id is correct. The zone id must be interface id not interface name. If you don't understand what this means, please use ipv4."
    )]
    url: String,
    #[arg(help = "Port to connect to (e.g. 8439)")]
    port: u16,
    #[arg(
        help = "PlotJuggler WebSocket address (e.g. ws://localhost:9871)",
        default_value = "ws://localhost:9871"
    )]
    plotjugger_address: String,
}

#[derive(Serialize, Clone, Debug)]
struct PlotJugglerMessage {
    // Make life easier for PlotJuggler. Since APIUp's timestamp is a struct, not single field
    timestamp_seconds: f64,
    message: ApiUp,
}

#[tokio::main]
async fn main() {
    init_logger();
    let args = Args::parse();
    let url = format!("ws://{}:{}", args.url, args.port);

    confirm_and_continue(INTRO_TEXT, &args.url, args.port).await;

    let mut client = PlotJugglerWebsocketClient::new(&args.plotjugger_address)
        .await
        .unwrap();

    let ws_stream = connect_websocket(&url)
        .await
        .expect("Error during websocket handshake. Did you type the correct URL?");
    let (_, mut ws_stream) = ws_stream.split();

    // Spawn the print task
    tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            let msg = decode_websocket_message(msg, true).unwrap();
            let monotonic_time_stamp = msg
                .time_stamp
                .clone()
                .unwrap()
                .monotonic_time_stamp
                .unwrap();
            let timestamp_seconds = monotonic_time_stamp.seconds as f64
                + monotonic_time_stamp.nanoseconds as f64 / 1000000000.0;
            let plot_juggler_message = PlotJugglerMessage {
                timestamp_seconds,
                message: msg,
            };
            client.send(&plot_juggler_message).await.unwrap();
        }
    });

    // Keep printing basic information from the robot.
    std::future::pending::<()>().await;
}

/// A client for the PlotJuggler WebSocket API.
///
/// Provides a `send` function to send any type that implements `Send + Sync + Serialize`.
/// When creating, provide a websocket address. The client will internally create a connection
/// and split the stream into write and read parts.
pub struct PlotJugglerWebsocketClient {
    write: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
}

impl PlotJugglerWebsocketClient {
    /// Creates a new PlotJugglerWebsocketClient and connects to the specified address.
    ///
    /// # Arguments
    ///
    /// * `address` - The WebSocket address to connect to (e.g., "ws://localhost:9871")
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the client on success, or an error if the connection fails.
    pub async fn new(address: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // debug!("Connecting to {}...", address);
        let (ws_stream, _) = connect_async(address).await?;
        // debug!("Connected!");
        let (write, _read) = ws_stream.split();
        Ok(Self { write })
    }

    /// Sends a serializable data structure to the PlotJuggler server.
    ///
    /// # Arguments
    ///
    /// * `data` - Any type that implements `Send + Sync + Serialize`
    ///
    /// # Returns
    ///
    /// Returns a `Result` indicating success or failure of the send operation.
    pub async fn send<T: Send + Sync + Serialize>(
        &mut self,
        data: &T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string(data)?;
        self.write.send(Message::Text(json.into())).await?;
        Ok(())
    }
}
