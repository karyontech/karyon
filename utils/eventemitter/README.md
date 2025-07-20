# karyon-eventemitter

A lightweight, asynchronous event emitter for Rust. It provides a
flexible pub-sub (publish-subscribe) mechanism, enabling decoupled
communication between components via strongly typed events, organized by topics
and event types.

This crate is similar to the EventEmitter in
[Node.js](https://nodejs.org/en/learn/asynchronous-work/the-nodejs-event-emitter)
and can also be used as a [Message
Dispatcher](https://www.enterpriseintegrationpatterns.com/patterns/messaging/MessageDispatcher.html).
See the examples for more details.

## Example

```rust
 use karyon_eventemitter::{EventEmitter, AsEventTopic, EventValue};

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

     impl AsEventTopic for B {
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


