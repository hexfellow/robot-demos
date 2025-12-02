// This is a demo reading time stamp from the robot.
// With correct use of PTP, you can achieve very accurate time synchronization.
// You must share the same PTP master with robots for clock synchronization to work. For more info, check out `src/proto-public-api/README.md` about `PTP Time Synchronization`.
// However, these demos will be provided as is, without any guarantees. Unless specifically stated, we will not provide any explanation for this feature.

use clap::Parser;
use futures_util::StreamExt;
use log::{error, info, warn};
use robot_demos::decode_websocket_message;
use std::fs::File;
use std::io;
use std::net::SocketAddrV4;
use std::os::fd::{AsRawFd, RawFd};
use tokio_tungstenite::MaybeTlsStream;

struct PtpClock {
    _file: File,
    clock_id: libc::clockid_t,
}

impl PtpClock {
    fn open(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        let clock_id = fd_to_clockid(file.as_raw_fd());
        Ok(Self {
            _file: file,
            clock_id,
        })
    }

    fn now_ms(&self) -> io::Result<u128> {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };

        let rc = unsafe { libc::clock_gettime(self.clock_id, &mut ts as *mut _) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        let secs = ts.tv_sec as i128;
        let nanos = ts.tv_nsec as i128;
        Ok((secs * 1_000 + nanos / 1_000_000) as u128)
    }
}

fn fd_to_clockid(fd: RawFd) -> libc::clockid_t {
    // Matches the FD_TO_CLOCKID macro from <linux/ptp_clock.h>
    ((!(fd as libc::clockid_t)) << 3) | 3
}

#[derive(Parser)]
struct Args {
    #[arg(help = "IpV4 address to connect to (e.g. 127.0.0.1:8439)")]
    url: SocketAddrV4,
    #[arg(help = "Device name to use for PTP (e.g. /dev/ptp0)")]
    device: std::path::PathBuf,
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    )
    .init();
    let args = Args::parse();
    let ptp = PtpClock::open(args.device.to_str().unwrap())
        .expect("Failed to open PTP device, are you root? Did you add udev rules?");
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
    let (_, mut ws_stream) = ws_stream.split();
    while let Some(Ok(msg)) = ws_stream.next().await {
        let msg = decode_websocket_message(msg).unwrap();
        if let Some(time_stamp) = msg.time_stamp {
            let local_now = ptp.now_ms().unwrap();
            info!(
                "Time stamp: {:?}, local ptp now: {:?}",
                time_stamp, local_now
            );
        }
    }
}
