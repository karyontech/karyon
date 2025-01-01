(function() {
    var implementors = Object.fromEntries([["karyon_core",[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"https://docs.rs/bincode/2.0.0-rc.3/bincode/error/enum.DecodeError.html\" title=\"enum bincode::error::DecodeError\">DecodeError</a>&gt; for <a class=\"enum\" href=\"karyon_core/error/enum.Error.html\" title=\"enum karyon_core::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"https://docs.rs/bincode/2.0.0-rc.3/bincode/error/enum.EncodeError.html\" title=\"enum bincode::error::EncodeError\">EncodeError</a>&gt; for <a class=\"enum\" href=\"karyon_core/error/enum.Error.html\" title=\"enum karyon_core::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/alloc/sync/struct.Arc.html\" title=\"struct alloc::sync::Arc\">Arc</a>&lt;<a class=\"struct\" href=\"karyon_core/async_runtime/executor/struct.SmolEx.html\" title=\"struct karyon_core::async_runtime::executor::SmolEx\">Executor</a>&lt;'static&gt;&gt;&gt; for <a class=\"struct\" href=\"karyon_core/async_runtime/executor/struct.Executor.html\" title=\"struct karyon_core::async_runtime::executor::Executor\">Executor</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_core/error/enum.Error.html\" title=\"enum karyon_core::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"karyon_core/async_runtime/executor/struct.SmolEx.html\" title=\"struct karyon_core::async_runtime::executor::SmolEx\">Executor</a>&lt;'static&gt;&gt; for <a class=\"struct\" href=\"karyon_core/async_runtime/executor/struct.Executor.html\" title=\"struct karyon_core::async_runtime::executor::Executor\">Executor</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;Error&gt; for <a class=\"enum\" href=\"karyon_core/error/enum.Error.html\" title=\"enum karyon_core::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;RecvError&gt; for <a class=\"enum\" href=\"karyon_core/error/enum.Error.html\" title=\"enum karyon_core::error::Error\">Error</a>"],["impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;SendError&lt;T&gt;&gt; for <a class=\"enum\" href=\"karyon_core/error/enum.Error.html\" title=\"enum karyon_core::error::Error\">Error</a>"],["impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;Task&lt;T&gt;&gt; for <a class=\"struct\" href=\"karyon_core/async_runtime/task/struct.Task.html\" title=\"struct karyon_core::async_runtime::task::Task\">Task</a>&lt;T&gt;"]]],["karyon_jsonrpc",[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"karyon_core/error/enum.Error.html\" title=\"enum karyon_core::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_jsonrpc/error/enum.Error.html\" title=\"enum karyon_jsonrpc::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"karyon_net/error/enum.Error.html\" title=\"enum karyon_net::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_jsonrpc/error/enum.Error.html\" title=\"enum karyon_jsonrpc::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_jsonrpc/error/enum.Error.html\" title=\"enum karyon_jsonrpc::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://docs.rs/serde_json/1.0.117/serde_json/error/struct.Error.html\" title=\"struct serde_json::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_jsonrpc/error/enum.Error.html\" title=\"enum karyon_jsonrpc::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://docs.rs/serde_json/1.0.117/serde_json/error/struct.Error.html\" title=\"struct serde_json::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_jsonrpc/error/enum.RPCError.html\" title=\"enum karyon_jsonrpc::error::RPCError\">RPCError</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;RecvError&gt; for <a class=\"enum\" href=\"karyon_jsonrpc/error/enum.Error.html\" title=\"enum karyon_jsonrpc::error::Error\">Error</a>"],["impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;SendError&lt;T&gt;&gt; for <a class=\"enum\" href=\"karyon_jsonrpc/error/enum.Error.html\" title=\"enum karyon_jsonrpc::error::Error\">Error</a>"]]],["karyon_net",[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"karyon_core/error/enum.Error.html\" title=\"enum karyon_core::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_net/error/enum.Error.html\" title=\"enum karyon_net::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"karyon_net/endpoint/enum.Endpoint.html\" title=\"enum karyon_net::endpoint::Endpoint\">Endpoint</a>&gt; for <a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/alloc/string/struct.String.html\" title=\"struct alloc::string::String\">String</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_net/error/enum.Error.html\" title=\"enum karyon_net::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;Error&gt; for <a class=\"enum\" href=\"karyon_net/error/enum.Error.html\" title=\"enum karyon_net::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;InvalidDnsNameError&gt; for <a class=\"enum\" href=\"karyon_net/error/enum.Error.html\" title=\"enum karyon_net::error::Error\">Error</a>"],["impl&lt;C, E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"karyon_net/transports/tcp/struct.TcpListener.html\" title=\"struct karyon_net::transports::tcp::TcpListener\">TcpListener</a>&lt;C&gt;&gt; for <a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/alloc/boxed/struct.Box.html\" title=\"struct alloc::boxed::Box\">Box</a>&lt;dyn <a class=\"trait\" href=\"karyon_net/listener/trait.ConnListener.html\" title=\"trait karyon_net::listener::ConnListener\">ConnListener</a>&lt;Message = C::<a class=\"associatedtype\" href=\"karyon_net/codec/trait.Codec.html#associatedtype.Message\" title=\"type karyon_net::codec::Codec::Message\">Message</a>, Error = E&gt;&gt;<div class=\"where\">where\n    C: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"karyon_net/codec/trait.Codec.html\" title=\"trait karyon_net::codec::Codec\">Codec</a>&lt;Error = E&gt; + 'static,\n    E: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt;,</div>"],["impl&lt;C, E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"karyon_net/transports/tls/struct.TlsListener.html\" title=\"struct karyon_net::transports::tls::TlsListener\">TlsListener</a>&lt;C&gt;&gt; for <a class=\"type\" href=\"karyon_net/listener/type.Listener.html\" title=\"type karyon_net::listener::Listener\">Listener</a>&lt;C::<a class=\"associatedtype\" href=\"karyon_net/codec/trait.Codec.html#associatedtype.Message\" title=\"type karyon_net::codec::Codec::Message\">Message</a>, E&gt;<div class=\"where\">where\n    C: <a class=\"trait\" href=\"karyon_net/codec/trait.Codec.html\" title=\"trait karyon_net::codec::Codec\">Codec</a>&lt;Error = E&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + 'static,\n    E: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt;,</div>"],["impl&lt;C, E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"karyon_net/transports/unix/struct.UnixListener.html\" title=\"struct karyon_net::transports::unix::UnixListener\">UnixListener</a>&lt;C&gt;&gt; for <a class=\"type\" href=\"karyon_net/listener/type.Listener.html\" title=\"type karyon_net::listener::Listener\">Listener</a>&lt;C::<a class=\"associatedtype\" href=\"karyon_net/codec/trait.Codec.html#associatedtype.Message\" title=\"type karyon_net::codec::Codec::Message\">Message</a>, E&gt;<div class=\"where\">where\n    C: <a class=\"trait\" href=\"karyon_net/codec/trait.Codec.html\" title=\"trait karyon_net::codec::Codec\">Codec</a>&lt;Error = E&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + 'static,\n    E: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt;,</div>"],["impl&lt;C, E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"karyon_net/transports/ws/struct.WsListener.html\" title=\"struct karyon_net::transports::ws::WsListener\">WsListener</a>&lt;C&gt;&gt; for <a class=\"type\" href=\"karyon_net/listener/type.Listener.html\" title=\"type karyon_net::listener::Listener\">Listener</a>&lt;C::<a class=\"associatedtype\" href=\"karyon_net/codec/websocket/trait.WebSocketCodec.html#associatedtype.Message\" title=\"type karyon_net::codec::websocket::WebSocketCodec::Message\">Message</a>, E&gt;<div class=\"where\">where\n    C: <a class=\"trait\" href=\"karyon_net/codec/websocket/trait.WebSocketCodec.html\" title=\"trait karyon_net::codec::websocket::WebSocketCodec\">WebSocketCodec</a>&lt;Error = E&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + 'static,\n    E: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;Error&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt;,</div>"]]],["karyon_p2p",[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"karyon_core/error/enum.Error.html\" title=\"enum karyon_core::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"karyon_net/error/enum.Error.html\" title=\"enum karyon_net::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"karyon_p2p/monitor/event/enum.ConnEvent.html\" title=\"enum karyon_p2p::monitor::event::ConnEvent\">ConnEvent</a>&gt; for <a class=\"struct\" href=\"karyon_p2p/monitor/struct.ConnectionEvent.html\" title=\"struct karyon_p2p::monitor::ConnectionEvent\">ConnectionEvent</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"karyon_p2p/monitor/event/enum.DiscvEvent.html\" title=\"enum karyon_p2p::monitor::event::DiscvEvent\">DiscvEvent</a>&gt; for <a class=\"struct\" href=\"karyon_p2p/monitor/struct.DiscoveryEvent.html\" title=\"struct karyon_p2p::monitor::DiscoveryEvent\">DiscoveryEvent</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"enum\" href=\"karyon_p2p/monitor/event/enum.PPEvent.html\" title=\"enum karyon_p2p::monitor::event::PPEvent\">PPEvent</a>&gt; for <a class=\"struct\" href=\"karyon_p2p/monitor/struct.PeerPoolEvent.html\" title=\"struct karyon_p2p::monitor::PeerPoolEvent\">PeerPoolEvent</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/core/num/dec2flt/struct.ParseFloatError.html\" title=\"struct core::num::dec2flt::ParseFloatError\">ParseFloatError</a>&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/core/num/error/struct.ParseIntError.html\" title=\"struct core::num::error::ParseIntError\">ParseIntError</a>&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://docs.rs/semver/1.0.23/semver/parse/struct.Error.html\" title=\"struct semver::parse::Error\">Error</a>&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"karyon_p2p/message/struct.PeerMsg.html\" title=\"struct karyon_p2p::message::PeerMsg\">PeerMsg</a>&gt; for <a class=\"struct\" href=\"karyon_p2p/routing_table/entry/struct.Entry.html\" title=\"struct karyon_p2p::routing_table::entry::Entry\">Entry</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"karyon_p2p/peer/peer_id/struct.PeerID.html\" title=\"struct karyon_p2p::peer::peer_id::PeerID\">PeerID</a>&gt; for <a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/alloc/string/struct.String.html\" title=\"struct alloc::string::String\">String</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"karyon_p2p/routing_table/entry/struct.Entry.html\" title=\"struct karyon_p2p::routing_table::entry::Entry\">Entry</a>&gt; for <a class=\"struct\" href=\"karyon_p2p/message/struct.PeerMsg.html\" title=\"struct karyon_p2p::message::PeerMsg\">PeerMsg</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"karyon_p2p/version/struct.VersionInt.html\" title=\"struct karyon_p2p::version::VersionInt\">VersionInt</a>&gt; for <a class=\"struct\" href=\"https://docs.rs/semver/1.0.23/semver/struct.Version.html\" title=\"struct semver::Version\">Version</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;ASN1Error&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;DecodeError&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;Error&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;Error&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;InvalidDnsNameError&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;RecvError&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;X509Error&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;[<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u8.html\">u8</a>; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.array.html\">32</a>]&gt; for <a class=\"struct\" href=\"karyon_p2p/peer/peer_id/struct.PeerID.html\" title=\"struct karyon_p2p::peer::peer_id::PeerID\">PeerID</a>"],["impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;SendError&lt;T&gt;&gt; for <a class=\"enum\" href=\"karyon_p2p/error/enum.Error.html\" title=\"enum karyon_p2p::error::Error\">Error</a>"]]]]);
    if (window.register_implementors) {
        window.register_implementors(implementors);
    } else {
        window.pending_implementors = implementors;
    }
})()
//{"start":57,"fragment_lengths":[3566,2645,7123,8190]}