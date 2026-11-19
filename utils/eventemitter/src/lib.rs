#![doc = include_str!("../README.md")]

pub mod error;
pub mod eventemitter;

pub use eventemitter::{
    AsEventValue, AsEventValueRef, Event, EventEmitter, EventEmitterRef, EventListener,
    EventListenerID, EventTopic,
};

pub use error::{Error, Result};

#[cfg(feature = "derive")]
pub use karyon_eventemitter_macro::EventValue;
