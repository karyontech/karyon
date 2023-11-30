use serde::{Deserialize, Serialize};
use smol::net::TcpStream;

use karyons_jsonrpc::{Client, ClientConfig};

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
        let conn = TcpStream::connect("127.0.0.1:60000").await.unwrap();
        let config = ClientConfig::default();
        let client = Client::new(conn.into(), config);

        let params = Req { x: 10, y: 7 };
        let result: u32 = client.call("Calc.add", params).await.unwrap();
        println!("result {result}");

        let params = Req { x: 10, y: 7 };
        let result: u32 = client.call("Calc.sub", params).await.unwrap();
        println!("result {result}");

        let result: Pong = client.call("Calc.ping", ()).await.unwrap();
        println!("result {:?}", result);

        let result: String = client.call("Calc.version", ()).await.unwrap();
        println!("result {result}");
    });
}
