use std::env;

use futures::StreamExt;
use std::str::FromStr;

use karyon_eventemitter::{EventEmitter, EventListener, EventValue};

// This example uses the OpenAI responses stream API
// (https://platform.openai.com/docs/api-reference/responses-streaming).
// It first registers the event type with the event emitter, and once it receives a new message
// from the API, it parses the message and emits it to the listener.

const OPENAI_URL: &str = "https://api.openai.com/v1/responses";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("OPENAI_API_KEY")?;

    let event_emitter = EventEmitter::new();

    let response_output_text_delta_listener =
        event_emitter.register::<EventData>(&StreamEventType::ResponseOutputTextDelta);

    let _error_listener = event_emitter.register::<EventData>(&StreamEventType::Error);

    let _unknown_listener = event_emitter.register::<EventData>(&StreamEventType::Unknown);

    tokio::spawn(handle_output_text_delta(
        response_output_text_delta_listener,
    ));

    let client = reqwest::Client::new();

    let body_str = r#"
            {
                "model": "gpt-4.1",
                "input": "Tell me a bedtime story about a unicorn.",
                "stream": true
            }
        "#;

    let res = client
        .post(OPENAI_URL)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .body(body_str)
        .send()
        .await?;

    let mut stream = res.bytes_stream();

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                if let Some(ev) = parse_event(&text) {
                    println!("RECEIVE NEW EV {:?}", ev.event_type);
                    let topic = StreamEventType::from_str(&ev.event_type).unwrap();
                    event_emitter
                        .emit_by_topic(&topic, &ev.data)
                        .await
                        .expect("Emit event");
                }
            }
            Err(e) => {
                println!("Stream error: {:?}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn handle_output_text_delta(listener: EventListener<StreamEventType, EventData>) {
    while let Ok(msg) = listener.recv().await {
        println!("RECEIVE NEW OUTPUT TEXT DELTA {:?}", msg.0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamEventType {
    ResponseStart,
    ResponseOutputTextDelta,
    Error,
    Unknown,
}

impl FromStr for StreamEventType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "response.output_text.delta" => Ok(StreamEventType::ResponseOutputTextDelta),
            "error" => Ok(StreamEventType::Error),
            _ => Ok(StreamEventType::Unknown),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventData(serde_json::Value);

impl EventValue for EventData {
    fn event_id() -> &'static str {
        "event_data"
    }
}

#[derive(Debug)]
struct Event {
    event_type: String,
    data: EventData,
}

fn parse_event(message: &str) -> Option<Event> {
    let mut event_type = None;
    let mut data_json = None;

    for line in message.lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            event_type = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data: ") {
            data_json = Some(rest.trim());
        }
    }

    if let (Some(event_type), Some(data_str)) = (event_type, data_json) {
        match serde_json::from_str::<serde_json::Value>(data_str) {
            Ok(data) => Some(Event {
                event_type,
                data: EventData(data),
            }),
            Err(err) => {
                println!("Failed to parse JSON data: {err}");
                None
            }
        }
    } else {
        None
    }
}
