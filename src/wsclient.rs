pub use walletconnect_sdk::rpc::domain::MessageId;
use {
    crate::{auth::jwt_token, error::Result, types::Envelope},
    futures::{future, StreamExt},
    rand::Rng,
    std::{
        collections::{HashMap, HashSet},
        sync::{Arc, RwLock},
        time::Duration,
    },
    tokio::{select, sync::mpsc},
    tokio_stream::wrappers::ReceiverStream,
    tracing::{info, warn},
    tungstenite::Message,
    walletconnect_sdk::rpc::{
        auth::ed25519_dalek::Keypair,
        domain::{SubscriptionId, Topic},
        rpc::{
            BatchSubscribe,
            Params,
            Payload,
            Publish,
            Request,
            Subscribe,
            SuccessfulResponse,
            Unsubscribe,
        },
    },
};

#[derive(Debug)]
pub struct WsClient {
    pub project_id: String,
    pub tx: mpsc::Sender<Message>,
    pub rx: mpsc::Receiver<Result<Message>>,
    pub ack_broadcast: tokio::sync::broadcast::Sender<MessageId>,
    _ack_receiver: tokio::sync::broadcast::Receiver<MessageId>,
    pub handler: tokio::task::JoinHandle<()>,
}

impl WsClient {
    pub fn is_finished(&self) -> bool {
        self.handler.is_finished()
    }

    pub async fn reconnect(&mut self, url: &str, keypair: &Keypair) -> Result<()> {
        let jwt = jwt_token(url, keypair)?;
        connect(url, &self.project_id, jwt).await?;
        Ok(())
    }

    pub async fn send(&mut self, msg: Request) -> Result<()> {
        self.send_raw(Payload::Request(msg)).await
    }

    pub async fn send_ack(&mut self, id: walletconnect_sdk::rpc::domain::MessageId) -> Result<()> {
        self.send_raw(Payload::Response(
            walletconnect_sdk::rpc::rpc::Response::Success(SuccessfulResponse::new(
                id,
                serde_json::Value::Bool(true),
            )),
        ))
        .await
    }

    pub async fn send_plaintext(&mut self, msg: String) -> Result<()> {
        self.tx
            .send(Message::Text(msg))
            .await
            .map_err(|_| crate::error::Error::ChannelClosed)
    }

    pub async fn send_raw(&mut self, msg: Payload) -> Result<()> {
        let msg = serde_json::to_string(&msg)?;
        self.tx
            .send(Message::Text(msg))
            .await
            .map_err(|_| crate::error::Error::ChannelClosed)
    }

    pub async fn recv(&mut self) -> Result<walletconnect_sdk::rpc::rpc::Request> {
        loop {
            match self.rx.recv().await {
                Some(msg) => match msg? {
                    Message::Text(msg) => {
                        match serde_json::from_str::<Payload>(&msg)? {
                            Payload::Request(req) => {
                                info!("Received request from Relay WS: {:?}", req);
                                return Ok(req);
                            }
                            Payload::Response(res) => {
                                info!("Received response from Relay WS: {:?}", res);
                                let id = res.id();
                                // TODO: Remove unwrap)
                                if let Err(e) = self.ack_broadcast.send(id) {
                                    warn!("Failed to broadcast ack: {}", e);
                                }
                            }
                        }
                    }
                    Message::Ping(_) => {
                        info!("Received ping from Relay WS, sending pong");
                        self.pong().await?;
                    }
                    e => {
                        warn!("Received unsupported message from relay: {:?}", e);
                    }
                },
                None => {
                    return Err(crate::error::Error::ChannelClosed);
                }
            }
        }
    }

    async fn pong(&mut self) -> Result<()> {
        let msg = Message::Pong("heartbeat".into());
        self.tx
            .send(msg)
            .await
            .map_err(|_| crate::error::Error::ChannelClosed)
    }

    pub async fn publish(&mut self, topic: &str, payload: &str) -> Result<()> {
        self.publish_with_tag(topic, payload, 1000).await
    }

    pub async fn publish_with_tag(&mut self, topic: &str, payload: &str, tag: u32) -> Result<()> {
        let msg = Payload::Request(new_rpc_request(
            walletconnect_sdk::rpc::rpc::Params::Publish(Publish {
                topic: topic.into(),
                message: payload.into(),
                ttl_secs: 86400,
                tag,
                prompt: true,
            }),
        ));

        self.send_raw(msg).await
    }

    /// Sends batch of messages with provided tag to relay. Returns set of
    /// topics for which relay didn't ack the published message..
    pub async fn batch_publish_with_tag(
        &mut self,
        messages: Vec<(Topic, String)>,
        tag: u32,
    ) -> Result<HashSet<Topic>> {
        if messages.len() == 0 {
            return Ok(HashSet::new());
        }

        let mut unacked: HashSet<MessageId> = HashSet::new();
        let mut mapping: HashMap<MessageId, Topic> = HashMap::new();

        let mut acks = self.ack_broadcast.subscribe();

        for (topic, payload) in messages.into_iter() {
            let request = new_rpc_request(walletconnect_sdk::rpc::rpc::Params::Publish(Publish {
                topic: topic.clone(),
                message: payload.into(),
                ttl_secs: 86400,
                tag,
                prompt: true,
            }));

            mapping.insert(request.id.clone(), topic.clone());
            unacked.insert(request.id);

            self.send_raw(Payload::Request(request)).await?;
        }

        let timeout_duration = Duration::from_secs(30);

        // We don't care about time running out, we just return the unacked
        _ = tokio::time::timeout(timeout_duration, async {
            while let Ok(ack) = acks.recv().await {
                unacked.remove(&ack);
                if unacked.is_empty() {
                    break;
                }
            }
        })
        .await;

        let unacked = unacked
            .into_iter()
            .filter_map(|id| mapping.remove(&id))
            .collect();

        Ok(unacked)
    }

    pub async fn subscribe(&mut self, topic: &str) -> Result<()> {
        let msg = Payload::Request(new_rpc_request(Params::Subscribe(Subscribe {
            topic: topic.into(),
        })));
        self.send_raw(msg).await
    }

    pub async fn batch_subscribe(&mut self, topics: Vec<String>) -> Result<()> {
        let msg = Payload::Request(new_rpc_request(Params::BatchSubscribe(BatchSubscribe {
            topics: topics.into_iter().map(|x| x.into()).collect(),
        })));
        self.send_raw(msg).await
    }

    pub async fn unsubscribe(
        &mut self,
        topic: Topic,
        subscription_id: SubscriptionId,
    ) -> Result<()> {
        let msg = Payload::Request(new_rpc_request(Params::Unsubscribe(Unsubscribe {
            topic,
            subscription_id,
        })));
        self.send_raw(msg).await
    }
}

pub fn new_rpc_request(params: Params) -> Request {
    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

    let id = id * 1000 + rand::thread_rng().gen_range(100, 1000);

    Request {
        id: id.into(),
        jsonrpc: "2.0".into(),
        params,
    }
}

pub async fn connect(url: &str, project_id: &str, jwt: String) -> Result<WsClient> {
    info!("Connecting to Relay WS ({})...", url);
    let relay_query = format!("auth={jwt}&projectId={project_id}");

    let mut url = url::Url::parse(url)?;
    url.set_query(Some(&relay_query));

    let (connection, _) = async_tungstenite::tokio::connect_async(&url).await?;

    // A channel for passing messages to websocket
    let (wr_tx, wr_rx) = mpsc::channel(1024);
    let (ack_tx, ack_rx) = tokio::sync::broadcast::channel(1024);

    // A channel for incoming messages from websocket
    let (rd_tx, rd_rx) = mpsc::channel::<Result<_>>(1024);

    let handler = tokio::spawn(async move {
        let (tx, rx) = connection.split();
        // Forward messages to the write half
        let write = ReceiverStream::new(wr_rx).map(Ok).forward(tx);

        // Process incoming messages and close the
        // client in case of a close frame. Else forward them.
        let read = rx
            .take_while(|result| match result {
                Err(_) => future::ready(false),
                Ok(m) => future::ready(!m.is_close()),
            })
            .for_each_concurrent(None, |result| async {
                if let Err(e) = rd_tx.send(result.map_err(Into::into)).await {
                    warn!("WSClient send error: {}", e);
                };
            });

        // Keep the thread alive until either
        // read or write complete.
        select! {
            _ = read => { info!("WSClient read died");  },
            _ = write => { info!("WSClient write died"); },
        };
    });

    Ok(WsClient {
        project_id: project_id.to_string(),
        tx: wr_tx,
        rx: rd_rx,
        _ack_receiver: ack_rx,
        ack_broadcast: ack_tx,
        handler,
    })
}
