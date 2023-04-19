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
        consts::U12,
        ChaCha20Poly1305,
        KeyInit,
    },
    data_encoding::{BASE64URL_NOPAD, BASE64_NOPAD},
    futures::{executor, future, select, FutureExt, StreamExt},
    log::debug,
    mongodb::{bson::doc, Database},
    rand::{distributions::Uniform, prelude::Distribution},
    rand_core::OsRng,
    serde::{Deserialize, Serialize},
    sha2::{Digest, Sha256},
    std::{io::Read, sync::Arc, time::SystemTime},
    tokio::sync::mpsc::Receiver,
    walletconnect_sdk::rpc::{
        domain::MessageId,
        rpc::{
            Params,
            Payload,
            Publish,
            Request,
            Response,
            Subscription,
            SubscriptionData,
            SuccessfulResponse,
        },
    },
    x25519_dalek::x25519,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestBody {
    pub id: MessageId,
    pub jsonrpc: String,
    pub params: String,
}

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
                                    let msg_id = msg.id();
                                    if let Err(e) = handle_msg(msg, &self.state, &mut self.client).await {
                                        warn!("Error handling message: {}", e);
                                    }

                                    self.client.send_ack(msg_id).await?
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
    debug!("Resubscribing to all topics");
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
                            let encryption_key = hex::decode(&acc.sym_key)?;
                            let cipher =
                                ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

                            match cipher.decrypt(
                                GenericArray::from_slice(&envelope.iv),
                                chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
                            ) {
                                Ok(msg) => {
                                    let msg = String::from_utf8(msg)?;
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
        .ok_or(crate::error::Error::NoProjectDataForTopic(topic))?;

    let envelope = EnvelopeType1::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(message.to_string())?,
    );

    info!("pubkey: {}", hex::encode(&envelope.pubkey));

    let sym_key = derive_key(hex::encode(envelope.pubkey), project_data.private_key)?;
    info!("sym_key: {}", &sym_key);

    let encryption_key = hex::decode(&sym_key)?;
    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

    let sub_auth: SubscriptionAuth = {
        let msg = cipher
            .decrypt(
                GenericArray::from_slice(&envelope.iv),
                chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
            )
            .map_err(|_| crate::error::Error::EncryptionError("Failed to encrypt".into()))?;
        let msg: serde_json::Value = serde_json::from_slice(&msg)?;
        dbg!(&msg);

        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

        let id = msg
            .get("id")
            .ok_or(crate::error::Error::JsonGetError)?
            .as_u64()
            .ok_or(crate::error::Error::JsonGetError)?;
        let params = msg
            .get("params")
            .ok_or(crate::error::Error::JsonGetError)?
            .as_object()
            .ok_or(crate::error::Error::JsonGetError)?;
        let jwt = params
            .get("subscriptionAuth")
            .ok_or(crate::error::Error::SubscriptionAuthError(
                "Subscription auth missing in request".into(),
            ))?
            .to_string();

        let claims = jwt.split(".").collect::<Vec<&str>>()[1];

        let claims = match BASE64_NOPAD.decode(claims.as_bytes()) {
            Ok(claims) => claims,
            Err(_) => {
                info!("Invalid JWT");
                return Ok(());
            }
        };

        info!("Sent");

        let uniform = Uniform::from(0u8..=255);

        let mut rng = OsRng {};
        let nonce: GenericArray<u8, U12> =
            GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

        let response = Payload::Response(walletconnect_sdk::rpc::rpc::Response::Success(
            SuccessfulResponse {
                id: id.into(),
                jsonrpc: "2.0".into(),
                result: serde_json::Value::Bool(true),
            },
        ));
        let response = cipher
            .encrypt(&nonce, serde_json::to_string(&response)?.as_bytes())
            .map_err(|_| crate::error::Error::EncryptionError("Encryption failed".into()))?;

        let envelope = EnvelopeType0 {
            envelope_type: 0,
            iv: nonce.into(),
            sealbox: response.to_vec(),
        };
        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        let key = hex::decode(sym_key.clone())?;
        let client_topic = sha256::digest(&*key);
        info!("client_topic: {}", &client_topic);

        info!("sending");
        let id = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_millis() as u64;
        client
            .send_raw(Payload::Request(Request {
                id: id.into(),
                jsonrpc: "2.0".into(),
                params: Params::Publish(Publish {
                    topic: client_topic.into(),
                    message: base64_notification.into(),
                    ttl_secs: 86400,
                    tag: 4007,
                    prompt: false,
                }),
            }))
            .await?;
        // TODO: tu odp
        serde_json::from_slice(&claims)?
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
            &url::Url::parse(&state.config.relay_url)?,
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

fn derive_key(pubkey: String, privkey: String) -> Result<String> {
    let pubkey: [u8; 32] = hex::decode(pubkey)?[..32].try_into()?;
    let privkey: [u8; 32] = hex::decode(privkey)?[..32].try_into()?;

    let secret_key = x25519_dalek::StaticSecret::from(privkey);
    let public_key = x25519_dalek::PublicKey::from(pubkey);

    let shared_key = secret_key.diffie_hellman(&public_key);

    let derived_key = hkdf::Hkdf::<Sha256>::new(None, shared_key.as_bytes());

    let mut expanded_key = [0u8; 32];
    derived_key
        .expand(b"", &mut expanded_key)
        .map_err(|_| crate::error::Error::HkdfInvalidLength)?;
    Ok(hex::encode(expanded_key))
}

#[test]
fn test_derive() {
    let keypriv1 = "1fb63fca5c6ac731246f2f069d3bc2454345d5208254aa8ea7bffc6d110c8862";
    let keypriv2 = "36bf507903537de91f5e573666eaa69b1fa313974f23b2b59645f20fea505854";
    let keypub1 = "ff7a7d5767c362b0a17ad92299ebdb7831dcbd9a56959c01368c7404543b3342";
    let keypub2 = "590c2c627be7af08597091ff80dd41f7fa28acd10ef7191d7e830e116d3a186a";
    let expected = "0653ca620c7b4990392e1c53c4a51c14a2840cd20f0f1524cf435b17b6fe988c";
    assert_eq!(
        expected,
        derive_key(keypub1.to_string(), keypriv2.to_string()).unwrap()
    );
    assert_eq!(
        expected,
        derive_key(keypub2.to_string(), keypriv1.to_string()).unwrap(),
    );
}
