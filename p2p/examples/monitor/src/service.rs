use std::{collections::HashMap, sync::Arc};

use futures::stream::{FuturesUnordered, StreamExt};
use log::{debug, error};
use ringbuffer::{AllocRingBuffer, RingBuffer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use smol::{lock::Mutex, Executor};

use karyon_core::async_util::{TaskGroup, TaskResult};
use karyon_jsonrpc::{
    message::SubscriptionID, rpc_impl, rpc_pubsub_impl, Channel, RPCResult, Subscription,
};
use karyon_p2p::{monitor::MonitorTopic, ArcBackend, Result};

const EVENT_BUFFER_SIZE: usize = 60;

pub struct MonitorRPC {
    backend: ArcBackend,
    subs: Arc<Subscriptions>,
    task_group: TaskGroup,
}

impl MonitorRPC {
    pub fn new(backend: ArcBackend, ex: Arc<Executor<'static>>) -> Arc<Self> {
        Arc::new(MonitorRPC {
            backend,
            task_group: TaskGroup::with_executor(ex.clone().into()),
            subs: Subscriptions::new(ex),
        })
    }

    pub async fn run(self: &Arc<Self>) -> Result<()> {
        let conn_events = self.backend.monitor().conn_events().await;
        let peer_pool_events = self.backend.monitor().peer_pool_events().await;
        let discovery_events = self.backend.monitor().discovery_events().await;

        let on_failuer = |res: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = res {
                error!("Event receive loop: {err}")
            }
        };

        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    let event = conn_events.recv().await?;
                    selfc.subs.notify(MonitorTopic::Conn, event).await;
                }
            },
            on_failuer,
        );

        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    let event = peer_pool_events.recv().await?;
                    selfc.subs.notify(MonitorTopic::PeerPool, event).await;
                }
            },
            on_failuer,
        );

        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    let event = discovery_events.recv().await?;
                    selfc.subs.notify(MonitorTopic::Discovery, event).await;
                }
            },
            on_failuer,
        );

        Ok(())
    }

    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
        self.subs.stop().await;
    }
}

#[rpc_impl]
impl MonitorRPC {
    pub async fn ping(&self, _params: Value) -> RPCResult<Value> {
        Ok(serde_json::json!(Pong {}))
    }

    pub async fn peer_id(&self, _params: Value) -> RPCResult<Value> {
        Ok(serde_json::json!(self.backend.peer_id().to_string()))
    }

    pub async fn inbound_connection(&self, _params: Value) -> RPCResult<Value> {
        Ok(serde_json::json!(self.backend.inbound_slots()))
    }

    pub async fn outbound_connection(&self, _params: Value) -> RPCResult<Value> {
        Ok(serde_json::json!(self.backend.outbound_slots()))
    }
}

#[rpc_pubsub_impl]
impl MonitorRPC {
    pub async fn conn_subscribe(
        &self,
        chan: Arc<Channel>,
        method: String,
        _params: Value,
    ) -> RPCResult<Value> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id;

        self.subs.add(MonitorTopic::Conn, sub).await;

        Ok(serde_json::json!(sub_id))
    }

    pub async fn peer_pool_subscribe(
        &self,
        chan: Arc<Channel>,
        method: String,
        _params: Value,
    ) -> RPCResult<Value> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id;

        self.subs.add(MonitorTopic::PeerPool, sub).await;

        Ok(serde_json::json!(sub_id))
    }

    pub async fn discovery_subscribe(
        &self,
        chan: Arc<Channel>,
        method: String,
        _params: Value,
    ) -> RPCResult<Value> {
        let sub = chan.new_subscription(&method).await;
        let sub_id = sub.id;

        self.subs.add(MonitorTopic::Discovery, sub).await;

        Ok(serde_json::json!(sub_id))
    }

    pub async fn conn_unsubscribe(
        &self,
        chan: Arc<Channel>,
        _method: String,
        params: Value,
    ) -> RPCResult<Value> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        chan.remove_subscription(&sub_id).await;
        Ok(serde_json::json!(true))
    }

    pub async fn peer_pool_unsubscribe(
        &self,
        chan: Arc<Channel>,
        _method: String,
        params: Value,
    ) -> RPCResult<Value> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        chan.remove_subscription(&sub_id).await;
        Ok(serde_json::json!(true))
    }

    pub async fn discovery_unsubscribe(
        &self,
        chan: Arc<Channel>,
        _method: String,
        params: Value,
    ) -> RPCResult<Value> {
        let sub_id: SubscriptionID = serde_json::from_value(params)?;
        chan.remove_subscription(&sub_id).await;
        Ok(serde_json::json!(true))
    }
}

#[derive(Deserialize, Serialize)]
struct Pong {}

struct Subscriptions {
    subs: Mutex<HashMap<MonitorTopic, HashMap<SubscriptionID, Subscription>>>,
    buffer: Mutex<HashMap<MonitorTopic, AllocRingBuffer<Value>>>,
    task_group: TaskGroup,
}

impl Subscriptions {
    fn new(ex: Arc<Executor<'static>>) -> Arc<Self> {
        let mut subs = HashMap::new();
        subs.insert(MonitorTopic::Conn, HashMap::new());
        subs.insert(MonitorTopic::PeerPool, HashMap::new());
        subs.insert(MonitorTopic::Discovery, HashMap::new());

        let mut buffer = HashMap::new();
        buffer.insert(MonitorTopic::Conn, AllocRingBuffer::new(EVENT_BUFFER_SIZE));
        buffer.insert(
            MonitorTopic::PeerPool,
            AllocRingBuffer::new(EVENT_BUFFER_SIZE),
        );
        buffer.insert(
            MonitorTopic::Discovery,
            AllocRingBuffer::new(EVENT_BUFFER_SIZE),
        );

        Arc::new(Self {
            subs: Mutex::new(subs),
            buffer: Mutex::new(buffer),
            task_group: TaskGroup::with_executor(ex.into()),
        })
    }

    /// Adds the subscription to the subs map according to the given type
    async fn add(self: &Arc<Self>, ty: MonitorTopic, sub: Subscription) {
        match self.subs.lock().await.get_mut(&ty) {
            Some(subs) => {
                subs.insert(sub.id, sub.clone());
            }
            None => todo!(),
        }
        // Send old events in the buffer to the subscriber
        self.send_old_events(ty, sub).await;
    }

    /// Notifies all subscribers   
    async fn notify<T: Serialize>(&self, ty: MonitorTopic, event: T) {
        let event = serde_json::json!(event);
        // Add the new event to the ringbuffer
        match self.buffer.lock().await.get_mut(&ty) {
            Some(events) => events.push(event.clone()),
            None => todo!(),
        }

        // Notify the subscribers
        match self.subs.lock().await.get_mut(&ty) {
            Some(subs) => {
                let mut fulist = FuturesUnordered::new();

                for sub in subs.values() {
                    let fu = async { (sub.id, sub.notify(event.clone()).await) };
                    fulist.push(fu)
                }

                let mut cleanup_list = vec![];
                while let Some((sub_id, result)) = fulist.next().await {
                    if let Err(err) = result {
                        error!("Failed to notify the subscription: {:?} {sub_id} {err}", ty);
                        cleanup_list.push(sub_id);
                    }
                }
                drop(fulist);

                for sub_id in cleanup_list {
                    subs.remove(&sub_id);
                }
            }
            None => todo!(),
        }
    }

    /// Sends old events in the ringbuffer to the new subscriber.
    async fn send_old_events(self: &Arc<Self>, ty: MonitorTopic, sub: Subscription) {
        let ty_cloned = ty.clone();
        let sub_id = sub.id;
        let on_complete = move |res: TaskResult<()>| async move {
            debug!("Send old events: {:?} {:?} {res}", ty_cloned, sub_id);
        };

        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                match selfc.buffer.lock().await.get_mut(&ty) {
                    Some(events) => {
                        let mut fu = FuturesUnordered::new();

                        for event in events.iter().rev() {
                            fu.push(sub.notify(event.clone()))
                        }

                        while let Some(result) = fu.next().await {
                            if result.is_err() {
                                return;
                            }
                        }
                    }
                    None => todo!(),
                }
            },
            on_complete,
        );
    }

    async fn stop(&self) {
        self.task_group.cancel().await;
    }
}
