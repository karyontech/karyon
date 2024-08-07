# Karyon p2p

Karyon p2p serves as the foundational stack for the Karyon library. It offers
a lightweight, extensible, and customizable peer-to-peer (p2p) network stack
that seamlessly integrates with any p2p project.

## Architecture 

### Discovery 

Karyon p2p uses a customized version of the Kademlia for discovering new peers 
in the network. This approach is based on Kademlia but with several significant 
differences and optimizations. Some of the main changes:

1. Karyon p2p uses TCP for the lookup process, while UDP is used for
   validating and refreshing the routing table. The reason for this choice is
   that the lookup process is infrequent, and the work required to manage
   messages with UDP is largely equivalent to using TCP for this purpose.
   However, for the periodic and efficient sending of numerous Ping messages to
   the entries in the routing table during refreshing, it makes sense to
   use UDP.  

2. In contrast to traditional Kademlia, which often employs 160 buckets,
   Karyon p2p reduces the number of buckets to 32. This optimization is a
   result of the observation that most nodes tend to map into the last few
   buckets, with the majority of other buckets remaining empty.

3. While Kademlia typically uses a 160-bit key to identify a peer, Karyon p2p
   uses a 256-bit key.

> Despite criticisms of Kademlia's vulnerabilities, particularly concerning
> Sybil and Eclipse attacks [[1]](https://eprint.iacr.org/2018/236.pdf)
> [[2]](https://arxiv.org/abs/1908.10141), we chose to use Kademlia because our
> main goal is to build a network focused on sharing data. This choice
> may also assist us in supporting sharding in the future. However, we have made
> efforts to mitigate most of its vulnerabilities. Several projects, including
> BitTorrent, Ethereum, IPFS, and Storj, still rely on Kademlia.

### Peer ID

In the Karyon p2p network, each peer is identified by a 256-bit (32-byte) Peer ID.

### Seeding

At the network's initiation, the client populates the routing table with peers
closest to its key(PeerID) through a seeding process. Once this process is
complete, and the routing table is filled, the client selects a random peer
from the routing table and establishes an outbound connection. This process
continues until all outbound slots are occupied.

The client can optionally provide a listening endpoint to accept inbound 
connections.

### Refreshing

The routing table undergoes periodic refreshment to validate the peers. This
process involves opening a UDP connection with the peers listed in the routing
table and sending a `PING` message. If the peer responds with a `PONG` message,
it means that the peer is still alive. Otherwise, the peer will be removed from
the routing table.

### Handshake

When an inbound or outbound connection is established, the client initiates a
handshake with that connection. If the handshake is successful, the connection
is added to the `PeerPool`.

### Protocols

In the Karyon p2p network, there are two types of protocols: core protocols and
custom protocols. Core protocols, such as the Ping and Handshake protocols,
come prebuilt into Karyon p2p. Custom protocols, however, are ones that you
create to provide the specific functionality your application needs.

Here's an example of a custom protocol:

```rust
pub struct NewProtocol {
    peer: Arc<Peer>,
}

impl NewProtocol {
    fn new(peer: Arc<Peer>) -> Arc<dyn Protocol> {
        Arc::new(Self {
            peer,
        })
    }
}

#[async_trait]
impl Protocol for NewProtocol {
    async fn start(self: Arc<Self>) -> Result<(), Error> {
        loop {
            match self.peer.recv::<Self>().await.expect("Receive msg") {
                ProtocolEvent::Message(msg) => {
                    println!("{:?}", msg);
                }
                ProtocolEvent::Shutdown => {
                    break;
                }
            }
        }
        Ok(())
    }

    fn version() -> Result<Version, Error> {
        "0.2.0, >0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "NEWPROTOCOLID".into()
    }
}

```

## Network Security 

Using TLS is possible for all inbound and outbound connections by enabling the
boolean `enable_tls` field in the configuration. However, implementing TLS for
a p2p network is not trivial and is still unstable, requiring a comprehensive
audit.


## Choosing the async runtime

Karyon p2p currently supports both **smol(async-std)** and **tokio** async runtimes.
The default is **smol**, but if you want to use **tokio**, you need to disable
the default features and then select the `tokio` feature.

## Examples 

You can check out the examples [here](./examples). 

If you have tmux installed, you can run the network simulation script in the 
examples directory to run 12 peers simultaneously.

```bash
$ RUST_LOG=karyon=info ./net_simulation.sh
```
