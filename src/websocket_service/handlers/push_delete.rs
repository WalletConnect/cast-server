use {
    crate::{
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

    match database
        .collection::<LookupEntry>("lookup_table")
        .find_one_and_delete(doc! {"_id": &topic.clone().to_string() }, None)
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
                                    if let Err(e) =
                                        client.unsubscribe(topic.clone(), subscription_id).await
                                    {
                                        warn!("Error unsubscribing Cast from topic: {}", e);
                                    };

                                    state
                                        .notify_webhook(
                                            &project_id,
                                            WebhookNotificationEvent::Unsubscribed,
                                            &account,
                                        )
                                        .await?;
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
    Ok(())
}
