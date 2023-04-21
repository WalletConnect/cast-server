use {
    super::WebhookConfig,
    crate::{
        state::{AppState},
        types::WebhookInfo,
    },
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    futures::{TryStreamExt},
    mongodb::{bson::doc},
    std::{collections::HashMap, sync::Arc},
};

pub async fn handler(
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> std::result::Result<axum::response::Response, crate::error::Error> {
    let mut result = HashMap::new();

    let mut cursor = state
        .database
        .collection::<WebhookInfo>("webhooks")
        .find(doc! {"project_id": project_id}, None)
        .await?;

    // Iterate over cursor adding webhook id:webhookconfig pairs to hashmap
    while let Some(webhook) = cursor.try_next().await? {
        let webhook_config = WebhookConfig {
            url: webhook.url,
            events: webhook.events,
        };
        result.insert(webhook.id, webhook_config);
    }

    Ok((axum::http::StatusCode::OK, Json(result)).into_response())
}
