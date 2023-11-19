import socket
import random
import json

HOST = "127.0.0.1"
PORT = 60000

s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.connect((HOST, PORT))

req = {
    "jsonrpc": "2.0",
    "id": str(random.randint(0, 1000)),
    "method": "Calc.add",
    "params": {"x": 4, "y": 3},
}
print("Send: ", req)
s.sendall(json.dumps(req).encode())
res = s.recv(1024)
res = json.loads(res)
print("Received: ", res)

req = {
    "jsonrpc": "2.0",
    "id": str(random.randint(0, 1000)),
    "method": "Calc.sub",
    "params": {"x": 4, "y": 3},
}
print("Send: ", req)
s.sendall(json.dumps(req).encode())
res = s.recv(1024)
res = json.loads(res)
print("Received: ", res)

req = {
    "jsonrpc": "2.0",
    "id": str(random.randint(0, 1000)),
    "method": "Calc.ping",
    "params": None,
}
print("Send: ", req)
s.sendall(json.dumps(req).encode())
res = s.recv(1024)
res = json.loads(res)
print("Received: ", res)

req = {
    "jsonrpc": "2.0",
    "id": str(random.randint(0, 1000)),
    "method": "Calc.version",
    "params": None,
}
print("Send: ", req)
s.sendall(json.dumps(req).encode())
res = s.recv(1024)
res = json.loads(res)
print("Received: ", res)

s.close()
