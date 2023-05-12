use {
    super::WebhookConfig,
    crate::{handlers::webhooks::validate_url, state::AppState, types::WebhookInfo},
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
) -> std::result::Result<axum::response::Response, crate::error::Error> {
    info!("Registering webhook for project: {}", project_id);
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

    info!(
        "Webhook registered: {} for project:{}",
        webhook_id, project_id
    );

    Ok((
        axum::http::StatusCode::CREATED,
        Json(RegisterWebhookResponse { id: webhook_id }),
    )
        .into_response())
}
