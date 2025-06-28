pub mod error;
pub mod eventemitter;

pub use eventemitter::{
    Event, EventEmitter, EventListener, EventListenerID, EventValue, EventValueAny, EventValueTopic,
};

pub use error::{Error, Result};
