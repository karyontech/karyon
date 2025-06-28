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
 use karyon_eventemitter::{EventEmitter, EventValueTopic, EventValue};

  async {
     let event_emitter = EventEmitter::new();

     #[derive(Hash, PartialEq, Eq, Debug, Clone)]
     enum Topic {
         TopicA,
         TopicB,
     }

     #[derive(Clone, Debug, PartialEq)]
     struct A(usize);

    impl EventValue for A {
         fn event_id() -> &'static str {
             "A"
         }
     }

     let listener = event_emitter.register::<A>(&Topic::TopicA);

     event_emitter.emit_by_topic(&Topic::TopicA, &A(3)) .await;
     let msg: A = listener.recv().await.unwrap();

     #[derive(Clone, Debug, PartialEq)]
     struct B(usize);

     impl EventValue for B {
         fn event_id() -> &'static str {
             "B"
         }
     }

     impl EventValueTopic for B {
         type Topic = Topic;
         fn topic() -> Self::Topic{
             Topic::TopicB
         }
     }

     let listener = event_emitter.register::<B>(&Topic::TopicB);

     event_emitter.emit(&B(3)) .await;
     let msg: B = listener.recv().await.unwrap();

     // ....
  };

 ```


