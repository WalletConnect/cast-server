use {
    crate::{
        error::Error,
        metrics::Metrics,
        types::{ClientData, LookupEntry, RegisterBody},
        unregister_service::UnregisterMessage,
        Configuration,
    },
    build_info::BuildInfo,
    mongodb::{bson::doc, options::ReplaceOptions},
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
    ) -> Result<(), Error> {
        let key = hex::decode(client_data.sym_key.clone())?;
        let topic = sha256::digest(&*key);
        // chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&key));

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

        Ok(())
    }

    pub fn set_metrics(&mut self, metrics: Metrics) {
        self.metrics = Some(metrics);
    }
}
