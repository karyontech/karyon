use std::time::Duration;

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

    let clientc = client.clone();
    smol::spawn(async move {}).detach();

    let (_, sub) = client
        .subscribe("Calc.log_subscribe", ())
        .await
        .expect("Subscribe to log_subscribe method");

    smol::spawn(async move {
        loop {
            let _m = sub.recv().await.unwrap();
        }
    })
    .detach();

    loop {
        Timer::after(Duration::from_secs(1)).await;
        let _: Pong = clientc
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
