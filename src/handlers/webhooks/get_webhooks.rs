use {
    super::WebhookConfig,
    crate::{error::Result, state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    futures::TryStreamExt,
    hyper::HeaderMap,
    log::info,
    mongodb::bson::doc,
    serde_json::json,
    std::{collections::HashMap, sync::Arc},
};

pub async fn handler(
    headers: HeaderMap,
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse> {
    match headers.get("Authorization") {
        Some(project_secret) => {
            if !state
                .registry
                .is_authenticated(&project_id, project_secret.to_str()?)
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
    info!("[{request_id}] Getting webhooks for project: {project_id}");

    let cursor = state
        .database
        .collection::<WebhookInfo>("webhooks")
        .find(doc! {"project_id": project_id}, None)
        .await?;

    let webhooks: HashMap<_, _> = cursor
        .into_stream()
        .map_ok(|webhook| {
            (webhook.id, WebhookConfig {
                url: webhook.url,
                events: webhook.events,
            })
        })
        .try_collect()
        .await?;

    Ok((axum::http::StatusCode::OK, Json(webhooks)).into_response())
}
