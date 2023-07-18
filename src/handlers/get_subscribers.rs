use {
    crate::{error::Result, state::AppState, types::ClientData},
    axum::{
        extract::{Path, State},
        http::StatusCode,
        response::IntoResponse,
        Json,
    },
    futures::TryStreamExt,
    hyper::HeaderMap,
    log::info,
    serde_json::json,
    std::sync::Arc,
};

pub async fn handler(
    headers: HeaderMap,
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response> {
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

    info!("Getting subscribers for project: {}", project_id);

    let mut cursor = state
        .database
        .collection::<ClientData>(&project_id)
        .find(None, None)
        .await?;

    let mut result = vec![];

    while let Some(client) = cursor.try_next().await? {
        result.push(client.id);
    }

    Ok((StatusCode::OK, Json(result)).into_response())
}
