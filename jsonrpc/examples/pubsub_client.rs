use serde::{Deserialize, Serialize};
use smol::stream::StreamExt;

use karyon_jsonrpc::Client;

#[derive(Deserialize, Serialize, Debug)]
struct Pong {}

fn main() {
    env_logger::init();
    smol::future::block_on(async {
        let client = Client::builder("tcp://127.0.0.1:6000")
            .expect("Create client builder")
            .build()
            .await
            .expect("Build a client");

        let result: Pong = client
            .call("Calc.ping", ())
            .await
            .expect("Send ping request");

        println!("receive pong msg: {:?}", result);

        let (sub_id, sub) = client
            .subscribe("Calc.log_subscribe", ())
            .await
            .expect("Subscribe to log_subscribe method");

        smol::spawn(async move {
            sub.for_each(|m| {
                println!("Receive new notification: {m}");
            })
            .await
        })
        .detach();

        smol::Timer::after(std::time::Duration::from_secs(5)).await;

        client
            .unsubscribe("Calc.log_unsubscribe", sub_id)
            .await
            .expect("Unsubscribe from log_unsubscirbe method");

        smol::Timer::after(std::time::Duration::from_secs(2)).await;
    });
}
