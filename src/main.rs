use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Server {
        #[arg(long, default_value = "127.0.0.1:8000")]
        bind: SocketAddr,
    },
    Client {
        #[arg(long, default_value = "0.0.0.0:0")]
        bind: SocketAddr,
        #[arg(long, default_value = "127.0.0.1:8000")]
        server: SocketAddr,
        #[arg(long, default_value = "Hello from KCP client!")]
        message: String,
        #[arg(long, default_value_t = 1)]
        count: usize,
        #[arg(long, default_value_t = 500)]
        interval_ms: u64,
    },
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args = Cli::parse();
    match args.command {
        Command::Server { bind } => {
            println!("Server running on {}", bind);
            server(bind).await;
        }
        Command::Client {
            bind,
            server,
            message,
            count,
            interval_ms,
        } => {
            println!("Client running on {}", bind);
            client(bind, server, message, count, interval_ms).await;
        }
    }
}

async fn client(
    bind: SocketAddr,
    server: SocketAddr,
    message: String,
    count: usize,
    interval_ms: u64,
) {
    let (kcp_port_owner, tx, rx) = kcp_bindings::KcpPortOwner::new(bind, 1, server)
        .await
        .unwrap();
    let start_time = Instant::now();
    tokio::spawn(async move {
        for i in 0..count {
            let message = format!(
                "{}: Hello from KCP client! {}; Time: {}",
                i,
                message,
                start_time.elapsed().as_millis()
            );
            println!("Sending data to KCP: {:?}", message);
            let data = message.as_bytes().to_vec();
            tx.send(data).await.unwrap();
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
        println!("Sent all data");
        drop(tx);
        tokio::time::sleep(Duration::from_secs(3)).await;
        drop(kcp_port_owner);
        drop(rx);
    });
    std::future::pending::<()>().await;
}

async fn server(bind: SocketAddr) {
    // let peer = SocketAddr::from((std::net::Ipv4Addr::new(127, 0, 0, 1), 12345));
    let peer = SocketAddr::from((std::net::Ipv4Addr::new(172, 18, 28, 177), 51234));
    let (kcp_port_owner, tx, mut rx) = kcp_bindings::KcpPortOwner::new(bind, 1, peer)
        .await
        .unwrap();
    let mut already_got_msg = false;
    loop {
        let data = rx.recv().await.unwrap();
        // println!(
        //     "Received data from KCP: {:?}",
        //     String::from_utf8_lossy(&data)
        // );
        tx.send(data).await.unwrap();
    }
}
