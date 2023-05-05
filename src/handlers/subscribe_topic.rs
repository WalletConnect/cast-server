use {
    crate::{error, state::AppState, websocket_service},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    hyper::HeaderMap,
    log::info,
    mongodb::{bson::doc, options::ReplaceOptions},
    serde::{Deserialize, Serialize},
    serde_json::json,
    std::sync::Arc,
    x25519_dalek::{PublicKey, StaticSecret},
};

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjectData {
    #[serde(rename = "_id")]
    pub id: String,
    pub private_key: String,
    pub public_key: String,
    pub topic: String,
}

pub async fn handler(
    headers: HeaderMap,
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, crate::error::Error> {
    info!("Generating keypair for project: {}", project_id);
    let db = state.database.clone();

    match headers.get("Authorization") {
        Some(project_secret) => {
            // TODO: Check if project_secret is valid
            // Also strange thing happens when I change the project_secret to a different
            // value but small difference
            let seed: [u8; 32] = project_secret.as_bytes()[..32]
                .try_into()
                .map_err(|_| error::Error::InvalidKeypairSeed)?;

            // let keypair = Keypair::generate(&mut seeded);
            let secret = StaticSecret::from(seed);
            let public = PublicKey::from(&secret);

            let public_key = hex::encode(public.to_bytes());

            let topic = sha256::digest(&public.to_bytes());
            let project_data = ProjectData {
                id: project_id.clone(),
                private_key: hex::encode(secret.to_bytes()),
                public_key: public_key.clone(),
                topic: topic.clone(),
            };

            info!(
                "Saving project_info to database for project: {} with pubkey: {}",
                project_id, public_key
            );

            db.collection::<ProjectData>("project_data")
                .replace_one(
                    doc! { "_id": project_id.clone()},
                    project_data,
                    ReplaceOptions::builder().upsert(true).build(),
                )
                .await?;

            info!("Subscribing to project topic: {}", &topic);
            state
                .unregister_tx
                .send(websocket_service::WebsocketMessage::Register(
                    topic.to_string(),
                ))
                .await
                .unwrap();

            Ok(Json(json!({ "publicKey": public_key })).into_response())
        }
        None => Ok(Json( json! ( 
            {
                "reason": "Unauthorized. Please make sure to include project secret in Authorization header. "
            })
        )
        .into_response()),
    }
}
