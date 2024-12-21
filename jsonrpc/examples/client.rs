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
        let client = ClientBuilder::new("tcp://127.0.0.1:6000")
            .expect("Create client builder")
            .build()
            .await
            .expect("Create rpc client");

        let params = Req { x: 10, y: 7 };
        let result: u32 = client
            .call("Calc.add", params)
            .await
            .expect("Call Calc.add method");
        info!("Add result: {result}");

        let params = Req { x: 10, y: 7 };
        let result: u32 = client
            .call("Calc.sub", params)
            .await
            .expect("Call Calc.sub method");
        info!("Sub result: {result}");

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
