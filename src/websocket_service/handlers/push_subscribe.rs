use {
    crate::{
        auth::SubscriptionAuth,
        handlers::subscribe_topic::ProjectData,
        log::info,
        state::AppState,
        types::{Envelope, EnvelopeType0, EnvelopeType1, RegisterBody},
        websocket_service::{derive_key, NotifyResponse},
        wsclient::WsClient,
        Result,
    },
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead},
        consts::U12,
        ChaCha20Poly1305,
        KeyInit,
    },
    data_encoding::BASE64_NOPAD,
    mongodb::bson::doc,
    rand::{distributions::Uniform, prelude::Distribution},
    rand_core::OsRng,
    serde_json::{json, Value},
    std::{sync::Arc, time::SystemTime},
    walletconnect_sdk::rpc::rpc::{
        Params,
        Payload,
        Publish,
        Request,
        Subscription,
        SubscriptionData,
    },
    x25519_dalek::{PublicKey, StaticSecret},
};

pub async fn handle(
    params: Subscription,
    state: &Arc<AppState>,
    client: &mut WsClient,
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
    let client_pubkey = envelope.pubkey;

    info!("pubkey: {}", hex::encode(&envelope.pubkey));

    let response_sym_key = derive_key(hex::encode(client_pubkey), project_data.private_key)?;
    info!("response_sym_key: {}", &response_sym_key);

    let response_encryption_key = hex::decode(&response_sym_key)?;
    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&response_encryption_key));

    let msg = cipher
        .decrypt(
            GenericArray::from_slice(&envelope.iv),
            chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
        )
        .map_err(|_| crate::error::Error::EncryptionError("Failed to encrypt".into()))?;
    let msg: serde_json::Value = serde_json::from_slice(&msg)?;
    info!("msg: {:?}", &msg);
    let id: &Value = msg.get("id").ok_or(crate::error::Error::JsonGetError)?;

    // .as_str()
    // .unwrap()
    // .parse()
    // .unwrap();

    let sub_auth: SubscriptionAuth = {
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

        serde_json::from_slice(&claims)?
    };
    let uniform = Uniform::from(0u8..=255);

    let mut rng = OsRng {};
    let nonce: GenericArray<u8, U12> =
        GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

    let secret = StaticSecret::random_from_rng(chacha20poly1305::aead::OsRng {});
    let public = PublicKey::from(&secret);

    let response = NotifyResponse::<Value> {
        id: id.as_u64().unwrap(),
        jsonrpc: "2.0".into(),
        result: json!({"publicKey": hex::encode(public.to_bytes())}),
    };
    info!("response: {}", serde_json::to_string(&response)?);

    let push_key = derive_key(hex::encode(client_pubkey), hex::encode(secret))?;
    info!("push_key: {}", &push_key);

    let response = cipher
        .encrypt(&nonce, serde_json::to_string(&response)?.as_bytes())
        .map_err(|_| crate::error::Error::EncryptionError("Encryption failed".into()))?;

    let envelope = EnvelopeType0 {
        envelope_type: 0,
        iv: nonce.into(),
        sealbox: response.to_vec(),
    };

    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let key = hex::decode(response_sym_key)?;
    let response_topic = sha256::digest(&*key);
    info!("response_topic: {}", &response_topic);

    let id = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis() as u64;
    client
        .send_raw(Payload::Request(Request {
            id: id.into(),
            jsonrpc: "2.0".into(),
            params: Params::Publish(Publish {
                topic: response_topic.into(),
                message: base64_notification.into(),
                ttl_secs: 86400,
                tag: 4007,
                prompt: false,
            }),
        }))
        .await?;

    let client_data = RegisterBody {
        account: sub_auth.sub.trim_start_matches("did:pkh:").into(),
        relay_url: state.config.relay_url.clone(),
        sym_key: push_key.clone(),
        scope: sub_auth.scp.split(" ").map(|s| s.into()).collect(),
    };
    info!("Registering client: {:?}", &client_data);

    state
        .register_client(
            &project_data.id,
            &client_data,
            &url::Url::parse(&state.config.relay_url)?,
        )
        .await?;

    Ok(())
}
