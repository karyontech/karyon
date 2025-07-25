#![doc = include_str!("../README.md")]

pub mod error;
pub mod eventemitter;

pub use eventemitter::{
    AsEventValue, Event, EventEmitter, EventListener, EventListenerID, EventTopic,
};

pub use error::{Error, Result};

#[cfg(feature = "derive")]
pub use karyon_eventemitter_macro::EventValue;
