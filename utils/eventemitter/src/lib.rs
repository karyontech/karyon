pub mod error;
pub mod eventemitter;

pub use eventemitter::{
    AsEventTopic, AsEventValue, Event, EventEmitter, EventListener, EventListenerID,
};

pub use error::{Error, Result};

#[cfg(feature = "derive")]
pub use karyon_eventemitter_macro::EventValue;
