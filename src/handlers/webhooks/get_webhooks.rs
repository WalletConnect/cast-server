use {
    super::WebhookConfig,
    crate::{error::Result, state::AppState, types::WebhookInfo},
    axum::{
        extract::{Path, State},
        response::IntoResponse,
        Json,
    },
    futures::TryStreamExt,
    log::info,
    mongodb::bson::doc,
    std::{collections::HashMap, sync::Arc},
};

pub async fn handler(
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse> {
    info!("Getting webhooks for project: {}", project_id);

    let cursor = state
        .database
        .collection::<WebhookInfo>("webhooks")
        .find(doc! {"project_id": project_id}, None)
        .await?;

    let webhooks: HashMap<_, _> =
        cursor
            .try_collect()
            .await
            .map(|webhooks: Vec<WebhookInfo>| {
                webhooks
                    .into_iter()
                    .map(|webhook| {
                        let webhook_config = WebhookConfig {
                            url: webhook.url,
                            events: webhook.events,
                        };
                        (webhook.id, webhook_config)
                    })
                    .collect()
            })?;

    Ok((axum::http::StatusCode::OK, Json(webhooks)))
}
