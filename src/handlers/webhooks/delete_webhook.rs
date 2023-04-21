use {
    crate::{state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
    },
    log::info,
    mongodb::bson::doc,
    std::sync::Arc,
    uuid::Uuid,
};

pub async fn handler(
    Path((project_id, webhook_id)): Path<(String, Uuid)>,
    State(state): State<Arc<AppState>>,
) -> std::result::Result<axum::response::Response, crate::error::Error> {
    info!(
        "Deleting webhook: {} for project: {}",
        webhook_id, project_id
    );

    state
        .database
        .collection::<WebhookInfo>("webhooks")
        .delete_one(
            doc! {"project_id": project_id, "id": webhook_id.to_string()},
            None,
        )
        .await?;

    Ok((axum::http::StatusCode::OK).into_response())
}
