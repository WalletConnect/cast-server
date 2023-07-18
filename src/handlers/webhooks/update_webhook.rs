use {
    super::WebhookConfig,
    crate::{error::Result, state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    hyper::HeaderMap,
    log::info,
    mongodb::{bson, bson::doc},
    serde_json::json,
    std::sync::Arc,
    uuid::Uuid,
};

pub async fn handler(
    headers: HeaderMap,
    Path((project_id, webhook_id)): Path<(String, Uuid)>,
    State(state): State<Arc<AppState>>,
    Json(webhook_info): Json<WebhookConfig>,
) -> Result<impl IntoResponse> {
    match headers.get("Authorization") {
        Some(project_secret) => {
            if !state
                .registry
                .is_authenticated(&project_id, project_secret.to_str().unwrap())
                .await?
            {
                return Ok(Json(json!({
                    "reason": "Unauthorized. Please make sure to include project secret in Authorization header. "
                })).into_response());
            };
        }
        None => {
            return Ok(Json(json!({
                "reason": "Unauthorized. Please make sure to include project secret in Authorization header. "
            })).into_response());
        }
    };

    let request_id = uuid::Uuid::new_v4();
    info!("[{request_id}] Updating webhook: {webhook_id} for project: {project_id}");
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
