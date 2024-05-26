use clap::Parser;

use karyon_jsonrpc::Client;
use karyon_p2p::endpoint::Endpoint;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// RPC server endpoint.
    #[arg(short)]
    rpc_endpoint: Endpoint,
}

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

        smol::spawn(async move {
            loop {
                let _event = sub.recv().await.expect("Receive connection event");
            }
        })
        .detach();

        smol::spawn(async move {
            loop {
                let _event = sub2.recv().await.expect("Receive peer pool event");
            }
        }).await;
    });
}
