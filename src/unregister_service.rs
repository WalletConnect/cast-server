use {
    crate::{
        auth::{jwt_token, SubscriptionAuth},
        handlers::subscribe_topic::ProjectData,
        log::{info, warn},
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, EnvelopeType1, LookupEntry, RegisterBody},
        wsclient::{self, WsClient},
        Result,
    },
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead},
        ChaCha20Poly1305,
        KeyInit,
    },
    data_encoding::BASE64URL_NOPAD,
    futures::{executor, future, select, FutureExt, StreamExt},
    mongodb::{bson::doc, Database},
    std::sync::Arc,
    tokio::sync::mpsc::Receiver,
    walletconnect_sdk::rpc::{
        domain::MessageId,
        rpc::{Params, Payload, Subscription, SubscriptionData, SuccessfulResponse},
    },
};

#[derive(Debug, Clone)]
pub enum UnregisterMessage {
    Register(String),
}

pub struct UnregisterService {
    state: Arc<AppState>,
    client: WsClient,
    rx: Receiver<UnregisterMessage>,
}

impl UnregisterService {
    pub async fn new(app_state: Arc<AppState>, rx: Receiver<UnregisterMessage>) -> Result<Self> {
        let url = app_state.config.relay_url.clone();

        let mut client = wsclient::connect(
            &app_state.config.relay_url,
            &app_state.config.project_id,
            jwt_token(&url, &app_state.unregister_keypair)?,
        )
        .await?;

        resubscribe(&app_state.database, &mut client).await?;

        Ok(Self {
            rx,
            state: app_state,
            client,
        })
    }

    async fn reconnect(&mut self) -> Result<()> {
        let url = self.state.config.relay_url.clone();
        let jwt = jwt_token(&url, &self.state.unregister_keypair)?;
        self.client = wsclient::connect(&url, &self.state.config.project_id, jwt).await?;
        resubscribe(&self.state.database, &mut self.client).await?;
        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            match self.client.handler.is_finished() {
                true => {
                    warn!("Client handler has finished, spawning new one");
                    self.reconnect().await?;
                }
                false => {
                    select! {

                    msg =  self.rx.recv().fuse() => {
                        if let Some(msg) = msg {
                            let UnregisterMessage::Register(topic) = msg;
                                info!("Subscribing to topic: {}", topic);
                                if let Err(e) = self.client.subscribe(&topic).await {
                                    warn!("Error subscribing to topic: {}", e);
                                }
                          }
                    }
                        ,
                        message = self.client.recv().fuse() => {
                            match message {
                                Ok(msg) => {
                                    handle_msg(msg, &self.state, &mut self.client).await?
                                },
                                Err(_) => {
                                    warn!("Client handler has finished, spawning new one");
                                   self.reconnect().await?;

                                }
                            }



                        }
                    };
                }
            }
        }
    }
}

async fn resubscribe(database: &Arc<Database>, client: &mut WsClient) -> Result<()> {
    // TODO: Sub to all
    info!("Resubscribing to all topics");
    // Get all topics from db
    let cursor = database
        .collection::<LookupEntry>("lookup_table")
        .find(None, None)
        .await?;

    // Iterate over all topics and sub to them again using the _id field from each
    // record
    // Chunked into 500, as thats the max relay is allowing
    cursor
        .chunks(500)
        .for_each(|chunk| {
            let topics = chunk
                .into_iter()
                .filter_map(|x| x.ok())
                .map(|x| x.topic)
                .collect::<Vec<String>>();
            if let Err(e) = executor::block_on(client.batch_subscribe(topics)) {
                warn!("Error resubscribing to topics: {}", e);
            }
            future::ready(())
        })
        .await;

    let cursor = database
        .collection::<ProjectData>("project_data")
        .find(None, None)
        .await?;

    cursor
        .chunks(500)
        .for_each(|chunk| {
            let topics = chunk
                .into_iter()
                .filter_map(|x| x.ok())
                .map(|x| x.topic)
                .collect::<Vec<String>>();
            if let Err(e) = executor::block_on(client.batch_subscribe(topics)) {
                warn!("Error resubscribing to topics: {}", e);
            }
            future::ready(())
        })
        .await;
    Ok(())
}

async fn handle_msg(msg: Payload, state: &Arc<AppState>, client: &mut WsClient) -> Result<()> {
    info!("Unregister service received message: {:?}", msg);
    if let Payload::Request(req) = msg {
        if let Params::Subscription(params) = req.params {
            match params.data.tag {
                4004 => {
                    info!("Received push delete for topic: {}", params.data.topic);
                    handle_push_delete(params, state, client).await?;
                }
                4006 => {
                    info!("Received push subscribe on topic: {}", params.data.topic);
                    handle_subscribe_message(params, state, client, req.id).await?;
                }
                _ => {
                    info!("Ignored tag: {}", params.data.tag);
                }
            }
        } else {
            info!("Ignored request: {:?}", req);
        }
        client.send_ack(req.id).await?
    }
    // TODO: This shouldnt be needed
    Ok(())
}

// Handle wc_pushDelete

async fn handle_push_delete(
    params: Subscription,
    state: &Arc<AppState>,
    client: &mut WsClient,
) -> Result<()> {
    let topic = params.data.topic;
    let database = &state.database;
    let subscription_id = params.id;
    // TODO: Keep subscription id in db
    if let Err(e) = client.unsubscribe(topic.clone(), subscription_id).await {
        warn!("Error unsubscribing Cast from topic: {}", e);
    };

    let topic = topic.to_string();

    match database
        .collection::<LookupEntry>("lookup_table")
        .find_one_and_delete(doc! {"_id": &topic }, None)
        .await
    {
        Ok(Some(LookupEntry {
            project_id,
            account,
            ..
        })) => {
            match database
                .collection::<ClientData>(&project_id)
                .find_one_and_delete(doc! {"_id": &account }, None)
                .await
            {
                Ok(Some(acc)) => {
                    match base64::engine::general_purpose::STANDARD
                        .decode(params.data.message.to_string())
                    {
                        Ok(message_bytes) => {
                            let envelope = EnvelopeType0::from_bytes(message_bytes);
                            // Safe unwrap - we are sure that stored keys are valid
                            let encryption_key = hex::decode(&acc.sym_key).unwrap();
                            let cipher =
                                ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

                            match cipher.decrypt(
                                GenericArray::from_slice(&envelope.iv),
                                chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
                            ) {
                                Ok(msg) => {
                                    let msg = String::from_utf8(msg).unwrap();
                                    info!(
                                        "Unregistered {} from {} with reason {}",
                                        account, project_id, msg
                                    );
                                }
                                Err(e) => {
                                    warn!(
                                        "Unregistered {} from {}, but couldn't decrypt reason \
                                         data: {}",
                                        account, project_id, e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Unregistered {} from {}, but couldn't decode base64 message \
                                 data: {}",
                                project_id,
                                params.data.message.to_string(),
                                e
                            );
                        }
                    };
                }
                Ok(None) => {
                    warn!("No entry found for account: {}", &account);
                }
                Err(e) => {
                    warn!("Error unregistering account {}: {}", &account, e);
                }
            }
        }
        Ok(None) => {
            warn!("No entry found for topic: {}", &topic);
        }
        Err(e) => {
            warn!("Error unregistering from topic {}: {}", &topic, e);
        }
    }
    // TODO: fix
    Ok(())
}

// Handle wc_pushSubscribe
async fn handle_subscribe_message(
    params: Subscription,
    state: &Arc<AppState>,
    client: &mut WsClient,
    id: MessageId,
) -> Result<()> {
    let Subscription {
        data: SubscriptionData { message, topic, .. },
        ..
    } = params;

    let topic = topic.to_string();

    // Grab record from db
    let project_data = state
        .database
        .collection::<ProjectData>("project_data")
        .find_one(doc!("topic":topic.clone()), None)
        .await?
        .unwrap();

    let envelope = EnvelopeType1::from_bytes(
        base64::engine::general_purpose::STANDARD
            .decode(message.to_string())
            .unwrap(),
    );

    let sym_key = derive_key(hex::encode(envelope.pubkey), project_data.private_key);
    info!("sym_key: {}", &sym_key);

    let encryption_key = hex::decode(&sym_key).unwrap();
    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

    let sub_auth: SubscriptionAuth = {
        let msg = cipher.decrypt(
            GenericArray::from_slice(&envelope.iv),
            chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
        );
        let msg: serde_json::Value = serde_json::from_slice(&msg.unwrap()).unwrap();

        let jwt = msg.get("subscriptionAuth").unwrap().to_string();
        let claims = jwt.split(".").collect::<Vec<&str>>()[1];
        let claims = BASE64URL_NOPAD.decode(claims.as_bytes()).unwrap();
        serde_json::from_slice(&claims).unwrap()
    };

    let client_data = RegisterBody {
        account: sub_auth.sub.trim_start_matches("did:pkh:").into(),
        relay_url: state.config.relay_url.clone(),
        sym_key: sym_key.clone(),
    };
    info!("Registering client: {:?}", &client_data);

    state
        .register_client(
            &project_data.id,
            &client_data,
            &url::Url::parse(&state.config.relay_url).unwrap(),
        )
        .await?;

    // Send response to relay
    client
        .send_raw(Payload::Response(
            walletconnect_sdk::rpc::rpc::Response::Success(SuccessfulResponse {
                id,
                jsonrpc: "2.0".into(),
                result: serde_json::Value::Bool(true),
            }),
        ))
        .await?;
    // TODO: should be sth else
    Ok(())
}

fn derive_key(pubkey: String, privkey: String) -> String {
    let pubkey: [u8; 32] = hex::decode(pubkey).unwrap()[..32].try_into().unwrap();
    let privkey: [u8; 32] = hex::decode(privkey).unwrap()[..32].try_into().unwrap();

    let secret_key = x25519_dalek::StaticSecret::from(privkey);
    let public_key = x25519_dalek::PublicKey::from(pubkey);

    let shared_key = secret_key.diffie_hellman(&public_key);
    hex::encode(shared_key.as_bytes())
}
