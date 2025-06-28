pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),

    EventEmitter(String),

    ChannelSend(String),
    ChannelRecv(async_channel::RecvError),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::IO(io) => write!(f, "IO error: {io}"),
            Self::EventEmitter(err) => write!(f, "Event emitter error: {err}"),
            Self::ChannelSend(err) => write!(f, "Async channel send error: {err}"),
            Self::ChannelRecv(err) => write!(f, "Async channel recv error: {err}"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::IO(error)
    }
}

impl From<async_channel::RecvError> for Error {
    fn from(error: async_channel::RecvError) -> Self {
        Error::ChannelRecv(error)
    }
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(error: async_channel::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}
