use std::{
    any::Any,
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Weak},
};

use async_channel::{Receiver, Sender};
use chrono::{DateTime, Utc};
use futures_util::stream::{FuturesUnordered, StreamExt};
use log::{debug, error};

use crate::{async_runtime::lock::Mutex, util::random_32, Result};

const CHANNEL_BUFFER_SIZE: usize = 1000;

pub type EventListenerID = u32;

type Listeners<T> = HashMap<T, HashMap<String, HashMap<EventListenerID, Sender<Event>>>>;

/// EventSys emits events to registered listeners based on topics.
/// # Example
///
/// ```
/// use karyon_core::event::{EventSys, EventValueTopic, EventValue};
///
///  async {
///     let event_sys = EventSys::new();
///
///     #[derive(Hash, PartialEq, Eq, Debug, Clone)]
///     enum Topic {
///         TopicA,
///         TopicB,
///     }
///
///     #[derive(Clone, Debug, PartialEq)]
///     struct A(usize);
///
///    impl EventValue for A {
///         fn id() -> &'static str {
///             "A"
///         }
///     }
///
///     let listener = event_sys.register::<A>(&Topic::TopicA).await;
///
///     event_sys.emit_by_topic(&Topic::TopicA, &A(3)) .await;
///     let msg: A = listener.recv().await.unwrap();
///
///     #[derive(Clone, Debug, PartialEq)]
///     struct B(usize);
///
///     impl EventValue for B {
///         fn id() -> &'static str {
///             "B"
///         }
///     }
///
///     impl EventValueTopic for B {
///         type Topic = Topic;
///         fn topic() -> Self::Topic{
///             Topic::TopicB
///         }
///     }
///
///     let listener = event_sys.register::<B>(&Topic::TopicB).await;
///
///     event_sys.emit(&B(3)) .await;
///     let msg: B = listener.recv().await.unwrap();
///
///     // ....
///  };
///
/// ```
///
pub struct EventSys<T> {
    listeners: Mutex<Listeners<T>>,
    listener_buffer_size: usize,
}

impl<T> EventSys<T>
where
    T: std::hash::Hash + Eq + std::fmt::Debug + Clone,
{
    /// Creates a new [`EventSys`]
    pub fn new() -> Arc<EventSys<T>> {
        Arc::new(Self {
            listeners: Mutex::new(HashMap::new()),
            listener_buffer_size: CHANNEL_BUFFER_SIZE,
        })
    }

    /// Creates a new [`EventSys`] with the provided buffer size for the
    /// [`EventListener`] channel.
    ///
    /// This is important to control the memory used by the listener channel.
    /// If the consumer for the event listener can't keep up with the new events
    /// coming, then the channel buffer will fill with new events, and if the
    /// buffer is full, the emit function will block until the listener
    /// starts to consume the buffered events.
    ///
    /// If `size` is zero, this function will panic.
    pub fn with_buffer_size(size: usize) -> Arc<EventSys<T>> {
        Arc::new(Self {
            listeners: Mutex::new(HashMap::new()),
            listener_buffer_size: size,
        })
    }

    /// Emits an event to the listeners.
    ///
    /// The event must implement the [`EventValueTopic`] trait to indicate the
    /// topic of the event. Otherwise, you can use `emit_by_topic()`.
    pub async fn emit<E: EventValueTopic<Topic = T> + Clone>(&self, value: &E) {
        let topic = E::topic();
        self.emit_by_topic(&topic, value).await;
    }

    /// Emits an event to the listeners.
    pub async fn emit_by_topic<E: EventValueAny + EventValue + Clone>(&self, topic: &T, value: &E) {
        let value: Arc<dyn EventValueAny> = Arc::new(value.clone());
        let event = Event::new(value);

        let mut topics = self.listeners.lock().await;

        if !topics.contains_key(topic) {
            debug!(
                "Failed to emit an event to a non-existent topic {:?}",
                topic
            );
            return;
        }

        let event_ids = topics.get_mut(topic).unwrap();
        let event_id = E::id().to_string();

        if !event_ids.contains_key(&event_id) {
            debug!("Failed to emit an event: unknown event id {:?}", event_id);
            return;
        }

        let mut results = FuturesUnordered::new();

        let listeners = event_ids.get_mut(&event_id).unwrap();
        for (listener_id, listener) in listeners.iter() {
            let result = async { (*listener_id, listener.send(event.clone()).await) };
            results.push(result);
        }

        let mut failed_listeners = vec![];
        while let Some((id, fut_err)) = results.next().await {
            if let Err(err) = fut_err {
                debug!("Failed to emit event for topic {:?}: {}", topic, err);
                failed_listeners.push(id);
            }
        }
        drop(results);

        for listener_id in failed_listeners.iter() {
            listeners.remove(listener_id);
        }
    }

    /// Registers a new event listener for the given topic.
    pub async fn register<E: EventValueAny + EventValue + Clone>(
        self: &Arc<Self>,
        topic: &T,
    ) -> EventListener<T, E> {
        let chan = async_channel::bounded(self.listener_buffer_size);

        let topics = &mut self.listeners.lock().await;

        if !topics.contains_key(topic) {
            topics.insert(topic.clone(), HashMap::new());
        }

        let event_ids = topics.get_mut(topic).unwrap();
        let event_id = E::id().to_string();

        if !event_ids.contains_key(&event_id) {
            event_ids.insert(event_id.clone(), HashMap::new());
        }

        let listeners = event_ids.get_mut(&event_id).unwrap();

        let mut listener_id = random_32();
        // Generate a new one if listener_id already exists
        while listeners.contains_key(&listener_id) {
            listener_id = random_32();
        }

        let listener =
            EventListener::new(listener_id, Arc::downgrade(self), chan.1, &event_id, topic);

        listeners.insert(listener_id, chan.0);

        listener
    }

    /// Removes the event listener attached to the given topic.
    async fn remove(&self, topic: &T, event_id: &str, listener_id: &EventListenerID) {
        let topics = &mut self.listeners.lock().await;
        if !topics.contains_key(topic) {
            error!("Failed to remove a non-existent topic");
            return;
        }

        let event_ids = topics.get_mut(topic).unwrap();
        if !event_ids.contains_key(event_id) {
            error!("Failed to remove a non-existent event id");
            return;
        }

        let listeners = event_ids.get_mut(event_id).unwrap();
        if listeners.remove(listener_id).is_none() {
            error!("Failed to remove a non-existent event listener");
        }
    }
}

/// EventListener listens for and receives events from the [`EventSys`].
pub struct EventListener<T, E> {
    id: EventListenerID,
    recv_chan: Receiver<Event>,
    event_sys: Weak<EventSys<T>>,
    event_id: String,
    topic: T,
    phantom: PhantomData<E>,
}

impl<T, E> EventListener<T, E>
where
    T: std::hash::Hash + Eq + Clone + std::fmt::Debug,
    E: EventValueAny + Clone + EventValue,
{
    /// Creates a new [`EventListener`].
    fn new(
        id: EventListenerID,
        event_sys: Weak<EventSys<T>>,
        recv_chan: Receiver<Event>,
        event_id: &str,
        topic: &T,
    ) -> EventListener<T, E> {
        Self {
            id,
            recv_chan,
            event_sys,
            event_id: event_id.to_string(),
            topic: topic.clone(),
            phantom: PhantomData,
        }
    }

    /// Receives the next event.
    pub async fn recv(&self) -> Result<E> {
        match self.recv_chan.recv().await {
            Ok(event) => match ((*event.value).value_as_any()).downcast_ref::<E>() {
                Some(v) => Ok(v.clone()),
                None => unreachable!("Failed to downcast the event value."),
            },
            Err(err) => {
                error!("Failed to receive new event: {err}");
                self.cancel().await;
                Err(err.into())
            }
        }
    }

    /// Cancels the event listener and removes it from the [`EventSys`].
    pub async fn cancel(&self) {
        if let Some(es) = self.event_sys.upgrade() {
            es.remove(&self.topic, &self.event_id, &self.id).await;
        }
    }

    /// Returns the topic for this event listener.
    pub fn topic(&self) -> &T {
        &self.topic
    }

    /// Returns the event id for this event listener.
    pub fn event_id(&self) -> &String {
        &self.event_id
    }
}

/// An event within the [`EventSys`].
#[derive(Clone, Debug)]
pub struct Event {
    /// The time at which the event was created.
    created_at: DateTime<Utc>,
    /// The value of the Event.
    value: Arc<dyn EventValueAny>,
}

impl Event {
    /// Creates a new Event.
    pub fn new(value: Arc<dyn EventValueAny>) -> Self {
        Self {
            created_at: Utc::now(),
            value,
        }
    }
}

impl std::fmt::Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}: {:?}", self.created_at, self.value)
    }
}

pub trait EventValueAny: Any + Send + Sync + std::fmt::Debug {
    fn value_as_any(&self) -> &dyn Any;
}

impl<T: Send + Sync + std::fmt::Debug + Any> EventValueAny for T {
    fn value_as_any(&self) -> &dyn Any {
        self
    }
}

pub trait EventValue: EventValueAny {
    fn id() -> &'static str
    where
        Self: Sized;
}

pub trait EventValueTopic: EventValueAny + EventValue {
    type Topic;
    fn topic() -> Self::Topic
    where
        Self: Sized;
}

#[cfg(test)]
mod tests {
    use crate::async_runtime::block_on;

    use super::*;

    #[derive(Hash, PartialEq, Eq, Debug, Clone)]
    enum Topic {
        TopicA,
        TopicB,
        TopicC,
        TopicD,
        TopicE,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct A {
        a_value: usize,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct B {
        b_value: usize,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct C {
        c_value: usize,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct E {
        e_value: usize,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct F {
        f_value: usize,
    }

    impl EventValue for A {
        fn id() -> &'static str {
            "A"
        }
    }

    impl EventValue for B {
        fn id() -> &'static str {
            "B"
        }
    }

    impl EventValue for C {
        fn id() -> &'static str {
            "C"
        }
    }

    impl EventValue for E {
        fn id() -> &'static str {
            "E"
        }
    }

    impl EventValue for F {
        fn id() -> &'static str {
            "F"
        }
    }

    impl EventValueTopic for C {
        type Topic = Topic;
        fn topic() -> Self::Topic {
            Topic::TopicC
        }
    }

    #[test]
    fn test_event_sys() {
        block_on(async move {
            let event_sys = EventSys::<Topic>::new();

            let a_listener = event_sys.register::<A>(&Topic::TopicA).await;
            let b_listener = event_sys.register::<B>(&Topic::TopicB).await;

            event_sys
                .emit_by_topic(&Topic::TopicA, &A { a_value: 3 })
                .await;
            event_sys
                .emit_by_topic(&Topic::TopicB, &B { b_value: 5 })
                .await;

            let msg = a_listener.recv().await.unwrap();
            assert_eq!(msg, A { a_value: 3 });

            let msg = b_listener.recv().await.unwrap();
            assert_eq!(msg, B { b_value: 5 });

            // register the same event type to different topics
            let c_listener = event_sys.register::<C>(&Topic::TopicC).await;
            let d_listener = event_sys.register::<C>(&Topic::TopicD).await;

            event_sys.emit(&C { c_value: 10 }).await;
            let msg = c_listener.recv().await.unwrap();
            assert_eq!(msg, C { c_value: 10 });

            event_sys
                .emit_by_topic(&Topic::TopicD, &C { c_value: 10 })
                .await;
            let msg = d_listener.recv().await.unwrap();
            assert_eq!(msg, C { c_value: 10 });

            // register different event types to the same topic
            let e_listener = event_sys.register::<E>(&Topic::TopicE).await;
            let f_listener = event_sys.register::<F>(&Topic::TopicE).await;

            event_sys
                .emit_by_topic(&Topic::TopicE, &E { e_value: 5 })
                .await;

            let msg = e_listener.recv().await.unwrap();
            assert_eq!(msg, E { e_value: 5 });

            event_sys
                .emit_by_topic(&Topic::TopicE, &F { f_value: 5 })
                .await;

            let msg = f_listener.recv().await.unwrap();
            assert_eq!(msg, F { f_value: 5 });
        });
    }
}
