pub use walletconnect_sdk::rpc::domain::MessageId;
use {
    crate::error::Result,
    std::time::Duration,
    tokio::sync::mpsc,
    tracing::{info, warn},
    tungstenite::protocol::CloseFrame,
    walletconnect_sdk::{
        client::{websocket::ConnectionHandler, ConnectionOptions},
        rpc::{
            auth::{ed25519_dalek::Keypair, AuthToken},
            user_agent::ValidUserAgent,
        },
    },
};

// #[derive(Debug)]
// pub struct WsClient {
//     pub project_id: String,
//     pub tx: mpsc::Sender<Message>,
//     pub rx: mpsc::Receiver<Result<Message>>,
//     pub ack_broadcast: tokio::sync::broadcast::Sender<MessageId>,
//     _ack_receiver: tokio::sync::broadcast::Receiver<MessageId>,
//     pub handler: tokio::task::JoinHandle<()>,
// }

// impl WsClient {
//     pub fn is_finished(&self) -> bool {
//         self.handler.is_finished()
//     }

//     pub async fn reconnect(&mut self, url: &str, keypair: &Keypair) ->
// Result<()> {         let jwt = jwt_token(url, keypair)?;
//         connect(url, &self.project_id, jwt).await?;
//         Ok(())
//     }

//     pub async fn send(&mut self, msg: Request) -> Result<()> {
//         self.send_raw(Payload::Request(msg)).await
//     }

//     pub async fn send_ack(&mut self, id:
// walletconnect_sdk::rpc::domain::MessageId) -> Result<()> {         self.
// send_raw(Payload::Response(
// walletconnect_sdk::rpc::rpc::Response::Success(SuccessfulResponse::new(
//                 id,
//                 serde_json::Value::Bool(true),
//             )),
//         ))
//         .await
//     }

//     pub async fn send_plaintext(&mut self, msg: String) -> Result<()> {
//         self.tx
//             .send(Message::Text(msg))
//             .await
//             .map_err(|_| crate::error::Error::ChannelClosed)
//     }

//     pub async fn send_raw(&mut self, msg: Payload) -> Result<()> {
//         info!("Sending to Relay WS: {:?}", msg);
//         let msg = serde_json::to_string(&msg)?;
//         dbg!(
//             "\n\n\n\n\n\n\n",
//             &msg,
//
// "\n\n\n\n\n\n\
// n---------------------------------------------------------------n"         );
//         self.tx
//             .send(Message::Text(msg))
//             .await
//             .map_err(|_| crate::error::Error::ChannelClosed)
//     }

//     pub async fn recv(&mut self) ->
// Result<walletconnect_sdk::rpc::rpc::Request> {         loop {
//             match self.rx.recv().await {
//                 Some(msg) => match msg? {
//                     Message::Text(msg) => {
//                         match serde_json::from_str::<Payload>(&msg)? {
//                             Payload::Request(req) => {
//                                 info!("Received request from Relay WS: {:?}",
// req);                                 return Ok(req);
//                             }
//                             Payload::Response(res) => {
//                                 info!("Received response from Relay WS:
// {:?}", res);                                 let id = res.id();
//                                 // TODO: Remove unwrap; distinguish between
// response types                                 if let Err(e) =
// self.ack_broadcast.send(id) {
// warn!("Failed to broadcast ack: {}", e);                                 }
//                             }
//                         }
//                     }
//                     Message::Ping(_) => {
//                         info!("Received ping from Relay WS, sending pong");
//                         self.pong().await?;
//                     }
//                     e => {
//                         warn!("Received unsupported message from relay:
// {:?}", e);                     }
//                 },
//                 None => {
//                     return Err(crate::error::Error::ChannelClosed);
//                 }
//             }
//         }
//     }

//     async fn pong(&mut self) -> Result<()> {
//         let msg = Message::Pong("heartbeat".into());
//         self.tx
//             .send(msg)
//             .await
//             .map_err(|_| crate::error::Error::ChannelClosed)
//     }

//     pub async fn publish(&mut self, topic: &str, payload: &str) ->
// Result<MessageId> {         self.publish_with_tag(topic, payload, 1000).await
//     }

//     pub async fn publish_with_tag(
//         &mut self,
//         topic: &str,
//         payload: &str,
//         tag: u32,
//     ) -> Result<MessageId> {
//         let msg = Payload::Request(new_rpc_request(
//             walletconnect_sdk::rpc::rpc::Params::Publish(Publish {
//                 topic: topic.into(),
//                 message: payload.into(),
//                 ttl_secs: 86400,
//                 tag,
//                 prompt: true,
//             }),
//         ));

//         let id = msg.id().clone();
//         self.send_raw(msg).await?;
//         Ok(id)
//     }

//     /// Sends batch of messages with provided tag to relay. Returns set of
//     /// topics for which relay didn't ack the published message..
//     pub async fn batch_publish_with_tag(
//         &mut self,
//         messages: Vec<(Topic, String)>,
//         tag: u32,
//     ) -> Result<HashSet<Topic>> {
//         info!("Starting batch publish with tag {}", tag);
//         if messages.is_empty() {
//             return Ok(HashSet::new());
//         }

//         let mut unacked: HashSet<MessageId> = HashSet::new();
//         let mut mapping: HashMap<MessageId, Topic> = HashMap::new();

//         let mut acks = self.ack_broadcast.subscribe();

//         info!("Messages len {} ", messages.len());

//         for (topic, payload) in messages.into_iter() {
//             info!("Sending to relay: {:?}", topic);
//             // let request =
//             //
// new_rpc_request(walletconnect_sdk::rpc::rpc::Params::Publish(Publish {
//             //     topic: topic.clone(),
//             //     message: payload.into(),
//             //     ttl_secs: 86400,
//             //     tag,
//             //     prompt: true,
//             // }));

//             let id = self.publish_with_tag(topic.value(), &payload,
// tag).await?;             mapping.insert(id, topic.clone());
//             unacked.insert(id);

//             info!("Sending to relay, want ack for {:?}", id);
//         }

//         let timeout_duration = Duration::from_secs(60);

//         // We don't care about time running out, we just return the unacked
//         _ = tokio::time::timeout(timeout_duration, async {
//             info!("Waiting for acks from Relay WS...");
//             while let Ok(ack) = acks.recv().await {
//                 info!("Received ack from Relay WS: {:?}", ack);
//                 unacked.remove(&ack);
//                 if unacked.is_empty() {
//                     break;
//                 }
//             }
//         })
//         .await;

//         let unacked = unacked
//             .into_iter()
//             .filter_map(|id| mapping.remove(&id))
//             .collect();

//         Ok(unacked)
//     }

//     pub async fn subscribe(&mut self, topic: &str) -> Result<()> {
//         let msg =
// Payload::Request(new_rpc_request(Params::Subscribe(Subscribe {
// topic: topic.into(),         })));
//         self.send_raw(msg).await
//     }

//     pub async fn batch_subscribe(&mut self, topics: Vec<String>) ->
// Result<()> {         let msg =
// Payload::Request(new_rpc_request(Params::BatchSubscribe(BatchSubscribe {
//             topics: topics.into_iter().map(|x| x.into()).collect(),
//         })));
//         self.send_raw(msg).await
//     }

//     pub async fn unsubscribe(
//         &mut self,
//         topic: Topic,
//         subscription_id: SubscriptionId,
//     ) -> Result<()> {
//         let msg =
// Payload::Request(new_rpc_request(Params::Unsubscribe(Unsubscribe {
//             topic,
//             subscription_id,
//         })));
//         self.send_raw(msg).await
//     }
// }

// pub fn new_rpc_request(params: Params) -> Request {
//     let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

//     let id = id * 1000 + rand::thread_rng().gen_range(100, 1000);

//     Request {
//         id: id.into(),
//         jsonrpc: "2.0".into(),
//         params,
//     }
// }

// pub async fn connect(url: &str, project_id: &str, jwt: String) ->
// Result<WsClient> {     info!("Connecting to Relay WS ({})...", url);
//     let relay_query =
// format!("auth={jwt}&projectId={project_id}&ua=wc-2/rust/cast");
//     dbg!(&relay_query);

//     let mut url = url::Url::parse(url)?;
//     url.set_query(Some(&relay_query));

//     let (connection, _) =
// async_tungstenite::tokio::connect_async(&url).await?;

//     // A channel for passing messages to websocket
//     let (wr_tx, wr_rx) = mpsc::channel(1024);
//     let (ack_tx, ack_rx) = tokio::sync::broadcast::channel(1024);

//     // A channel for incoming messages from websocket
//     let (rd_tx, rd_rx) = mpsc::channel::<Result<_>>(1024);

//     let handler = tokio::spawn(async move {
//         let (tx, rx) = connection.split();
//         // Forward messages to the write half
//         let write = ReceiverStream::new(wr_rx).map(Ok).forward(tx);

//         // Process incoming messages and close the
//         // client in case of a close frame. Else forward them.
//         let read = rx
//             .take_while(|result| match result {
//                 Err(_) => future::ready(false),
//                 Ok(m) => future::ready(!m.is_close()),
//             })
//             .for_each_concurrent(None, |result| async {
//                 if let Err(e) = rd_tx.send(result.map_err(Into::into)).await
// {                     warn!("WSClient send error: {}", e);
//                 };
//             });

//         // Keep the thread alive until either
//         // read or write complete.
//         select! {
//             _ = read => { info!("WSClient read died");  },
//             _ = write => { info!("WSClient write died"); },
//         };
//     });

//     Ok(WsClient {
//         project_id: project_id.to_string(),
//         tx: wr_tx,
//         rx: rd_rx,
//         _ack_receiver: ack_rx,
//         ack_broadcast: ack_tx,
//         handler,
//     })
// }

pub struct RelayConnectionHandler {
    name: &'static str,
    tx: mpsc::UnboundedSender<RelayClientEvent>,
}

pub enum RelayClientEvent {
    Message(walletconnect_sdk::client::websocket::PublishedMessage),
    Error(walletconnect_sdk::client::error::Error),
    Disconnected(Option<CloseFrame<'static>>),
    Connected,
}

impl RelayConnectionHandler {
    pub fn new(name: &'static str, tx: mpsc::UnboundedSender<RelayClientEvent>) -> Self {
        Self { name, tx }
    }
}

impl ConnectionHandler for RelayConnectionHandler {
    fn connected(&mut self) {
        info!("[{}]connection open", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Connected) {
            warn!("[{}] failed to emit the connection event: {}", self.name, e);
        }
    }

    fn disconnected(&mut self, frame: Option<CloseFrame<'static>>) {
        info!("[{}] connection closed: frame={frame:?}", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Disconnected(frame)) {
            warn!(
                "[{}] failed to emit the disconnection event: {}",
                self.name, e
            );
        }
    }

    fn message_received(
        &mut self,
        message: walletconnect_sdk::client::websocket::PublishedMessage,
    ) {
        info!(
            "[{}] inbound message: topic={} message={}",
            self.name, message.topic, message.message
        );
        if let Err(e) = self.tx.send(RelayClientEvent::Message(message)) {
            warn!("[{}] failed to emit the message event: {}", self.name, e);
        }
    }

    fn inbound_error(&mut self, error: walletconnect_sdk::client::error::Error) {
        info!("[{}] inbound error: {error}", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Error(error)) {
            warn!(
                "[{}] failed to emit the inbound error event: {}",
                self.name, e
            );
        }
    }

    fn outbound_error(&mut self, error: walletconnect_sdk::client::error::Error) {
        info!("[{}] outbound error: {error}", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Error(error)) {
            warn!(
                "[{}] failed to emit the outbound error event: {}",
                self.name, e
            );
        }
    }
}

pub fn create_connection_opts(
    relay_url: &str,
    project_id: &str,
    keypair: &Keypair,
    cast_url: &str,
) -> Result<ConnectionOptions> {
    let auth = AuthToken::new(cast_url)
        .aud(relay_url)
        .ttl(Duration::from_secs(60 * 60))
        .as_jwt(&keypair)?;

    let ua = ValidUserAgent {
        protocol: walletconnect_sdk::rpc::user_agent::Protocol {
            kind: walletconnect_sdk::rpc::user_agent::ProtocolKind::WalletConnect,
            version: 2,
        },
        sdk: walletconnect_sdk::rpc::user_agent::Sdk {
            language: walletconnect_sdk::rpc::user_agent::SdkLanguage::Rust,
            // TODO: proper version
            version: "1.0".to_string(),
        },
        os: walletconnect_sdk::rpc::user_agent::OsInfo {
            os_family: "ECS".into(),
            ua_family: None,
            version: None,
        },
        id: Some(walletconnect_sdk::rpc::user_agent::Id {
            environment: walletconnect_sdk::rpc::user_agent::Environment::Unknown(
                "Notify Server".into(),
            ),
            host: Some(cast_url.into()),
        }),
    };
    let user_agent = walletconnect_sdk::rpc::user_agent::UserAgent::ValidUserAgent(ua);

    let opts = ConnectionOptions::new(project_id, auth).with_user_agent(user_agent);
    Ok(opts)
}
