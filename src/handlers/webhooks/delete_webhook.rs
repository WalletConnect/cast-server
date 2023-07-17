use {
    crate::{error::Result, state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    hyper::HeaderMap,
    log::info,
    mongodb::bson::doc,
    serde_json::json,
    std::sync::Arc,
    uuid::Uuid,
};

pub async fn handler(
    headers: HeaderMap,
    Path((project_id, webhook_id)): Path<(String, Uuid)>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse> {
    match headers.get("Authorization") {
        Some(project_secret) => {
            if !state
                .registry
                .is_authenticated(&project_id, &project_secret.to_str().unwrap())
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
    info!("[{request_id}] Deleting webhook: {webhook_id} for project: {project_id}");

    state
        .database
        .collection::<WebhookInfo>("webhooks")
        .delete_one(
            doc! {"project_id": project_id, "id": webhook_id.to_string()},
            None,
        )
        .await?;

    Ok(axum::http::StatusCode::OK.into_response())
}
