use clap::Parser;
use serde::{Deserialize, Serialize};

use karyon_jsonrpc::Client;
use karyon_p2p::endpoint::Endpoint;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// RPC server endpoint.
    #[arg(short)]
    rpc_endpoint: Endpoint,
}

#[derive(Deserialize, Serialize)]
struct Pong {}

fn main() {
    smol::block_on(async {
        env_logger::init();
        let cli = Cli::parse();

        let rpc = Client::builder(cli.rpc_endpoint)
            .expect("Create rpc client builder")
            .build()
            .await
            .expect("Create rpc client");

        let (_, sub) = rpc
            .subscribe("MonitorRPC.conn_subscribe", ())
            .await
            .expect("Subscribe to connection events");

        let (_, sub2) = rpc
            .subscribe("MonitorRPC.peer_pool_subscribe", ())
            .await
            .expect("Subscribe to peer pool events");

        let (_, sub3) = rpc
            .subscribe("MonitorRPC.discovery_subscribe", ())
            .await
            .expect("Subscribe to discovery events");

        smol::spawn(async move {
            loop {
                let event = sub.recv().await.expect("Receive connection event");
                println!("Receive new connection event: {event}");
            }
        })
        .detach();

        smol::spawn(async move {
            loop {
                let event = sub2.recv().await.expect("Receive peer pool event");
                println!("Receive new peerpool event: {event}");
            }
        })
        .detach();

        smol::spawn(async move {
            loop {
                let event = sub3.recv().await.expect("Receive discovery event");
                println!("Receive new discovery event: {event}");
            }
        })
        .detach();

        // start ping-pong loop
        loop {
            smol::Timer::after(std::time::Duration::from_secs(1)).await;
            let _: Pong = rpc
                .call("MonitorRPC.ping", ())
                .await
                .expect("Receive pong message");

            println!("Receive pong message");
        }
    });
}
