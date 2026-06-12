use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use robot_demos::proto_public_api_version;
use robot_demos::{
    connect_websocket, decode_websocket_message, init_logger, proto_public_api,
    send_api_down_message_to_websocket,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_tungstenite::accept_async;
use tungstenite::Message;

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Robot Base GUI</title>
  <style>
    :root {
      color-scheme: dark;
      --bg: #0b1020;
      --panel: #121a2e;
      --panel-2: #17213a;
      --text: #e7edf7;
      --muted: #96a5bd;
      --accent: #61dafb;
      --danger: #ff6b6b;
      --ok: #5ce1a6;
      --warn: #ffd166;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: radial-gradient(circle at top left, #1a2d56, var(--bg) 42%);
      color: var(--text);
    }
    main {
      display: grid;
      grid-template-columns: minmax(320px, 1fr) 360px;
      gap: 18px;
      min-height: 100vh;
      padding: 18px;
    }
    section, aside, .card {
      background: rgba(18, 26, 46, 0.88);
      border: 1px solid rgba(255, 255, 255, 0.08);
      border-radius: 18px;
      box-shadow: 0 20px 70px rgba(0, 0, 0, 0.28);
    }
    .stage {
      position: relative;
      overflow: hidden;
      min-height: 560px;
      padding: 18px;
    }
    .grid {
      position: absolute;
      inset: 0;
      background-image:
        linear-gradient(rgba(255,255,255,0.045) 1px, transparent 1px),
        linear-gradient(90deg, rgba(255,255,255,0.045) 1px, transparent 1px);
      background-size: 36px 36px;
      mask-image: radial-gradient(circle at center, black, transparent 75%);
    }
    .robot {
      position: absolute;
      left: 50%;
      top: 50%;
      width: 120px;
      height: 86px;
      transform: translate(-50%, -50%) rotate(0deg);
      border: 3px solid var(--accent);
      border-radius: 24px;
      background: linear-gradient(145deg, rgba(97,218,251,0.24), rgba(97,218,251,0.08));
      box-shadow: 0 0 38px rgba(97,218,251,0.22);
      transition: transform 120ms linear;
    }
    .robot::before {
      content: "";
      position: absolute;
      right: -18px;
      top: 50%;
      transform: translateY(-50%);
      border-left: 18px solid var(--accent);
      border-top: 14px solid transparent;
      border-bottom: 14px solid transparent;
    }
    .trail {
      position: absolute;
      left: 50%;
      top: 50%;
      width: 8px;
      height: 8px;
      transform: translate(-50%, -50%);
      border-radius: 999px;
      background: var(--ok);
      box-shadow: 0 0 18px var(--ok);
    }
    aside { padding: 18px; }
    h1, h2 { margin: 0 0 12px; }
    h1 { font-size: 24px; }
    h2 { font-size: 16px; color: var(--muted); font-weight: 600; }
    .status-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      margin-bottom: 12px;
    }
    .pill {
      padding: 6px 10px;
      border-radius: 999px;
      background: var(--panel-2);
      color: var(--muted);
      font-size: 13px;
    }
    .pill.ok { color: var(--ok); }
    .pill.bad { color: var(--danger); }
    button {
      width: 100%;
      border: 0;
      border-radius: 14px;
      padding: 13px 14px;
      color: #061018;
      background: var(--accent);
      font-weight: 800;
      cursor: pointer;
    }
    button.danger { background: var(--danger); color: white; }
    .metrics {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px;
      margin: 14px 0;
    }
    .card { padding: 12px; background: rgba(23, 33, 58, 0.82); }
    .label { color: var(--muted); font-size: 12px; margin-bottom: 4px; }
    .value { font-size: 20px; font-weight: 800; overflow-wrap: anywhere; }
    .wide { grid-column: 1 / -1; }
    .keys {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 8px;
      margin-top: 10px;
    }
    .key {
      text-align: center;
      padding: 10px;
      border-radius: 12px;
      background: var(--panel-2);
      color: var(--muted);
      border: 1px solid rgba(255,255,255,0.08);
      font-weight: 700;
    }
    .key.active { color: var(--text); border-color: var(--accent); box-shadow: inset 0 0 0 1px var(--accent); }
    pre {
      max-height: 220px;
      overflow: auto;
      padding: 12px;
      border-radius: 12px;
      background: #070b16;
      color: var(--muted);
      font-size: 12px;
    }
    @media (max-width: 860px) {
      main { grid-template-columns: 1fr; }
      .stage { min-height: 420px; }
    }
  </style>
</head>
<body>
  <main>
    <section class="stage">
      <div class="grid"></div>
      <div id="trail" class="trail"></div>
      <div id="robot" class="robot"></div>
    </section>
    <aside>
      <div class="status-row">
        <h1>Robot Base GUI</h1>
        <span id="connection" class="pill bad">offline</span>
      </div>
      <button id="enable">Enable API Control</button>
      <div class="metrics">
        <div class="card"><div class="label">Battery</div><div id="battery" class="value">--</div></div>
        <div class="card"><div class="label">Base State</div><div id="baseState" class="value">--</div></div>
        <div class="card"><div class="label">Command vx / vy</div><div id="cmdLinear" class="value">0.00 / 0.00</div></div>
        <div class="card"><div class="label">Command wz</div><div id="cmdAngular" class="value">0.00</div></div>
        <div class="card"><div class="label">Odom Speed</div><div id="odomSpeed" class="value">--</div></div>
        <div class="card"><div class="label">Odom Pos</div><div id="odomPos" class="value">--</div></div>
        <div class="card wide"><div class="label">Session Holder</div><div id="session" class="value">--</div></div>
      </div>
      <h2>Keyboard Remote</h2>
      <div class="keys">
        <div></div><div id="key-w" class="key">W / Up</div><div></div>
        <div id="key-a" class="key">A / Left</div><div id="key-space" class="key">Space Stop</div><div id="key-d" class="key">D / Right</div>
        <div id="key-q" class="key">Q Rotate</div><div id="key-s" class="key">S / Down</div><div id="key-e" class="key">E Rotate</div>
      </div>
      <p class="label">Hold Shift for faster motion. Disable API control before leaving the page.</p>
      <pre id="raw">{}</pre>
    </aside>
  </main>
  <script>
    const uiWsUrl = "ws://{{UI_WS_ADDR}}";
    const keys = new Set();
    let enabled = false;
    let socket;
    let lastCommand = "";

    const $ = (id) => document.getElementById(id);

    function connect() {
      socket = new WebSocket(uiWsUrl);
      socket.onopen = () => setConnection(true, "ui connected");
      socket.onclose = () => {
        setConnection(false, "offline");
        setTimeout(connect, 1000);
      };
      socket.onerror = () => setConnection(false, "ui error");
      socket.onmessage = (event) => render(JSON.parse(event.data));
    }

    function setConnection(ok, text) {
      $("connection").textContent = text;
      $("connection").className = ok ? "pill ok" : "pill bad";
    }

    function computeCommand() {
      const fast = keys.has("ShiftLeft") || keys.has("ShiftRight");
      const linear = fast ? 0.55 : 0.25;
      const angular = fast ? 1.0 : 0.5;
      const forward = keys.has("KeyW") || keys.has("ArrowUp");
      const back = keys.has("KeyS") || keys.has("ArrowDown");
      const left = keys.has("KeyA") || keys.has("ArrowLeft");
      const right = keys.has("KeyD") || keys.has("ArrowRight");
      const rotLeft = keys.has("KeyQ");
      const rotRight = keys.has("KeyE");
      const stop = keys.has("Space");
      return {
        type: "drive",
        enabled,
        vx: stop ? 0 : (forward ? linear : 0) + (back ? -linear : 0),
        vy: stop ? 0 : (left ? linear : 0) + (right ? -linear : 0),
        wz: stop ? 0 : (rotLeft ? angular : 0) + (rotRight ? -angular : 0),
      };
    }

    function sendCommand(force = false) {
      if (!socket || socket.readyState !== WebSocket.OPEN) return;
      const command = computeCommand();
      const encoded = JSON.stringify(command);
      if (force || encoded !== lastCommand) {
        socket.send(encoded);
        lastCommand = encoded;
      }
      $("cmdLinear").textContent = `${command.vx.toFixed(2)} / ${command.vy.toFixed(2)}`;
      $("cmdAngular").textContent = command.wz.toFixed(2);
    }

    function render(snapshot) {
      if (snapshot.robot_connected) setConnection(true, enabled ? "driving" : "robot connected");
      $("enable").textContent = enabled ? "Disable API Control" : "Enable API Control";
      $("enable").className = enabled ? "danger" : "";
      $("battery").textContent = snapshot.battery_voltage == null
        ? "--"
        : `${snapshot.battery_voltage.toFixed(2)}V (${snapshot.battery_thousandth ?? "--"}/1000)`;
      $("baseState").textContent = snapshot.base_state ?? "--";
      $("session").textContent = snapshot.session_holder || "--";
      if (snapshot.odom) {
        $("odomSpeed").textContent = `${snapshot.odom.speed_x.toFixed(3)}, ${snapshot.odom.speed_y.toFixed(3)}, ${snapshot.odom.speed_z.toFixed(3)}`;
        $("odomPos").textContent = `${snapshot.odom.pos_x.toFixed(3)}, ${snapshot.odom.pos_y.toFixed(3)}, ${snapshot.odom.pos_z.toFixed(3)}`;
        $("robot").style.transform = `translate(-50%, -50%) rotate(${snapshot.odom.pos_z}rad)`;
        $("trail").style.transform = `translate(calc(-50% + ${snapshot.odom.pos_x * 40}px), calc(-50% + ${-snapshot.odom.pos_y * 40}px))`;
      }
      $("raw").textContent = JSON.stringify(snapshot, null, 2);
    }

    function paintKeys() {
      const active = (id, on) => $(id).classList.toggle("active", on);
      active("key-w", keys.has("KeyW") || keys.has("ArrowUp"));
      active("key-s", keys.has("KeyS") || keys.has("ArrowDown"));
      active("key-a", keys.has("KeyA") || keys.has("ArrowLeft"));
      active("key-d", keys.has("KeyD") || keys.has("ArrowRight"));
      active("key-q", keys.has("KeyQ"));
      active("key-e", keys.has("KeyE"));
      active("key-space", keys.has("Space"));
    }

    window.addEventListener("keydown", (event) => {
      if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Space"].includes(event.code)) event.preventDefault();
      keys.add(event.code);
      paintKeys();
      sendCommand();
    });
    window.addEventListener("keyup", (event) => {
      keys.delete(event.code);
      paintKeys();
      sendCommand();
    });
    $("enable").addEventListener("click", () => {
      enabled = !enabled;
      sendCommand(true);
    });
    window.addEventListener("beforeunload", () => {
      enabled = false;
      sendCommand(true);
    });
    setInterval(() => sendCommand(), 50);
    connect();
  </script>
</body>
</html>"#;

#[derive(Parser)]
struct Args {
    #[arg(help = "Robot WebSocket URL host (e.g. 127.0.0.1 or [fe80::500d:96ff:fee1:d60b%3])")]
    robot_url: String,
    #[arg(help = "Robot WebSocket port (e.g. 8439)")]
    robot_port: u16,
    #[arg(
        long,
        default_value = "127.0.0.1:8080",
        help = "HTTP GUI listen address"
    )]
    http_listen: SocketAddr,
    #[arg(
        long,
        default_value = "127.0.0.1:8081",
        help = "UI command WebSocket listen address"
    )]
    ui_ws_listen: SocketAddr,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct DriveCommand {
    enabled: bool,
    vx: f32,
    vy: f32,
    wz: f32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "drive")]
    Drive {
        enabled: bool,
        vx: f32,
        vy: f32,
        wz: f32,
    },
}

#[derive(Clone, Debug, Default, Serialize)]
struct DashboardSnapshot {
    robot_connected: bool,
    api_control_initialized: bool,
    base_state: Option<String>,
    battery_voltage: Option<f32>,
    battery_thousandth: Option<u32>,
    battery_charging: Option<bool>,
    battery_current: Option<f32>,
    session_holder: Option<String>,
    odom: Option<OdometrySnapshot>,
    warning: Option<String>,
    last_error: Option<String>,
    last_update_ms: Option<u128>,
}

#[derive(Clone, Debug, Serialize)]
struct OdometrySnapshot {
    speed_x: f64,
    speed_y: f64,
    speed_z: f64,
    pos_x: f64,
    pos_y: f64,
    pos_z: f64,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    init_logger();
    let args = Args::parse();
    let robot_ws_url = format!("ws://{}:{}", args.robot_url, args.robot_port);

    let (command_tx, command_rx) = watch::channel(DriveCommand::default());
    let (snapshot_tx, snapshot_rx) = watch::channel(DashboardSnapshot::default());
    let snapshot_tx = Arc::new(snapshot_tx);

    tokio::spawn(run_robot_bridge(
        robot_ws_url,
        command_rx,
        Arc::clone(&snapshot_tx),
    ));
    tokio::spawn(serve_http(args.http_listen, args.ui_ws_listen));
    tokio::spawn(serve_ui_websocket(
        args.ui_ws_listen,
        command_tx,
        snapshot_rx,
    ));

    info!("Open http://{} in a browser", args.http_listen);
    std::future::pending::<()>().await;
    Ok(())
}

async fn serve_http(http_addr: SocketAddr, ui_ws_addr: SocketAddr) {
    let listener = match TcpListener::bind(http_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            error!("Failed to bind HTTP GUI server on {http_addr}: {err}");
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(handle_http_request(stream, ui_ws_addr));
            }
            Err(err) => warn!("HTTP accept error: {err}"),
        }
    }
}

async fn handle_http_request(mut stream: TcpStream, ui_ws_addr: SocketAddr) {
    let mut buffer = [0_u8; 1024];
    let _ = stream.read(&mut buffer).await;
    let body = INDEX_HTML.replace("{{UI_WS_ADDR}}", &ui_ws_addr.to_string());
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.as_bytes().len(),
        body
    );
    if let Err(err) = stream.write_all(response.as_bytes()).await {
        warn!("HTTP write error: {err}");
    }
}

async fn serve_ui_websocket(
    ui_ws_addr: SocketAddr,
    command_tx: watch::Sender<DriveCommand>,
    snapshot_rx: watch::Receiver<DashboardSnapshot>,
) {
    let listener = match TcpListener::bind(ui_ws_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            error!("Failed to bind UI WebSocket server on {ui_ws_addr}: {err}");
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let command_tx = command_tx.clone();
                let snapshot_rx = snapshot_rx.clone();
                tokio::spawn(handle_ui_websocket(stream, command_tx, snapshot_rx));
            }
            Err(err) => warn!("UI WebSocket accept error: {err}"),
        }
    }
}

async fn handle_ui_websocket(
    stream: TcpStream,
    command_tx: watch::Sender<DriveCommand>,
    mut snapshot_rx: watch::Receiver<DashboardSnapshot>,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws_stream) => ws_stream,
        Err(err) => {
            warn!("UI WebSocket handshake failed: {err}");
            return;
        }
    };
    let (mut sink, mut stream) = ws_stream.split();

    let initial = match serde_json::to_string(&*snapshot_rx.borrow()) {
        Ok(json) => json,
        Err(err) => {
            warn!("Failed to serialize initial dashboard snapshot: {err}");
            "{}".to_string()
        }
    };
    if sink.send(Message::Text(initial.into())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientMessage>(&text.to_string()) {
                            Ok(ClientMessage::Drive { enabled, vx, vy, wz }) => {
                                let _ = command_tx.send(DriveCommand {
                                    enabled,
                                    vx: vx.clamp(-1.0, 1.0),
                                    vy: vy.clamp(-1.0, 1.0),
                                    wz: wz.clamp(-2.0, 2.0),
                                });
                            }
                            Err(err) => warn!("Invalid UI command: {err}"),
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        let _ = command_tx.send(DriveCommand::default());
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        warn!("UI WebSocket read error: {err}");
                        let _ = command_tx.send(DriveCommand::default());
                        break;
                    }
                }
            }
            changed = snapshot_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let json = match serde_json::to_string(&*snapshot_rx.borrow()) {
                    Ok(json) => json,
                    Err(err) => {
                        warn!("Failed to serialize dashboard snapshot: {err}");
                        continue;
                    }
                };
                if sink.send(Message::Text(json.into())).await.is_err() {
                    let _ = command_tx.send(DriveCommand::default());
                    break;
                }
            }
        }
    }
}

async fn run_robot_bridge(
    robot_ws_url: String,
    mut command_rx: watch::Receiver<DriveCommand>,
    snapshot_tx: Arc<watch::Sender<DashboardSnapshot>>,
) {
    let ws_stream = match connect_websocket(&robot_ws_url).await {
        Ok(ws_stream) => ws_stream,
        Err(err) => {
            update_error(&snapshot_tx, format!("Robot connection failed: {err}"));
            return;
        }
    };
    let (mut ws_sink, mut ws_stream) = ws_stream.split();
    snapshot_tx.send_modify(|snapshot| {
        snapshot.robot_connected = true;
        snapshot.last_error = None;
    });

    if let Err(err) = send_api_down_message_to_websocket(
        &mut ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::SetReportFrequency(
                proto_public_api::ReportFrequency::Rf50Hz as i32,
            )),
            protocol_major_version: proto_public_api_version::CURRENT_PROTOCOL_MAJOR_VERSION,
            protocol_minor_version: proto_public_api_version::CURRENT_PROTOCOL_MINOR_VERSION,
        },
    )
    .await
    {
        update_error(
            &snapshot_tx,
            format!("Failed to set report frequency: {err}"),
        );
        return;
    }

    let reader_snapshot_tx = Arc::clone(&snapshot_tx);
    tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(msg) => match decode_websocket_message(msg, true) {
                    Ok(api_up) => update_snapshot_from_api_up(&reader_snapshot_tx, api_up),
                    Err(err) => update_error(&reader_snapshot_tx, format!("Decode error: {err}")),
                },
                Err(err) => {
                    update_error(
                        &reader_snapshot_tx,
                        format!("Robot WebSocket read error: {err}"),
                    );
                    break;
                }
            }
        }
        reader_snapshot_tx.send_modify(|snapshot| {
            snapshot.robot_connected = false;
            snapshot.api_control_initialized = false;
        });
    });

    let mut initialized = false;
    let mut current_command = command_rx.borrow().clone();
    let mut tick = tokio::time::interval(Duration::from_millis(20));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                if initialized {
                    if let Err(err) = send_move_command(&mut ws_sink, &current_command).await {
                        update_error(&snapshot_tx, format!("Failed to send move command: {err}"));
                        break;
                    }
                }
            }
            changed = command_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let next_command = command_rx.borrow().clone();
                if next_command.enabled != initialized {
                    if let Err(err) = send_initialize_command(&mut ws_sink, next_command.enabled).await {
                        update_error(&snapshot_tx, format!("Failed to change API control state: {err}"));
                        break;
                    }
                    initialized = next_command.enabled;
                    snapshot_tx.send_modify(|snapshot| {
                        snapshot.api_control_initialized = initialized;
                    });
                }
                current_command = next_command;
            }
        }
    }

    if initialized {
        let _ = send_move_command(&mut ws_sink, &DriveCommand::default()).await;
        let _ = send_initialize_command(&mut ws_sink, false).await;
    }
}

async fn send_initialize_command(
    ws_sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    initialized: bool,
) -> Result<(), anyhow::Error> {
    send_api_down_message_to_websocket(
        ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::BaseCommand(
                proto_public_api::BaseCommand {
                    command: Some(
                        proto_public_api::base_command::Command::ApiControlInitialize(initialized),
                    ),
                },
            )),
            protocol_major_version: proto_public_api_version::CURRENT_PROTOCOL_MAJOR_VERSION,
            protocol_minor_version: proto_public_api_version::CURRENT_PROTOCOL_MINOR_VERSION,
        },
    )
    .await
}

async fn send_move_command(
    ws_sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    command: &DriveCommand,
) -> Result<(), anyhow::Error> {
    send_api_down_message_to_websocket(
        ws_sink,
        proto_public_api::ApiDown {
            down: Some(proto_public_api::api_down::Down::BaseCommand(
                proto_public_api::BaseCommand {
                    command: Some(proto_public_api::base_command::Command::SimpleMoveCommand(
                        proto_public_api::SimpleBaseMoveCommand {
                            command: Some(
                                proto_public_api::simple_base_move_command::Command::XyzSpeed(
                                    proto_public_api::XyzSpeed {
                                        speed_x: command.vx,
                                        speed_y: command.vy,
                                        speed_z: command.wz,
                                    },
                                ),
                            ),
                        },
                    )),
                },
            )),
            protocol_major_version: proto_public_api_version::CURRENT_PROTOCOL_MAJOR_VERSION,
            protocol_minor_version: proto_public_api_version::CURRENT_PROTOCOL_MINOR_VERSION,
        },
    )
    .await
}

fn update_snapshot_from_api_up(
    snapshot_tx: &watch::Sender<DashboardSnapshot>,
    api_up: proto_public_api::ApiUp,
) {
    match api_up.status {
        Some(proto_public_api::api_up::Status::BaseStatus(base_status)) => {
            snapshot_tx.send_modify(|snapshot| {
                snapshot.robot_connected = true;
                snapshot.api_control_initialized = base_status.api_control_initialized;
                snapshot.base_state = proto_public_api::BaseState::try_from(base_status.state)
                    .map(|state| state.as_str_name().to_string())
                    .ok();
                snapshot.battery_voltage = Some(base_status.battery_voltage);
                snapshot.battery_thousandth = Some(base_status.battery_thousandth);
                snapshot.battery_charging = base_status.battery_charging;
                snapshot.battery_current = base_status.battery_current;
                snapshot.session_holder = Some(base_status.session_holder.to_string());
                snapshot.warning = base_status.warning.and_then(|warning| {
                    proto_public_api::WarningCategory::try_from(warning)
                        .map(|category| category.as_str_name().to_string())
                        .ok()
                });
                snapshot.odom = base_status.estimated_odometry.map(|odom| OdometrySnapshot {
                    speed_x: odom.speed_x as f64,
                    speed_y: odom.speed_y as f64,
                    speed_z: odom.speed_z as f64,
                    pos_x: odom.pos_x,
                    pos_y: odom.pos_y,
                    pos_z: odom.pos_z,
                });
                snapshot.last_error = None;
                snapshot.last_update_ms = Some(now_ms());
            });
        }
        Some(other) => {
            update_error(
                snapshot_tx,
                format!("Unexpected robot status: {}", status_name(&other)),
            );
        }
        None => {}
    }
}

fn status_name(status: &proto_public_api::api_up::Status) -> &'static str {
    match status {
        proto_public_api::api_up::Status::BaseStatus(_) => "BaseStatus",
        proto_public_api::api_up::Status::ArmStatus(_) => "ArmStatus",
        proto_public_api::api_up::Status::LinearLiftStatus(_) => "LinearLiftStatus",
        proto_public_api::api_up::Status::RotateLiftStatus(_) => "RotateLiftStatus",
    }
}

fn update_error(snapshot_tx: &watch::Sender<DashboardSnapshot>, error: String) {
    warn!("{error}");
    snapshot_tx.send_modify(|snapshot| {
        snapshot.last_error = Some(error);
        snapshot.last_update_ms = Some(now_ms());
    });
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
