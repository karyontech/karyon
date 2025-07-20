use std::{
    any::Any,
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Weak},
};

use async_channel::{Receiver, Sender};
use chrono::{DateTime, Utc};
use futures_util::stream::{FuturesUnordered, StreamExt};
use log::trace;
use parking_lot::Mutex;
use rand::{rngs::OsRng, Rng};

use crate::error::{Error, Result};

/// Default buffer size for an event listener channel
const DEFAULT_CHANNEL_BUFFER_SIZE: usize = 1000;

/// Unique identifier for event listeners
pub type EventListenerID = u64;

/// Internal type alias for the nested HashMap structure that stores listeners
type Listeners<T> = HashMap<T, HashMap<String, HashMap<EventListenerID, Sender<Event>>>>;

fn random_id() -> EventListenerID {
    OsRng.gen()
}

/// EventEmitter asynchronous event emitter.
///
/// Allows components to communicate through events organized by topics.
///
/// # Example
///
/// ```
/// use karyon_eventemitter::{EventEmitter, AsEventTopic, EventValue};
///
///  async {
///     let event_emitter = EventEmitter::new();
///
///     #[derive(Hash, PartialEq, Eq, Debug, Clone)]
///     enum Topic {
///         TopicA,
///         TopicB,
///     }
///
///     #[derive(Clone, Debug, PartialEq, EventValue)]
///     struct A(usize);
///
///     #[derive(Clone, Debug, PartialEq, EventValue)]
///     struct B(usize);
///
///     impl AsEventTopic for B {
///         type Topic = Topic;
///         fn topic() -> Self::Topic{
///             Topic::TopicB
///         }
///     }
///
///     #[derive(Clone, Debug, PartialEq, EventValue)]
///     struct C(usize);
///
///     let a_listener = event_emitter.register::<A>(&Topic::TopicA);
///     let b_listener = event_emitter.register::<B>(&Topic::TopicB);
///     // This also listens to Topic B
///     let c_listener = event_emitter.register::<C>(&Topic::TopicB);
///
///     event_emitter.emit_by_topic(&Topic::TopicA, &A(3)) .await;
///     event_emitter.emit(&B(3)) .await;
///     event_emitter.emit_by_topic(&Topic::TopicB, &C(3)) .await;
///
///     let msg: A = a_listener.recv().await.unwrap();
///     let msg: B = b_listener.recv().await.unwrap();
///     let msg: C = c_listener.recv().await.unwrap();
///
///     // ....
///  };
///
/// ```
///
pub struct EventEmitter<T> {
    listeners: Mutex<Listeners<T>>,
    listener_buffer_size: usize,
}

impl<T> EventEmitter<T>
where
    T: std::hash::Hash + Eq + std::fmt::Debug + Clone,
{
    /// Creates a new [`EventEmitter`]
    pub fn new() -> Arc<EventEmitter<T>> {
        Arc::new(Self {
            listeners: Mutex::new(HashMap::new()),
            listener_buffer_size: DEFAULT_CHANNEL_BUFFER_SIZE,
        })
    }

    /// Creates a new [`EventEmitter`] with the provided buffer size for the
    /// [`EventListener`] channel.
    ///
    /// This is important to control the memory used by the listener channel.
    /// If the consumer for the event listener can't keep up with the new events
    /// coming, then the channel buffer will fill with new events, and if the
    /// buffer is full, the emit function will block until the listener
    /// starts to consume the buffered events.
    ///
    /// If `size` is zero, this function will panic.
    pub fn with_buffer_size(size: usize) -> Arc<EventEmitter<T>> {
        assert_ne!(size, 0);
        Arc::new(Self {
            listeners: Mutex::new(HashMap::new()),
            listener_buffer_size: size,
        })
    }

    /// Emits an event to the listeners.
    ///
    /// The event must implement the [`AsEventTopic`] trait to indicate the
    /// topic of the event. Otherwise, you can use `emit_by_topic()`.
    pub async fn emit<E: AsEventTopic<Topic = T> + Clone>(&self, value: &E) -> Result<()> {
        let topic = E::topic();
        self.emit_by_topic(&topic, value).await
    }

    /// Emits an event to the listeners.
    pub async fn emit_by_topic<E: AsEventValue + Clone>(&self, topic: &T, value: &E) -> Result<()> {
        let mut results = self.send(topic, value).await?;

        let mut failed_listeners = vec![];
        while let Some((id, fut_err)) = results.next().await {
            if let Err(err) = fut_err {
                trace!("Failed to emit event for topic {topic:?}: {err}");
                failed_listeners.push(id);
            }
        }
        drop(results);

        if !failed_listeners.is_empty() {
            self.remove(topic, E::event_id(), &failed_listeners);
        }

        Ok(())
    }

    /// Registers a new event listener for the given topic.
    pub fn register<E: AsEventValue + Clone>(self: &Arc<Self>, topic: &T) -> EventListener<T, E> {
        let topics = &mut self.listeners.lock();

        let chan = async_channel::bounded(self.listener_buffer_size);

        let event_ids = topics.entry(topic.clone()).or_default();
        let event_id = E::event_id().to_string();

        let listeners = event_ids.entry(event_id.clone()).or_default();

        let mut listener_id = random_id();
        // Generate a new one if listener_id already exists
        while listeners.contains_key(&listener_id) {
            listener_id = random_id();
        }

        let listener =
            EventListener::new(listener_id, Arc::downgrade(self), chan.1, &event_id, topic);

        listeners.insert(listener_id, chan.0);

        listener
    }

    /// Removes all topics and event listeners.
    ///
    /// This effectively resets the EventEmitter to its initial state.
    pub fn clear(self: &Arc<Self>) {
        self.listeners.lock().clear();
    }

    /// Unregisters all event listeners for the given topic.
    pub fn unregister_topic(self: &Arc<Self>, topic: &T) {
        self.listeners.lock().remove(topic);
    }

    /// Internal method that handles the actual sending of events to listeners.
    async fn send<E: AsEventValue + Clone>(
        &self,
        topic: &T,
        value: &E,
    ) -> Result<
        FuturesUnordered<
            impl std::future::Future<
                    Output = (
                        EventListenerID,
                        std::result::Result<(), async_channel::SendError<Event>>,
                    ),
                > + use<'_, T, E>,
        >,
    > {
        let value: Arc<dyn AsEventValue> = Arc::new(value.clone());
        let event = Event::new(value);

        let mut topics = self.listeners.lock();

        let results = FuturesUnordered::new();
        let event_ids = match topics.get_mut(topic) {
            Some(ids) => ids,
            None => {
                trace!("Failed to emit an event to a non-existent topic {topic:?}",);
                return Err(Error::EventEmitter(format!(
                    "Emit an event to a non-existent topic {topic:?}",
                )));
            }
        };

        let event_id = E::event_id().to_string();

        let Some(listeners) = event_ids.get_mut(&event_id) else {
            trace!("No listeners found for event '{event_id}' on topic {topic:?}",);
            return Ok(results);
        };

        for (listener_id, listener) in listeners {
            let event = event.clone();
            let listener = listener.clone();
            let id = *listener_id;
            let result = async move { (id, listener.send(event.clone()).await) };
            results.push(result);
        }
        drop(topics);

        Ok(results)
    }

    /// Internal method to remove the event listener attached to the given topic.
    fn remove(&self, topic: &T, event_id: &str, listener_ids: &[EventListenerID]) {
        let topics = &mut self.listeners.lock();

        let Some(event_ids) = topics.get_mut(topic) else {
            trace!("Failed to remove a non-existent topic");
            return;
        };

        let Some(listeners) = event_ids.get_mut(event_id) else {
            trace!("Failed to remove a non-existent event id");
            return;
        };

        for listener_id in listener_ids {
            if listeners.remove(listener_id).is_none() {
                trace!("Failed to remove a non-existent event listener");
            }
        }
    }
}

/// EventListener listens for and receives events from the [`EventEmitter`].
pub struct EventListener<T, E> {
    id: EventListenerID,
    recv_chan: Receiver<Event>,
    event_emitter: Weak<EventEmitter<T>>,
    event_id: String,
    topic: T,
    phantom: PhantomData<E>,
}

impl<T, E> EventListener<T, E>
where
    T: std::hash::Hash + Eq + Clone + std::fmt::Debug,
    E: AsEventValue + Clone,
{
    /// Creates a new [`EventListener`].
    fn new(
        id: EventListenerID,
        event_emitter: Weak<EventEmitter<T>>,
        recv_chan: Receiver<Event>,
        event_id: &str,
        topic: &T,
    ) -> EventListener<T, E> {
        Self {
            id,
            recv_chan,
            event_emitter,
            event_id: event_id.to_string(),
            topic: topic.clone(),
            phantom: PhantomData,
        }
    }

    /// Receives the next event from the emitter.
    ///
    /// This method blocks until an event is available or the channel is closed.
    /// Events are automatically type-cast to the expected type E.
    pub async fn recv(&self) -> Result<E> {
        match self.recv_chan.recv().await {
            Ok(event) => match (event.value as Arc<dyn Any>).downcast_ref::<E>() {
                Some(v) => Ok(v.clone()),
                None => unreachable!("Failed to downcast the event value."),
            },
            Err(err) => {
                trace!("Failed to receive new event: {err}");
                self.cancel();
                Err(err.into())
            }
        }
    }

    /// Cancels the event listener and removes it from the [`EventEmitter`].
    pub fn cancel(&self) {
        self.event_emitter()
            .remove(&self.topic, &self.event_id, &[self.id]);
    }

    /// Returns the topic this listener is registered for.
    pub fn topic(&self) -> &T {
        &self.topic
    }

    /// Returns the event id for this event listener.
    pub fn event_id(&self) -> &String {
        &self.event_id
    }

    /// Returns a reference to the parent EventEmitter.
    pub fn event_emitter(&self) -> Arc<EventEmitter<T>> {
        self.event_emitter.upgrade().unwrap()
    }
}

/// A timestamped event container.
#[derive(Clone, Debug)]
pub struct Event {
    /// The time at which the event was created.
    created_at: DateTime<Utc>,
    /// The value of the Event.
    value: Arc<dyn AsEventValue>,
}

impl Event {
    /// Creates a new Event.
    pub fn new(value: Arc<dyn AsEventValue>) -> Self {
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

/// Trait for event types that can be emitted.
///
/// This trait provides a string identifier for each event type,
/// used internally for routing and type checking.
pub trait AsEventValue: Any + Send + Sync + std::fmt::Debug {
    fn event_id() -> &'static str
    where
        Self: Sized;
}

/// Trait for events that define their own topic.
///
/// This trait allows events to specify which topic they belong to,
/// enabling the use of the convenient `emit()` method instead of
/// requiring explicit topic specification with `emit_by_topic()`.
pub trait AsEventTopic: AsEventValue {
    type Topic;
    fn topic() -> Self::Topic
    where
        Self: Sized;
}
