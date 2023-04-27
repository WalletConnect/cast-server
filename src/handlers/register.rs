use {
    crate::{state::AppState, types::RegisterBody},
    axum::{
        extract::{Json, Path, State},
        http::StatusCode,
        response::IntoResponse,
    },
    opentelemetry::{Context, KeyValue},
    std::sync::Arc,
};
pub async fn handler(
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(data): Json<RegisterBody>,
) -> Result<axum::response::Response, crate::error::Error> {
    let url = url::Url::parse(&data.relay_url)?;
    state.register_client(&project_id, &data, &url).await?;

    #[cfg(test)]
    if url.scheme() != "wss" {
        return Ok((
            StatusCode::BAD_REQUEST,
            "Invalid procotol. Only \"wss://\" is accepted.",
        )
            .into_response());
    }

    if let Some(metrics) = &state.metrics {
        metrics
            .registered_clients
            .add(&Context::current(), 1, &[KeyValue::new(
                "project_id",
                project_id,
            )])
    }

    Ok((
        StatusCode::CREATED,
        format!("Successfully registered user {}", data.account),
    )
        .into_response())
}
