import socket
import json

HOST = "127.0.0.1"
PORT = 6000

s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.connect((HOST, PORT))
s.settimeout(5)

for id in range(1000000):
    req = {
        "jsonrpc": "2.0",
        "id": str(id),
        "method": "Calc.ping",
        "params": None,
    }
    print("Send: ", req)
    s.sendall((json.dumps(req)).encode())
    res = s.recv(1024)
    res = json.loads(res)
    print("Received: ", res)

s.close()
