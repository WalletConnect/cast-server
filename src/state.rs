use {
    crate::{
        error::{Error, Result},
        metrics::Metrics,
        types::{ClientData, LookupEntry, RegisterBody, WebhookInfo},
        unregister_service::UnregisterMessage,
        Configuration,
    },
    build_info::BuildInfo,
    futures::TryStreamExt,
    log::info,
    mongodb::{bson::doc, options::ReplaceOptions},
    serde::{Deserialize, Serialize},
    std::sync::Arc,
    url::Url,
    walletconnect_sdk::rpc::auth::ed25519_dalek::Keypair,
};

pub struct AppState {
    pub config: Configuration,
    pub build_info: BuildInfo,
    pub metrics: Option<Metrics>,
    pub database: Arc<mongodb::Database>,
    pub keypair: Keypair,
    pub unregister_keypair: Keypair,
    pub unregister_tx: tokio::sync::mpsc::Sender<UnregisterMessage>,
}

build_info::build_info!(fn build_info);

impl AppState {
    pub fn new(
        config: Configuration,
        database: Arc<mongodb::Database>,
        keypair: Keypair,
        unregister_keypair: Keypair,
        unregister_tx: tokio::sync::mpsc::Sender<UnregisterMessage>,
    ) -> crate::Result<AppState> {
        let build_info: &BuildInfo = build_info();

        Ok(AppState {
            config,
            build_info: build_info.clone(),
            metrics: None,
            database,
            keypair,
            unregister_keypair,
            unregister_tx,
        })
    }

    // TODO: erroro
    pub async fn register_client(
        &self,
        project_id: &String,
        client_data: &RegisterBody,
        url: &Url,
    ) -> Result<()> {
        let key = hex::decode(client_data.sym_key.clone())?;
        let topic = sha256::digest(&*key);

        let insert_data = ClientData {
            id: client_data.account.clone(),
            relay_url: url.to_string().trim_end_matches('/').to_string(),
            sym_key: client_data.sym_key.clone(),
        };
        // Currently overwriting the document if it exists,
        // but we should probably just update the fields
        self.database
            .collection::<ClientData>(&project_id)
            .replace_one(
                doc! { "_id": client_data.account.clone()},
                insert_data,
                ReplaceOptions::builder().upsert(true).build(),
            )
            .await?;

        // TODO: Replace with const
        self.database
            .collection::<LookupEntry>("lookup_table")
            .replace_one(
                doc! { "_id": &topic},
                LookupEntry {
                    topic: topic.clone(),
                    project_id: project_id.clone(),
                    account: client_data.account.clone(),
                },
                ReplaceOptions::builder().upsert(true).build(),
            )
            .await?;

        self.unregister_tx
            .send(UnregisterMessage::Register(topic))
            .await
            .unwrap();

        self.notify_webhook(
            project_id,
            WebhookNotificationEvent::Subscribed,
            &client_data.account,
        )
        .await?;

        Ok(())
    }

    pub fn set_metrics(&mut self, metrics: Metrics) {
        self.metrics = Some(metrics);
    }

    pub async fn notify_webhook(
        &self,
        project_id: &str,
        event: WebhookNotificationEvent,
        account: &str,
    ) -> Result<()> {
        let mut cursor = self
            .database
            .collection::<WebhookInfo>("webhooks")
            .find(doc! { "project_id": project_id}, None)
            .await?;

        // Interate over cursor
        while let Some(webhook) = cursor.try_next().await? {
            dbg!(&webhook);
            if !webhook.events.contains(&event) {
                continue;
            }

            let client = reqwest::Client::new();
            let res = client
                .post(&webhook.url)
                .json(&WebhookMessage {
                    id: webhook.id.clone(),
                    event,
                    account: account.to_string(),
                })
                .send()
                .await?;

            info!(
                "Triggering webhook: {} resulted in http status: {}",
                webhook.id,
                res.status()
            );
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WebhookMessage {
    pub id: String,
    pub event: WebhookNotificationEvent,
    pub account: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WebhookNotificationEvent {
    Subscribed,
    Unsubscribed,
}
