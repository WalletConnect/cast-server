use {
    anyhow::anyhow,
    crate::{
        error::Error,
        log::{info, warn},
        state::{AppState, WebhookNotificationEvent},
        types::{ClientData, Envelope, EnvelopeType0, LookupEntry},
        wsclient::WsClient,
        Result,
    },
    base64::Engine,
    chacha20poly1305::{
        aead::{generic_array::GenericArray, Aead},
        ChaCha20Poly1305,
        KeyInit,
    },
    mongodb::bson::doc,
    std::sync::Arc,
    walletconnect_sdk::rpc::rpc::Subscription,
};

pub async fn handle(
    params: Subscription,
    state: &Arc<AppState>,
    client: &mut WsClient,
) -> Result<()> {
    let topic = params.data.topic;
    let database = &state.database;
    let subscription_id = params.id;

    let Ok(Some(LookupEntry {
        project_id, 
        account,
        ..
    }))= database
        .collection::<LookupEntry>("lookup_table")
        .find_one_and_delete(doc! {"_id": &topic.to_string() }, None)
        .await
        else {
            return Err(Error::NoProjectDataForTopic(topic.to_string()))
        };

    let Ok(Some(acc)) = database
                 .collection::<ClientData>(&project_id)
                .find_one_and_delete(doc! {"_id": &account }, None)
                .await else {
                    return Err(Error::NoClientDataForTopic(topic.to_string()))
                };

    let Ok(message_bytes) = base64::engine::general_purpose::STANDARD
        .decode(params.data.message.to_string()) else {
            Err(anyhow!("Failed to decode message"))?
        };

    let envelope = Envelope::<EnvelopeType0>::from_bytes(message_bytes)?;
    let encryption_key = hex::decode(&acc.sym_key)?;
    let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

    let Ok(msg) = cipher.decrypt(
            GenericArray::from_slice(&envelope.iv),
            chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
        ) else {
            warn!(
                "Unregistered {} from {}, but couldn't decrypt message",
                account, project_id
            );
            return Err(Error::EncryptionError("Failed to decrypt".to_string()))
        };

    let msg = String::from_utf8(msg)?;
    info!(
        "Unregistered {} from {} with reason {}",
        account, project_id, msg
    );
    if let Err(e) = client.unsubscribe(topic.clone(), subscription_id).await {
        warn!("Error unsubscribing Cast from topic: {}", e);
    };

    state
        .notify_webhook(
            &project_id,
            WebhookNotificationEvent::Unsubscribed,
            &account,
        )
        .await?;

    Ok(())
}
