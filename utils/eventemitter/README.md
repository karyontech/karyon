# karyon eventemitter

A lightweight, typed async event emitter (pub/sub) for Rust. Components publish and subscribe to
strongly typed events, organized by topic and event type. It does not depend on an async runtime,
so it works with tokio, smol, or any other executor.

It works like the EventEmitter in
[Node.js](https://nodejs.org/en/learn/asynchronous-work/the-nodejs-event-emitter) and can also serve
as a [Message Dispatcher](https://www.enterpriseintegrationpatterns.com/patterns/messaging/MessageDispatcher.html).

Use cases:

- Decouple components of an async application with typed events
- Route messages to listeners by topic
- Expose a monitor or event stream, as karyon_p2p does for peer and discovery events

## Install

```bash
$ cargo add karyon_eventemitter
```

## Example

```rust
 use karyon_eventemitter::{EventEmitter, EventTopic, EventValue};

  async {
     let event_emitter = EventEmitter::new();

     #[derive(Hash, PartialEq, Eq, Debug, Clone)]
     enum Topic {
         TopicA,
         TopicB,
     }

     #[derive(Clone, Debug, PartialEq, EventValue)]
     struct A(usize);

     #[derive(Clone, Debug, PartialEq, EventValue)]
     struct B(usize);

     impl EventTopic for B {
         type Topic = Topic;
         fn topic() -> Self::Topic{
             Topic::TopicB
         }
     }

     #[derive(Clone, Debug, PartialEq, EventValue)]
     struct C(usize);

     let a_listener = event_emitter.register::<A>(&Topic::TopicA);
     let b_listener = event_emitter.register::<B>(&Topic::TopicB);
     // This also listens to Topic B
     let c_listener = event_emitter.register::<C>(&Topic::TopicB);

     event_emitter.emit_by_topic(&Topic::TopicA, &A(3)) .await;
     event_emitter.emit(&B(3)) .await;
     event_emitter.emit_by_topic(&Topic::TopicB, &C(3)) .await;

     let msg: A = a_listener.recv().await.unwrap();
     let msg: B = b_listener.recv().await.unwrap();
     let msg: C = c_listener.recv().await.unwrap();

     // ....
  };

```

Events derive `EventValue`, which implements `AsEventValue` with the type name as the event id.
`#[event_id("name")]` sets a custom id. A listener only receives events with its type's event id on
its topic.

Implement `EventTopic` to give an event a default topic, so it can be sent with `emit` instead of
`emit_by_topic`.

Each listener has a bounded channel, 1000 events by default. `EventEmitter::with_buffer_size` sets
another size. When a listener's buffer is full, `emit` waits until it consumes events. Listeners
whose channel is closed are removed on the next emit. `EventListener::cancel`,
`unregister_topic`, and `clear` remove listeners explicitly.

See [examples/](./examples) for a client of the OpenAI responses stream API.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `derive` | The `#[derive(EventValue)]` macro (default) |
