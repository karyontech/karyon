use std::sync::{Arc, Mutex};

use tokio::time::{timeout, Duration};

use karyon_eventemitter::{AsEventTopic, AsEventValue, EventEmitter, EventValue};

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
enum Topic {
    TopicA,
    TopicB,
    TopicC,
    TopicD,
    TopicE,
    TopicF,
    TopicG,
    TopicH,
    SpecialTopic,
    BenchmarkTopic,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
struct AEvent {
    value: usize,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
struct BEvent {
    value: usize,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
struct CEvent {
    value: usize,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
struct EEvent {
    value: usize,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
struct FEvent {
    value: usize,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
#[event_id("custom_g_event")]
struct GEvent {
    data: String,
    timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
#[event_id("special.event.type")]
struct SpecialEvent {
    priority: u8,
    message: String,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
#[event_id("benchmark_event")]
struct BenchmarkEvent {
    id: u64,
    payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
enum StatusEvent {
    Started { id: u32 },
    Stopped,
}

#[derive(Clone, Debug, PartialEq, EventValue)]
#[event_id("user_action")]
enum UserAction {
    Login { user_id: String },
}

// Complex nested structures
#[derive(Clone, Debug, PartialEq, EventValue)]
struct ComplexEvent {
    header: EventHeader,
    payload: EventPayload,
}

#[derive(Clone, Debug, PartialEq)]
struct EventHeader {
    version: u8,
    source: String,
}

#[derive(Clone, Debug, PartialEq)]
enum EventPayload {
    Structured { key: String, value: i64 },
}

// Zero-sized type events
#[derive(Clone, Debug, PartialEq, EventValue)]
struct PingEvent;

#[derive(Clone, Debug, PartialEq, EventValue)]
#[event_id("heartbeat")]
struct HeartbeatEvent;

impl AsEventTopic for CEvent {
    type Topic = Topic;
    fn topic() -> Self::Topic {
        Topic::TopicC
    }
}

impl AsEventTopic for SpecialEvent {
    type Topic = Topic;
    fn topic() -> Self::Topic {
        Topic::SpecialTopic
    }
}

#[tokio::test]
async fn test_basic_event_emitter() {
    let event_emitter = EventEmitter::<Topic>::new();
    let a_listener = event_emitter.register::<AEvent>(&Topic::TopicA);
    let b_listener = event_emitter.register::<BEvent>(&Topic::TopicB);

    event_emitter
        .emit_by_topic(&Topic::TopicA, &AEvent { value: 3 })
        .await
        .expect("Emit event");
    event_emitter
        .emit_by_topic(&Topic::TopicB, &BEvent { value: 5 })
        .await
        .expect("Emit event");

    let msg = a_listener.recv().await.unwrap();
    assert_eq!(msg, AEvent { value: 3 });
    let msg = b_listener.recv().await.unwrap();
    assert_eq!(msg, BEvent { value: 5 });
}

#[tokio::test]
async fn test_custom_event_ids() {
    let event_emitter = EventEmitter::<Topic>::new();
    let g_listener = event_emitter.register::<GEvent>(&Topic::TopicG);
    let special_listener = event_emitter.register::<SpecialEvent>(&Topic::SpecialTopic);

    event_emitter
        .emit_by_topic(
            &Topic::TopicG,
            &GEvent {
                data: "test data".to_string(),
                timestamp: 1234567890,
            },
        )
        .await
        .expect("Emit GEvent");

    event_emitter
        .emit(&SpecialEvent {
            priority: 1,
            message: "Special message".to_string(),
        })
        .await
        .expect("Emit SpecialEvent");

    let g_msg = g_listener.recv().await.unwrap();
    assert_eq!(g_msg.data, "test data");
    assert_eq!(g_msg.timestamp, 1234567890);

    let special_msg = special_listener.recv().await.unwrap();
    assert_eq!(special_msg.priority, 1);
    assert_eq!(special_msg.message, "Special message");
}

#[tokio::test]
async fn test_enum_events() {
    let event_emitter = EventEmitter::<Topic>::new();
    let status_listener = event_emitter.register::<StatusEvent>(&Topic::TopicF);
    let user_listener = event_emitter.register::<UserAction>(&Topic::TopicG);

    event_emitter
        .emit_by_topic(&Topic::TopicF, &StatusEvent::Started { id: 42 })
        .await
        .expect("Emit StatusEvent");

    event_emitter
        .emit_by_topic(&Topic::TopicF, &StatusEvent::Stopped)
        .await
        .expect("Emit StatusEvent");

    event_emitter
        .emit_by_topic(
            &Topic::TopicG,
            &UserAction::Login {
                user_id: "user123".to_string(),
            },
        )
        .await
        .expect("Emit UserAction");

    let status_msg = status_listener.recv().await.unwrap();
    assert!(matches!(status_msg, StatusEvent::Started { id: 42 }));

    let status_msg = status_listener.recv().await.unwrap();
    assert!(matches!(status_msg, StatusEvent::Stopped));

    let user_msg = user_listener.recv().await.unwrap();
    assert!(matches!(user_msg, UserAction::Login { user_id } if user_id == "user123"));
}

#[tokio::test]
async fn test_zero_sized_events() {
    let event_emitter = EventEmitter::<Topic>::new();
    let ping_listener = event_emitter.register::<PingEvent>(&Topic::TopicA);
    let heartbeat_listener = event_emitter.register::<HeartbeatEvent>(&Topic::TopicB);

    event_emitter
        .emit_by_topic(&Topic::TopicA, &PingEvent)
        .await
        .expect("Emit PingEvent");

    event_emitter
        .emit_by_topic(&Topic::TopicB, &HeartbeatEvent)
        .await
        .expect("Emit HeartbeatEvent");

    let ping_msg = ping_listener.recv().await.unwrap();
    assert_eq!(ping_msg, PingEvent);

    let heartbeat_msg = heartbeat_listener.recv().await.unwrap();
    assert_eq!(heartbeat_msg, HeartbeatEvent);
}

#[tokio::test]
async fn test_complex_nested_events() {
    let event_emitter = EventEmitter::<Topic>::new();
    let complex_listener = event_emitter.register::<ComplexEvent>(&Topic::TopicH);

    let event = ComplexEvent {
        header: EventHeader {
            version: 1,
            source: "test_system".to_string(),
        },
        payload: EventPayload::Structured {
            key: "temperature".to_string(),
            value: 42,
        },
    };

    event_emitter
        .emit_by_topic(&Topic::TopicH, &event)
        .await
        .expect("Emit ComplexEvent");

    let received = complex_listener.recv().await.unwrap();
    assert_eq!(received.header.version, 1);
    assert_eq!(received.header.source, "test_system");

    let EventPayload::Structured { key, value } = received.payload;
    assert_eq!(key, "temperature");
    assert_eq!(value, 42);
}

#[tokio::test]
async fn test_multiple_listeners_same_event() {
    let event_emitter = EventEmitter::<Topic>::new();

    // Register multiple listeners for the same event type on the same topic
    let listener1 = event_emitter.register::<AEvent>(&Topic::TopicA);
    let listener2 = event_emitter.register::<AEvent>(&Topic::TopicA);
    let listener3 = event_emitter.register::<AEvent>(&Topic::TopicA);

    let test_event = AEvent { value: 999 };

    event_emitter
        .emit_by_topic(&Topic::TopicA, &test_event)
        .await
        .expect("Emit to multiple listeners");

    // All listeners should receive the same event
    let msg1 = listener1.recv().await.unwrap();
    let msg2 = listener2.recv().await.unwrap();
    let msg3 = listener3.recv().await.unwrap();

    assert_eq!(msg1, test_event);
    assert_eq!(msg2, test_event);
    assert_eq!(msg3, test_event);
}

#[tokio::test]
async fn test_cross_topic_event() {
    let event_emitter = EventEmitter::<Topic>::new();

    let listener_a = event_emitter.register::<AEvent>(&Topic::TopicA);
    let listener_b = event_emitter.register::<AEvent>(&Topic::TopicB);

    // Emit to TopicA only
    event_emitter
        .emit_by_topic(&Topic::TopicA, &AEvent { value: 100 })
        .await
        .expect("Emit to TopicA");

    // Only TopicA listener should receive the event
    let msg = listener_a.recv().await.unwrap();
    assert_eq!(msg, AEvent { value: 100 });

    // TopicB listener should not receive anything
    let result = timeout(Duration::from_millis(100), listener_b.recv()).await;
    assert!(
        result.is_err(),
        "TopicB listener should not receive the event"
    );
}

#[tokio::test]
async fn test_same_event_different_topics() {
    let event_emitter = EventEmitter::<Topic>::new();

    // Test same event type on different topics
    let c_listener = event_emitter.register::<CEvent>(&Topic::TopicC);
    let d_listener = event_emitter.register::<CEvent>(&Topic::TopicD);

    event_emitter
        .emit(&CEvent { value: 10 })
        .await
        .expect("Emit event");

    let msg = c_listener.recv().await.unwrap();
    assert_eq!(msg, CEvent { value: 10 });

    event_emitter
        .emit_by_topic(&Topic::TopicD, &CEvent { value: 20 })
        .await
        .expect("Emit event");

    let msg = d_listener.recv().await.unwrap();
    assert_eq!(msg, CEvent { value: 20 });
}

#[tokio::test]
async fn test_different_events_same_topic() {
    let event_emitter = EventEmitter::<Topic>::new();

    // Test different event types on the same topic
    let e_listener = event_emitter.register::<EEvent>(&Topic::TopicE);
    let f_listener = event_emitter.register::<FEvent>(&Topic::TopicE);

    event_emitter
        .emit_by_topic(&Topic::TopicE, &EEvent { value: 5 })
        .await
        .expect("Emit event");

    let msg = e_listener.recv().await.unwrap();
    assert_eq!(msg, EEvent { value: 5 });

    event_emitter
        .emit_by_topic(&Topic::TopicE, &FEvent { value: 15 })
        .await
        .expect("Emit event");

    let msg = f_listener.recv().await.unwrap();
    assert_eq!(msg, FEvent { value: 15 });
}

#[tokio::test]
async fn test_high_volume_events() {
    let event_emitter = EventEmitter::<Topic>::with_buffer_size(10000);
    let benchmark_listener = event_emitter.register::<BenchmarkEvent>(&Topic::BenchmarkTopic);

    const EVENT_COUNT: u64 = 10000;
    let payload = vec![0u8; 1024];

    for i in 0..EVENT_COUNT {
        let event = BenchmarkEvent {
            id: i,
            payload: payload.clone(),
        };

        event_emitter
            .emit_by_topic(&Topic::BenchmarkTopic, &event)
            .await
            .expect(&format!("Emit event {}", i));
    }

    // Verify all events are received in order
    for expected_id in 0..EVENT_COUNT {
        let msg = benchmark_listener.recv().await.unwrap();
        assert_eq!(msg.id, expected_id);
        assert_eq!(msg.payload.len(), 1024);
    }
}

#[tokio::test]
async fn test_concurrent_emitters() {
    let event_emitter = EventEmitter::<Topic>::new();
    let listener = event_emitter.register::<AEvent>(&Topic::TopicA);

    const EMITTERS: usize = 100;
    const EVENTS_PER_EMITTER: usize = 1000;

    let mut handles = Vec::new();

    let received_values = Arc::new(Mutex::new(Vec::new()));
    let received_values_cloned = received_values.clone();
    tokio::spawn(async move {
        // Collect all received events
        for _ in 0..(EMITTERS * EVENTS_PER_EMITTER) {
            let msg = listener.recv().await.unwrap();
            received_values_cloned.lock().unwrap().push(msg.value);
        }
    });

    // Spawn multiple concurrent emitters
    for emitter_id in 0..EMITTERS {
        let emitter = event_emitter.clone();
        let handle = tokio::spawn(async move {
            for event_id in 0..EVENTS_PER_EMITTER {
                let value = emitter_id * EVENTS_PER_EMITTER + event_id;
                emitter
                    .emit_by_topic(&Topic::TopicA, &AEvent { value })
                    .await
                    .expect("Emit concurrent event");
            }
        });
        handles.push(handle);
    }

    // Wait for all emitters to complete
    for handle in handles {
        handle.await.expect("Emitter task completed");
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Verify we received all events
    received_values.lock().unwrap().sort();
    let expected: Vec<usize> = (0..(EMITTERS * EVENTS_PER_EMITTER)).collect();
    assert_eq!(received_values.lock().unwrap().to_vec(), expected);
}

#[tokio::test]
async fn test_concurrent_emitters_with_big_buffer_size() {
    let event_emitter = EventEmitter::<Topic>::with_buffer_size(100000);
    let listener = event_emitter.register::<AEvent>(&Topic::TopicA);

    const EMITTERS: usize = 100;
    const EVENTS_PER_EMITTER: usize = 1000;

    let mut handles = Vec::new();

    // Spawn multiple concurrent emitters
    for emitter_id in 0..EMITTERS {
        let emitter = event_emitter.clone();
        let handle = tokio::spawn(async move {
            for event_id in 0..EVENTS_PER_EMITTER {
                let value = emitter_id * EVENTS_PER_EMITTER + event_id;
                emitter
                    .emit_by_topic(&Topic::TopicA, &AEvent { value })
                    .await
                    .expect("Emit concurrent event");
            }
        });
        handles.push(handle);
    }

    // Wait for all emitters to complete
    for handle in handles {
        handle.await.expect("Emitter task completed");
    }

    // Collect all received events
    let mut received_values = Vec::new();
    for _ in 0..(EMITTERS * EVENTS_PER_EMITTER) {
        let msg = listener.recv().await.unwrap();
        received_values.push(msg.value);
    }

    // Verify we received all events
    received_values.sort();
    let expected: Vec<usize> = (0..(EMITTERS * EVENTS_PER_EMITTER)).collect();
    assert_eq!(received_values, expected);
}

#[tokio::test]
async fn test_event_id_uniqueness() {
    let ids = vec![
        AEvent::event_id(),
        BEvent::event_id(),
        CEvent::event_id(),
        EEvent::event_id(),
        FEvent::event_id(),
        GEvent::event_id(),
        SpecialEvent::event_id(),
        BenchmarkEvent::event_id(),
        StatusEvent::event_id(),
        UserAction::event_id(),
        ComplexEvent::event_id(),
        PingEvent::event_id(),
        HeartbeatEvent::event_id(),
    ];

    let mut unique_ids = ids.clone();
    unique_ids.sort();
    unique_ids.dedup();

    assert_eq!(
        ids.len(),
        unique_ids.len(),
        "All event IDs should be unique"
    );

    // Custom event IDs
    assert_eq!(GEvent::event_id(), "custom_g_event");
    assert_eq!(SpecialEvent::event_id(), "special.event.type");
    assert_eq!(BenchmarkEvent::event_id(), "benchmark_event");
    assert_eq!(UserAction::event_id(), "user_action");
    assert_eq!(HeartbeatEvent::event_id(), "heartbeat");

    // Default event IDs
    assert_eq!(AEvent::event_id(), "AEvent");
    assert_eq!(StatusEvent::event_id(), "StatusEvent");
    assert_eq!(PingEvent::event_id(), "PingEvent");
}
