use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use tokio::time::{self, Duration};

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

#[tokio::main]
async fn main() -> io::Result<()> {
    let ptp = PtpClock::open("/dev/ptp0")?;
    let mut ticker = time::interval(Duration::from_millis(1));

    let mut count = 0;
    loop {
        count += 1;
        ticker.tick().await;
        match ptp.now_ms() {
            Ok(ms) => {
                if count % 100 == 0 {
                    println!("{}", ms)
                }
            }
            Err(err) => eprintln!("Failed to read PTP clock: {err}"),
        }
    }
}
