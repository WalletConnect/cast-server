use {
    super::WebhookConfig,
    crate::{state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    log::info,
    mongodb::{bson, bson::doc},
    std::sync::Arc,
    uuid::Uuid,
};

pub async fn handler(
    Path((project_id, webhook_id)): Path<(String, Uuid)>,
    State(state): State<Arc<AppState>>,
    Json(webhook_info): Json<WebhookConfig>,
) -> std::result::Result<axum::response::Response, crate::error::Error> {
    info!(
        "Updating webhook: {} for project: {}",
        webhook_id, project_id
    );
    state
        .database
        .collection::<WebhookInfo>("webhooks")
        .update_one(
            doc! {"project_id": project_id, "id": webhook_id.to_string()},
            doc! {"$set": {"url": webhook_info.url, "events": bson::to_bson(&webhook_info.events)? } },
            None,
        )
        .await?;

    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}
