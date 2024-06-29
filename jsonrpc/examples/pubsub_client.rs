use std::time::Duration;

use log::info;
use serde::{Deserialize, Serialize};
use smol::Timer;

use karyon_jsonrpc::Client;

#[derive(Deserialize, Serialize, Debug)]
struct Pong {}

async fn run_client() {
    let client = Client::builder("tcp://127.0.0.1:6000")
        .expect("Create client builder")
        .build()
        .await
        .expect("Build a client");

    let sub = client
        .subscribe("Calc.log_subscribe", ())
        .await
        .expect("Subscribe to log_subscribe method");

    smol::spawn(async move {
        loop {
            let m = sub.recv().await.expect("Receive new log msg");
            info!("Receive new log {m}");
        }
    })
    .detach();

    loop {
        Timer::after(Duration::from_millis(500)).await;
        let _: Pong = client
            .call("Calc.ping", ())
            .await
            .expect("Send ping request");
    }
}

fn main() {
    env_logger::init();
    smol::future::block_on(async {
        smol::spawn(run_client()).await;
    });
}
