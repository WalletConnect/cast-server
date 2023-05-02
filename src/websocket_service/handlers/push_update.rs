use {
    crate::{
        auth::SubscriptionAuth,
        log::info,
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry, RegisterBody},
        websocket_service::{NotifyMessage, NotifyResponse, NotifySubscribe},
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
    std::{sync::Arc, time::SystemTime},
    walletconnect_sdk::rpc::rpc::{
        Params,
        Payload,
        Publish,
        Request,
        Subscription,
        SubscriptionData,
    },
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
    let lookup_data = state
        .database
        .collection::<LookupEntry>("lookup_table")
        .find_one(doc!("_id":topic.clone()), None)
        .await?
        .ok_or(crate::error::Error::NoProjectDataForTopic(topic.clone()))?;
    info!("Fetched data for topic: {:?}", &lookup_data);

    let client_data = state
        .database
        .collection::<ClientData>(&lookup_data.project_id)
        .find_one(doc!("_id": lookup_data.account), None)
        .await?
        .ok_or(crate::error::Error::NoClientDataForTopic(topic.clone()))?;

    info!("Fetched client: {:?}", &client_data);

    let envelope = EnvelopeType0::from_bytes(
        base64::engine::general_purpose::STANDARD.decode(message.to_string())?,
    );

    let encryption_key = hex::decode(client_data.sym_key.clone())?;

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

    let msg = cipher
        .decrypt(
            GenericArray::from_slice(&envelope.iv),
            chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
        )
        .map_err(|_| crate::error::Error::EncryptionError("Failed to decrypt".into()))?;

    let msg: NotifyMessage<NotifySubscribe> = serde_json::from_slice(&msg)?;

    let sub_auth: SubscriptionAuth = {
        let params = msg.params;
        let jwt = params.subscription_auth;

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

    let mut rng = OsRng {};

    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));
    let uniform = Uniform::from(0u8..=255);
    let nonce: GenericArray<u8, U12> =
        GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

    // TODO: fix id
    let response = NotifyResponse::<bool> {
        id: msg.id.into(),
        jsonrpc: "2.0".into(),
        result: true,
    };
    let response = cipher
        .encrypt(&nonce, serde_json::to_string(&response)?.as_bytes())
        .map_err(|_| crate::error::Error::EncryptionError("Encryption failed".into()))?;

    info!("Decrypted response");

    let envelope = EnvelopeType0 {
        envelope_type: 0,
        iv: nonce.into(),
        sealbox: response.to_vec(),
    };
    let base64_notification = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let id = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis() as u64;

    client
        .send_raw(Payload::Request(Request {
            id: id.into(),
            jsonrpc: "2.0".into(),
            params: Params::Publish(Publish {
                topic: topic.into(),
                message: base64_notification.into(),
                ttl_secs: 86400,
                tag: 4009,
                prompt: false,
            }),
        }))
        .await?;

    let client_data = RegisterBody {
        account: sub_auth.sub.trim_start_matches("did:pkh:").into(),
        relay_url: state.config.relay_url.clone(),
        sym_key: client_data.sym_key.clone(),
        scope: sub_auth.scp.split(" ").map(|s| s.into()).collect(),
    };
    info!("Updating client: {:?}", &client_data);

    state
        .register_client(
            &lookup_data.project_id,
            &client_data,
            &url::Url::parse(&state.config.relay_url)?,
        )
        .await?;

    Ok(())
}
