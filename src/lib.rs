use futures_util::stream::SplitSink;
use futures_util::SinkExt;
use log::warn;
use prost::Message;
use std::io::Write;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::sleep;
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

/// Logs a message, displays a countdown progress bar, then exits the program.
///
/// # Arguments
/// * `text` - The message to log (can include colored text using the `colored` crate)
/// * `duration` - How long to wait before exiting
///
/// # Example
/// ```no_run
/// use std::time::Duration;
/// use robot_demos::countdown_and_exit;
/// use colored::Colorize;
///
/// #[tokio::main]
/// async fn main() {
///     countdown_and_exit(
///         &format!("Starting robot in... {}", "WARNING!".red()),
///         Duration::from_secs(5)
///     ).await;
/// }
/// ```
pub async fn countdown_and_exit(text: &str, duration: Duration) {
    println!("{}", text);

    let total_seconds = duration.as_secs();
    let progress_bar = indicatif::ProgressBar::new(total_seconds);
    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.226/238}] {pos}/{len} seconds remaining")
            .unwrap()
            .progress_chars("#>-"),
    );

    let start = std::time::Instant::now();
    let tick_interval = Duration::from_millis(100);

    loop {
        let elapsed = start.elapsed();
        if elapsed >= duration {
            break;
        }
        let elapsed_secs = elapsed.as_secs();
        progress_bar.set_position(elapsed_secs);
        sleep(tick_interval).await;
    }
}

/// Logs a message, prompts for user confirmation (y/N), then continues or exits.
///
/// # Arguments
/// * `intro_text` - The demo-specific introduction text describing what the demo will do
/// * `url` - The URL/IP address to connect to (e.g., "127.0.0.1")
/// * `port` - The port number to connect to (e.g., 8439)
///
/// # Behavior
/// - Prints a formatted message: "\n--------\nThis demo will try connect to {url}:{port}, {intro_text}"
/// - Prompts for y/N confirmation
/// - If 'y' or 'Y' is entered, continues execution
/// - If 'n', 'N', empty input, or any other input is entered, exits the program
///
/// # Example
/// ```no_run
/// use robot_demos::confirm_and_continue;
///
/// const INTRO_TEXT: &str = "and control the robot.";
///
/// #[tokio::main]
/// async fn main() {
///     confirm_and_continue(INTRO_TEXT, "127.0.0.1", 8439).await;
/// }
/// ```
pub async fn confirm_and_continue(intro_text: &str, url: &str, port: u16) {
    println!(
        "\n--------\nThis demo is about to connect to {}:{}. {}",
        url, port, intro_text
    );
    print!("Continue? (y/N): ");
    std::io::stdout().flush().expect("Failed to flush stdout");

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut input = String::new();

    reader
        .read_line(&mut input)
        .await
        .expect("Failed to read line");

    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("y") {
        // User confirmed, continue execution
    } else {
        // User declined or entered anything else, exit
        println!("Exiting...");
        std::process::exit(0);
    }
}
