# karyons

> :warning: **Warning: This Project is a Work in Progress**

We are in the process of developing an infrastructure for peer-to-peer,
decentralized, and collaborative software.

Join us on:

- [Discord](https://discord.gg/NDSpDdck)
- [Matrix](https://matrix.to/#/#karyons:matrix.org)

## karyons p2p

karyons p2p serves as the foundational stack for the karyons project. It
offers a modular, lightweight, and customizable p2p network stack that 
seamlessly integrates with any p2p project.

### Architecture 

#### Discovery 

karyons p2p uses a customized version of the Kademlia for discovering new peers 
in the network. This approach is based on Kademlia but with several significant 
differences and optimizations. Some of the main changes:

1. karyons p2p uses TCP for the lookup process, while UDP is used for
   validating and refreshing the routing table. The reason for this choice is
   that the lookup process is infrequent, and the work required to manage
   messages with UDP is largely equivalent to using TCP for this purpose.
   However, for the periodic and efficient sending of numerous Ping messages to
   the entries in the routing table during refreshing, it makes sense to
   use UDP.  

2. In contrast to traditional Kademlia, which often employs 160 buckets,
   karyons p2p reduces the number of buckets to 32. This optimization is a
   result of the observation that most nodes tend to map into the last few
   buckets, with the majority of other buckets remaining empty.

3. While Kademlia typically uses a 160-bit key to identify a peer, karyons p2p
   uses a 256-bit key.

> Despite criticisms of Kademlia's vulnerabilities, particularly concerning
> Sybil and Eclipse attacks [[1]](https://eprint.iacr.org/2018/236.pdf)
> [[2]](https://arxiv.org/abs/1908.10141), we chose to use Kademlia because our
> main goal is to build an infrastructure focused on sharing data. This choice
> may also assist us in supporting sharding in the future. However, we have made
> efforts to mitigate most of its vulnerabilities. Several projects, including
> BitTorrent, Ethereum, IPFS, and Storj, still rely on Kademlia.

#### Peer IDs

Peers in the karyons p2p network are identified by their 256-bit (32-byte) Peer IDs.

#### Seeding

At the network's initiation, the client populates the routing table with peers
closest to its key(PeerID) through a seeding process. Once this process is
complete, and the routing table is filled, the client selects a random peer
from the routing table and establishes an outbound connection. This process
continues until all outbound slots are occupied.

The client can optionally provide a listening endpoint to accept inbound 
connections.

#### Handshake

When an inbound or outbound connection is established, the client initiates a
handshake with that connection. If the handshake is successful, the connection
is added to the PeerPool.

#### Protocols

In the karyons p2p network, we have two types of protocols: core protocols and
custom protocols. Core protocols are prebuilt into karyons p2p, such as the
Ping protocol used to maintain connections. Custom protocols, on the other
hand, are protocols that you define for your application to provide its core
functionality.

Here's an example of a custom protocol:

```rust
pub struct NewProtocol {
    peer: ArcPeer,
}

impl NewProtocol {
    fn new(peer: ArcPeer) -> ArcProtocol {
        Arc::new(Self {
            peer,
        })
    }
}

#[async_trait]
impl Protocol for NewProtocol {
    async fn start(self: Arc<Self>, ex: Arc<Executor<'_>>) -> Result<(), P2pError> {
        let listener = self.peer.register_listener::<Self>().await;
        loop {
            let event = listener.recv().await.unwrap();

            match event {
                ProtocolEvent::Message(msg) => {
                    println!("{:?}", msg);
                }
                ProtocolEvent::Shutdown => {
                    break;
                }
            }
        }

        listener.cancel().await;
        Ok(())
    }

    fn version() -> Result<Version, P2pError> {
        "0.2.0, >0.1.0".parse()
    }

    fn id() -> ProtocolID {
        "NEWPROTOCOLID".into()
    }
}

```

Whenever a new peer is added to the PeerPool, all the protocols, including 
your custom protocols, will automatically start running with the newly connected peer.

### Network Security

It's obvious that connections in karyons p2p are not secure at the moment, as
it currently only supports TCP connections. However, we are currently working
on adding support for TLS connections.

### Usage

You can check out the examples [here](./karyons_p2p/examples). 

If you have tmux installed, you can run the network simulation script in the 
examples directory to run 12 peers simultaneously.

    RUST_LOG=karyons=debug ./net_simulation.sh

## Contribution

Feel free to open a pull request. We appreciate your help.

## License

All the code in this repository is licensed under the GNU General Public
License, version 3 (GPL-3.0). You can find a copy of the license in the
[LICENSE](./LICENSE) file.

