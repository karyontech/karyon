use std::time::Duration;

use log::info;
use serde::{Deserialize, Serialize};
use smol::Timer;

use karyon_jsonrpc::client::ClientBuilder;

#[derive(Deserialize, Serialize)]
struct Req {
    x: u32,
    y: u32,
}

#[derive(Deserialize, Serialize, Debug)]
struct Pong {}

fn main() {
    env_logger::init();
    smol::future::block_on(async {
        let client = ClientBuilder::new("tcp://127.0.0.1:7878")
            .expect("Create client builder")
            .build()
            .await
            .expect("Create rpc client");

        let result: String = client
            .call("Calc.version", ())
            .await
            .expect("Call Calc.version method");
        info!("Version result: {result}");

        loop {
            Timer::after(Duration::from_millis(100)).await;
            let result: Pong = client
                .call("Calc.ping", ())
                .await
                .expect("Call Calc.ping method");
            info!("Ping result:  {:?}", result);
        }
    });
}
