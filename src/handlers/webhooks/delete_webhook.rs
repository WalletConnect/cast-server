use {
    crate::{
        error::Result,
        state::{AppState, WebhookNotificationEvent},
        types::WebhookInfo,
    },
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    futures::{StreamExt, TryStreamExt},
    log::info,
    mongodb::{bson::doc, Cursor},
    serde::{Deserialize, Serialize},
    std::{collections::HashMap, sync::Arc},
    uuid::Uuid,
};

pub async fn handler(
    Path(project_id): Path<String>,
    Path(webhook_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
) -> std::result::Result<axum::response::Response, crate::error::Error> {
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
