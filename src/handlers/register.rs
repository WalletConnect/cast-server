use {
    crate::{
        auth::SubscriptionAuth,
        state::AppState,
        types::{ClientData, RegisterBody},
    },
    axum::{
        extract::{Json, Path, State},
        http::StatusCode,
        response::IntoResponse,
    },
    data_encoding::BASE64_NOPAD,
    opentelemetry::{Context, KeyValue},
    std::sync::Arc,
};
pub async fn handler(
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(data): Json<RegisterBody>,
) -> Result<axum::response::Response, crate::error::Error> {
    let url = url::Url::parse(&data.relay_url)?;
    #[cfg(test)]
    if url.scheme() != "wss" {
        return Ok((
            StatusCode::BAD_REQUEST,
            "Invalid procotol. Only \"wss://\" is accepted.",
        )
            .into_response());
    }

    let sub_auth: SubscriptionAuth = {
        let jwt = data.subscription_auth;

        let claims = jwt.split(".").collect::<Vec<&str>>()[1];

        let claims = BASE64_NOPAD.decode(claims.as_bytes())?;

        serde_json::from_slice(&claims)?
    };

    let register_data = ClientData {
        id: data.account.into(),
        relay_url: data.relay_url,
        sym_key: data.sym_key,
        scope: sub_auth.scp.split(" ").map(|s| s.into()).collect(),
    };

    state
        .register_client(&project_id, &register_data, &url)
        .await?;

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
        format!("Successfully registered user {}", register_data.id),
    )
        .into_response())
}
