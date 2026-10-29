"""JSON-RPC over HTTP/1.1 and HTTP/2 with httpx.

Needs `pip install 'httpx[http2]'`. Start the server first:
    cargo run --example http_server
"""

import httpx

URL = "http://127.0.0.1:6000"


def call(client, method, params=None):
    req = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    res = client.post(URL, json=req)
    res.raise_for_status()
    return res.http_version, res.json()


# HTTP/2 here uses prior knowledge: no upgrade from HTTP/1.1.
clients = [
    ("HTTP/1.1", httpx.Client()),
    ("HTTP/2", httpx.Client(http1=False, http2=True)),
]

for name, client in clients:
    with client:
        version, res = call(client, "calculator.ping")
        print(f"{name}: sent over {version}: {res}")
        _, res = call(client, "calculator.math.add", {"x": 10, "y": 7})
        print(f"{name}: add: {res}")
        _, res = call(client, "calculator.version")
        print(f"{name}: version: {res}")
