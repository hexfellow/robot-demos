// This is a (ratatui based) TUI program. It will find all controllers for you to select first, with it's its info.(Keys that are not `hexfellow/controllers/xxxx/robots/`)
// User can then futher select a robot, to view it's API Up messages.

// ## Zenoh Key Hierarchy

// ```
// hexfellow/controllers/<MACHINE_ID>/
//   ├── controller-type   (queryable, returns CONTROLLER_TYPE as string)
//   ├── config            (queryable, returns TotalConfig as JSON string)
//   └── robots/<robot_id>/
//       └── api-up        (pub, ApiUp protobuf bytes, ~100Hz)
// ```
// If protobuf bytes decode fails, show an error message to where it should've be showing ApiUp message.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use prost::Message;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use anyhow::Result;
use robot_demos::proto_public_api;
use tokio::sync::mpsc;

struct ControllerInfo {
    controller_type: Option<String>,
    robots: BTreeSet<String>,
}

#[derive(PartialEq)]
enum Screen {
    ControllerList,
    RobotList,
    ApiUpView,
}

enum ZenohUpdate {
    RobotDiscovered {
        controller_id: String,
        robot_id: String,
    },
    ControllerType {
        controller_id: String,
        controller_type: String,
    },
    ApiUpMessage {
        controller_id: String,
        robot_id: String,
        result: Result<proto_public_api::ApiUp, String>,
    },
}

enum Action {
    Quit,
    Continue,
}

struct App {
    screen: Screen,
    controllers: BTreeMap<String, ControllerInfo>,
    controller_list_state: ListState,
    robot_list_state: ListState,
    selected_controller: Option<String>,
    selected_robot: Option<String>,
    latest_api_up: Option<Result<proto_public_api::ApiUp, String>>,
}

impl App {
    fn new() -> Self {
        Self {
            screen: Screen::ControllerList,
            controllers: BTreeMap::new(),
            controller_list_state: ListState::default(),
            robot_list_state: ListState::default(),
            selected_controller: None,
            selected_robot: None,
            latest_api_up: None,
        }
    }

    fn controller_ids(&self) -> Vec<String> {
        self.controllers.keys().cloned().collect()
    }

    fn robot_ids(&self) -> Vec<String> {
        self.selected_controller
            .as_ref()
            .and_then(|id| self.controllers.get(id))
            .map(|info| info.robots.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let session = zenoh::open(zenoh::Config::default())
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let (tx, mut rx) = mpsc::unbounded_channel::<ZenohUpdate>();

    // Background: subscribe to all api-up publications to discover controllers and robots.
    let session_sub = session.clone();
    let tx_sub = tx.clone();
    tokio::spawn(async move {
        let subscriber = session_sub
            .declare_subscriber("hexfellow/controllers/*/robots/*/api-up")
            .await
            .unwrap();
        while let Ok(sample) = subscriber.recv_async().await {
            let key = sample.key_expr().as_str().to_string();
            // hexfellow/controllers/<cid>/robots/<rid>/api-up
            let parts: Vec<&str> = key.split('/').collect();
            if parts.len() >= 6 {
                let controller_id = parts[2].to_string();
                let robot_id = parts[4].to_string();

                let _ = tx_sub.send(ZenohUpdate::RobotDiscovered {
                    controller_id: controller_id.clone(),
                    robot_id: robot_id.clone(),
                });

                let payload = sample.payload().to_bytes();
                let result = match proto_public_api::ApiUp::decode(payload.as_ref()) {
                    Ok(msg) => Ok(msg),
                    Err(e) => Err(format!("Protobuf decode error: {e}")),
                };
                let _ = tx_sub.send(ZenohUpdate::ApiUpMessage {
                    controller_id,
                    robot_id,
                    result,
                });
            }
        }
    });

    // Background: periodically query controller-type for all controllers.
    let session_query = session.clone();
    let tx_query = tx.clone();
    tokio::spawn(async move {
        loop {
            if let Ok(replies) = session_query
                .get("hexfellow/controllers/*/controller-type")
                .await
            {
                while let Ok(reply) = replies.recv_async().await {
                    if let Ok(sample) = reply.result() {
                        let key = sample.key_expr().as_str().to_string();
                        let parts: Vec<&str> = key.split('/').collect();
                        if parts.len() >= 3 {
                            let controller_id = parts[2].to_string();
                            let payload = sample.payload().to_bytes();
                            let controller_type =
                                String::from_utf8_lossy(&payload).to_string();
                            let _ = tx_query.send(ZenohUpdate::ControllerType {
                                controller_id,
                                controller_type,
                            });
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    let mut terminal = ratatui::init();
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        original_hook(info);
    }));

    let mut app = App::new();
    let result = run_app(&mut terminal, &mut app, &mut rx).await;

    ratatui::restore();
    result?;
    Ok(())
}

async fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    rx: &mut mpsc::UnboundedReceiver<ZenohUpdate>,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if let Action::Quit = handle_key(app, key.code) {
                        return Ok(());
                    }
                }
            }
        }

        while let Ok(update) = rx.try_recv() {
            handle_zenoh_update(app, update);
        }
    }
}

// ── Key handling ────────────────────────────────────────────────────────────

fn handle_key(app: &mut App, key: KeyCode) -> Action {
    match app.screen {
        Screen::ControllerList => match key {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Up => {
                move_selection_up(&mut app.controller_list_state, app.controllers.len());
                Action::Continue
            }
            KeyCode::Down => {
                move_selection_down(&mut app.controller_list_state, app.controllers.len());
                Action::Continue
            }
            KeyCode::Enter => {
                if let Some(idx) = app.controller_list_state.selected() {
                    let ids = app.controller_ids();
                    if let Some(id) = ids.get(idx) {
                        app.selected_controller = Some(id.clone());
                        app.robot_list_state = ListState::default();
                        if !app.robot_ids().is_empty() {
                            app.robot_list_state.select(Some(0));
                        }
                        app.screen = Screen::RobotList;
                    }
                }
                Action::Continue
            }
            _ => Action::Continue,
        },
        Screen::RobotList => match key {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Esc | KeyCode::Backspace => {
                app.screen = Screen::ControllerList;
                app.selected_controller = None;
                Action::Continue
            }
            KeyCode::Up => {
                let count = app.robot_ids().len();
                move_selection_up(&mut app.robot_list_state, count);
                Action::Continue
            }
            KeyCode::Down => {
                let count = app.robot_ids().len();
                move_selection_down(&mut app.robot_list_state, count);
                Action::Continue
            }
            KeyCode::Enter => {
                if let Some(idx) = app.robot_list_state.selected() {
                    let ids = app.robot_ids();
                    if let Some(id) = ids.get(idx) {
                        app.selected_robot = Some(id.clone());
                        app.latest_api_up = None;
                        app.screen = Screen::ApiUpView;
                    }
                }
                Action::Continue
            }
            _ => Action::Continue,
        },
        Screen::ApiUpView => match key {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Esc | KeyCode::Backspace => {
                app.screen = Screen::RobotList;
                app.selected_robot = None;
                app.latest_api_up = None;
                Action::Continue
            }
            _ => Action::Continue,
        },
    }
}

fn move_selection_up(state: &mut ListState, count: usize) {
    if count == 0 {
        return;
    }
    let i = match state.selected() {
        Some(0) | None => count - 1,
        Some(i) => i - 1,
    };
    state.select(Some(i));
}

fn move_selection_down(state: &mut ListState, count: usize) {
    if count == 0 {
        return;
    }
    let i = match state.selected() {
        Some(i) if i >= count - 1 => 0,
        Some(i) => i + 1,
        None => 0,
    };
    state.select(Some(i));
}

// ── Zenoh update handling ───────────────────────────────────────────────────

fn handle_zenoh_update(app: &mut App, update: ZenohUpdate) {
    match update {
        ZenohUpdate::RobotDiscovered {
            controller_id,
            robot_id,
        } => {
            let info = app
                .controllers
                .entry(controller_id)
                .or_insert_with(|| ControllerInfo {
                    controller_type: None,
                    robots: BTreeSet::new(),
                });
            info.robots.insert(robot_id);

            if app.controller_list_state.selected().is_none() && !app.controllers.is_empty() {
                app.controller_list_state.select(Some(0));
            }
        }
        ZenohUpdate::ControllerType {
            controller_id,
            controller_type,
        } => {
            let info = app
                .controllers
                .entry(controller_id)
                .or_insert_with(|| ControllerInfo {
                    controller_type: None,
                    robots: BTreeSet::new(),
                });
            info.controller_type = Some(controller_type);
        }
        ZenohUpdate::ApiUpMessage {
            controller_id,
            robot_id,
            result,
        } => {
            if app.screen == Screen::ApiUpView
                && app.selected_controller.as_deref() == Some(controller_id.as_str())
                && app.selected_robot.as_deref() == Some(robot_id.as_str())
            {
                app.latest_api_up = Some(result);
            }
        }
    }
}

// ── UI rendering ────────────────────────────────────────────────────────────

fn ui(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    match app.screen {
        Screen::ControllerList => render_controller_list(frame, app, chunks[0]),
        Screen::RobotList => render_robot_list(frame, app, chunks[0]),
        Screen::ApiUpView => render_api_up_view(frame, app, chunks[0]),
    }

    let hint = match app.screen {
        Screen::ControllerList => "↑↓ Navigate  Enter Select  q Quit",
        Screen::RobotList => "↑↓ Navigate  Enter Select  Esc Back  q Quit",
        Screen::ApiUpView => "Esc Back  q Quit",
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
        chunks[1],
    );
}

fn render_controller_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .controllers
        .iter()
        .map(|(id, info)| {
            let type_str = info
                .controller_type
                .as_deref()
                .unwrap_or("(querying...)");
            let n = info.robots.len();
            ListItem::new(format!(
                "{id}  [{type_str}]  ({n} robot{})",
                if n == 1 { "" } else { "s" }
            ))
        })
        .collect();

    let title = if items.is_empty() {
        " Controllers (discovering...) "
    } else {
        " Controllers "
    };

    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut app.controller_list_state);
}

fn render_robot_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let robot_ids = app.robot_ids();
    let items: Vec<ListItem> = robot_ids
        .iter()
        .map(|id| ListItem::new(format!("Robot {id}")))
        .collect();

    let cid = app.selected_controller.as_deref().unwrap_or("?");
    let title = format!(" Robots on {cid} ");

    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut app.robot_list_state);
}

fn render_api_up_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let cid = app.selected_controller.as_deref().unwrap_or("?");
    let rid = app.selected_robot.as_deref().unwrap_or("?");
    let title = format!(" {cid} / Robot {rid} — ApiUp ");

    let content: Vec<Line> = match &app.latest_api_up {
        None => vec![Line::from(Span::styled(
            "Waiting for data...",
            Style::default().fg(Color::DarkGray),
        ))],
        Some(Ok(msg)) => format_api_up(msg),
        Some(Err(e)) => vec![Line::from(Span::styled(
            e.clone(),
            Style::default().fg(Color::Red),
        ))],
    };

    let paragraph = Paragraph::new(content)
        .block(Block::bordered().title(title))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

// ── ApiUp formatting ────────────────────────────────────────────────────────

fn format_api_up(msg: &proto_public_api::ApiUp) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    let robot_type = proto_public_api::RobotType::try_from(msg.robot_type)
        .map(|rt| rt.as_str_name().to_string())
        .unwrap_or_else(|_| format!("Unknown({})", msg.robot_type));
    lines.push(Line::from(vec![
        Span::styled("Robot Type: ", Style::default().fg(Color::Yellow)),
        Span::raw(robot_type),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Protocol: ", Style::default().fg(Color::Yellow)),
        Span::raw(format!(
            "v{}.{}",
            msg.protocol_major_version, msg.protocol_minor_version
        )),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Session ID: ", Style::default().fg(Color::Yellow)),
        Span::raw(msg.session_id.to_string()),
    ]));

    let freq = proto_public_api::ReportFrequency::try_from(msg.report_frequency)
        .map(|f| f.as_str_name().to_string())
        .unwrap_or_else(|_| format!("Unknown({})", msg.report_frequency));
    lines.push(Line::from(vec![
        Span::styled("Report Freq: ", Style::default().fg(Color::Yellow)),
        Span::raw(freq),
    ]));

    if let Some(v) = msg.main_bus_voltage {
        lines.push(Line::from(vec![
            Span::styled("Bus Voltage: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{v:.2}V")),
        ]));
    }

    if let Some(ref log) = msg.log {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("Log: {log}"),
            Style::default().fg(Color::Red),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "--- Status ---",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));

    match &msg.status {
        Some(proto_public_api::api_up::Status::BaseStatus(s)) => format_base_status(&mut lines, s),
        Some(proto_public_api::api_up::Status::ArmStatus(s)) => format_arm_status(&mut lines, s),
        Some(proto_public_api::api_up::Status::LinearLiftStatus(s)) => {
            format_linear_lift_status(&mut lines, s)
        }
        Some(proto_public_api::api_up::Status::RotateLiftStatus(s)) => {
            format_rotate_lift_status(&mut lines, s)
        }
        Some(proto_public_api::api_up::Status::HexCanApiCanAnyFrames(_)) => {
            lines.push(Line::from("  CAN Forwarding Mode"));
        }
        None => {
            lines.push(Line::from("  (no status)"));
        }
    }

    if !msg.secondary_device_status.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "--- Secondary Devices ---",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
        for dev in &msg.secondary_device_status {
            format_secondary_device(&mut lines, dev);
        }
    }

    if let Some(ref ts) = msg.time_stamp {
        lines.push(Line::from(""));
        if let Some(ref mono) = ts.monotonic_time_stamp {
            lines.push(Line::from(format!(
                "Monotonic: {}s {}ns",
                mono.seconds, mono.nanoseconds
            )));
        }
        if let Some(ref ptp) = ts.ptp_time_stamp {
            lines.push(Line::from(format!(
                "PTP: {}s {}ns (calibrated={})",
                ptp.seconds, ptp.nanoseconds, ptp.calibrated
            )));
        }
    }

    lines
}

fn format_base_status(lines: &mut Vec<Line<'static>>, s: &proto_public_api::BaseStatus) {
    let state = proto_public_api::BaseState::try_from(s.state)
        .map(|st| st.as_str_name().to_string())
        .unwrap_or_else(|_| format!("Unknown({})", s.state));
    lines.push(Line::from(format!("  State: {state}")));
    lines.push(Line::from(format!(
        "  API Initialized: {}",
        s.api_control_initialized
    )));
    lines.push(Line::from(format!(
        "  Battery: {:.2}V ({}/1000)",
        s.battery_voltage, s.battery_thousandth
    )));
    if let Some(charging) = s.battery_charging {
        lines.push(Line::from(format!("  Charging: {charging}")));
    }
    if let Some(current) = s.battery_current {
        lines.push(Line::from(format!("  Battery Current: {current:.2}A")));
    }
    lines.push(Line::from(format!(
        "  Session Holder: {}",
        s.session_holder
    )));
    if let Some(ref odom) = s.estimated_odometry {
        lines.push(Line::from(format!(
            "  Odom Speed: ({:.3}, {:.3}, {:.3})",
            odom.speed_x, odom.speed_y, odom.speed_z
        )));
        lines.push(Line::from(format!(
            "  Odom Pos:   ({:.3}, {:.3}, {:.3})",
            odom.pos_x, odom.pos_y, odom.pos_z
        )));
    }
    format_motors(lines, &s.motor_status);
    if let Some(ref ps) = s.parking_stop_detail {
        format_parking_stop(lines, ps);
    }
    if let Some(w) = s.warning {
        let wc = proto_public_api::WarningCategory::try_from(w)
            .map(|c| c.as_str_name().to_string())
            .unwrap_or_else(|_| format!("Unknown({w})"));
        lines.push(Line::from(Span::styled(
            format!("  Warning: {wc}"),
            Style::default().fg(Color::Yellow),
        )));
    }
}

fn format_arm_status(lines: &mut Vec<Line<'static>>, s: &proto_public_api::ArmStatus) {
    lines.push(Line::from(format!(
        "  API Initialized: {}",
        s.api_control_initialized
    )));
    lines.push(Line::from(format!("  Calibrated: {}", s.calibrated)));
    lines.push(Line::from(format!(
        "  Session Holder: {}",
        s.session_holder
    )));
    format_motors(lines, &s.motor_status);
    if let Some(ref ps) = s.parking_stop_detail {
        format_parking_stop(lines, ps);
    }
}

fn format_linear_lift_status(
    lines: &mut Vec<Line<'static>>,
    s: &proto_public_api::LinearLiftStatus,
) {
    let state = proto_public_api::LiftState::try_from(s.state)
        .map(|st| st.as_str_name().to_string())
        .unwrap_or_else(|_| format!("Unknown({})", s.state));
    lines.push(Line::from(format!("  State: {state}")));
    lines.push(Line::from(format!("  Calibrated: {}", s.calibrated)));
    lines.push(Line::from(format!(
        "  Position: {} / {} (max)",
        s.current_pos, s.max_pos
    )));
    lines.push(Line::from(format!(
        "  Speed: {} / {} (max)  PPR: {}",
        s.speed, s.max_speed, s.pulse_per_rotation
    )));
    if let Some(ref ps) = s.parking_stop_detail {
        format_parking_stop(lines, ps);
    }
}

fn format_rotate_lift_status(
    lines: &mut Vec<Line<'static>>,
    s: &proto_public_api::RotateLiftStatus,
) {
    let state = proto_public_api::LiftState::try_from(s.state)
        .map(|st| st.as_str_name().to_string())
        .unwrap_or_else(|_| format!("Unknown({})", s.state));
    lines.push(Line::from(format!("  State: {state}")));
    lines.push(Line::from(format!("  Calibrated: {}", s.calibrated)));
    lines.push(Line::from(format!(
        "  Session Holder: {}",
        s.session_holder
    )));
    format_motors(lines, &s.motor_status);
    if let Some(ref ps) = s.parking_stop_detail {
        format_parking_stop(lines, ps);
    }
}

fn format_motors(lines: &mut Vec<Line<'static>>, motors: &[proto_public_api::MotorStatus]) {
    if motors.is_empty() {
        return;
    }
    lines.push(Line::from(format!("  Motors ({}):", motors.len())));
    for (i, m) in motors.iter().enumerate() {
        let errors: Vec<String> = m
            .error
            .iter()
            .filter_map(|e| {
                proto_public_api::MotorError::try_from(*e)
                    .ok()
                    .map(|me| me.as_str_name().to_string())
            })
            .collect();
        let error_str = if errors.is_empty() {
            String::new()
        } else {
            format!("  ERR: {}", errors.join(", "))
        };
        lines.push(Line::from(format!(
            "    [{i}] pos={} spd={:.2} trq={:.2} ppr={}{}",
            m.position, m.speed, m.torque, m.pulse_per_rotation, error_str
        )));
    }
}

fn format_parking_stop(lines: &mut Vec<Line<'static>>, ps: &proto_public_api::ParkingStopDetail) {
    let cat = proto_public_api::ParkingStopCategory::try_from(ps.category)
        .map(|c| c.as_str_name().to_string())
        .unwrap_or_else(|_| format!("Unknown({})", ps.category));
    lines.push(Line::from(Span::styled(
        format!(
            "  PARKING STOP: {} [{}] clearable={}",
            ps.reason, cat, ps.is_remotely_clearable
        ),
        Style::default().fg(Color::Red),
    )));
}

fn format_secondary_device(
    lines: &mut Vec<Line<'static>>,
    dev: &proto_public_api::SecondaryDeviceStatus,
) {
    let dev_type = proto_public_api::SecondaryDeviceType::try_from(dev.device_type)
        .map(|t| t.as_str_name().to_string())
        .unwrap_or_else(|_| format!("Unknown({})", dev.device_type));
    lines.push(Line::from(format!(
        "  Device {} [{}]",
        dev.device_id, dev_type
    )));
    match &dev.status {
        Some(proto_public_api::secondary_device_status::Status::ImuData(imu)) => {
            if let Some(ref acc) = imu.acceleration {
                lines.push(Line::from(format!(
                    "    Accel: ({:.3}, {:.3}, {:.3})",
                    acc.ax, acc.ay, acc.az
                )));
            }
            if let Some(ref gyro) = imu.angular_velocity {
                lines.push(Line::from(format!(
                    "    Gyro:  ({:.3}, {:.3}, {:.3})",
                    gyro.wx, gyro.wy, gyro.wz
                )));
            }
            if let Some(ref q) = imu.quaternion {
                lines.push(Line::from(format!(
                    "    Quat:  ({:.3}, {:.3}, {:.3}, {:.3})",
                    q.qw, q.qx, q.qy, q.qz
                )));
            }
        }
        Some(proto_public_api::secondary_device_status::Status::HandStatus(hand)) => {
            for (i, m) in hand.motor_status.iter().enumerate() {
                lines.push(Line::from(format!(
                    "    Motor {i}: pos={} spd={:.2} trq={:.2}",
                    m.position, m.speed, m.torque
                )));
            }
        }
        Some(proto_public_api::secondary_device_status::Status::GamepadRead(gp)) => {
            lines.push(Line::from(format!(
                "    L-Stick: ({:.2}, {:.2})  R-Stick: ({:.2}, {:.2})",
                gp.left_stick_x, gp.left_stick_y, gp.right_stick_x, gp.right_stick_y
            )));
            lines.push(Line::from(format!(
                "    Triggers: L={:.2} R={:.2}  Bumpers: L={} R={}",
                gp.left_trigger, gp.right_trigger, gp.left_bumper, gp.right_bumper
            )));
        }
        Some(proto_public_api::secondary_device_status::Status::Hello1j1t4bStatus(h)) => {
            lines.push(Line::from(format!(
                "    Joystick: ({:.2}, {:.2})  Trigger: {:.2}",
                h.joystick_x, h.joystick_y, h.trigger
            )));
            lines.push(Line::from(format!(
                "    Buttons: X={} Y={} Z={} W={}",
                h.btn_x, h.btn_y, h.btn_z, h.btn_w
            )));
        }
        None => {
            lines.push(Line::from("    (no data)"));
        }
    }
}
