use {
    super::WebhookConfig,
    crate::{error::Result, handlers::webhooks::validate_url, state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    log::info,
    mongodb::bson::doc,
    serde::Serialize,
    std::sync::Arc,
    uuid::Uuid,
};

#[derive(Serialize)]
struct RegisterWebhookResponse {
    id: String,
}

pub async fn handler(
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(webhook_info): Json<WebhookConfig>,
) -> Result<impl IntoResponse> {
    let uuid = uuid::Uuid::new_v4();
    info!("[{uuid}] Registering webhook for project: {project_id}");
    let webhook_id = Uuid::new_v4().to_string();

    validate_url(&webhook_info.url)?;

    let webhook = WebhookInfo {
        id: webhook_id.clone(),
        url: webhook_info.url,
        events: webhook_info.events,
        project_id: project_id.clone(),
    };

    state
        .database
        .collection("webhooks")
        .insert_one(webhook, None)
        .await?;

    info!("[{uuid}] Webhook registered: {webhook_id} for project:{project_id}");

    Ok((
        axum::http::StatusCode::CREATED,
        Json(RegisterWebhookResponse { id: webhook_id }),
    ))
}
