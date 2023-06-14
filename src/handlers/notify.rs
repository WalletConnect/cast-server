use {
    crate::{
        error,
        jsonrpc::{JsonRpcParams, JsonRpcPayload},
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, Notification},
        wsclient,
    },
    axum::{
        extract::{ConnectInfo, Path, State},
        http::StatusCode,
        response::IntoResponse,
        Json,
    },
    base64::Engine,
    mongodb::bson::doc,
    opentelemetry::{Context, KeyValue},
    serde::{Deserialize, Serialize},
    std::{
        collections::{HashMap, HashSet},
        net::SocketAddr,
        sync::Arc,
        time::Duration,
    },
    tokio_stream::StreamExt,
    tracing::info,
    walletconnect_sdk::rpc::domain::Topic,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotifyBody {
    pub notification: Notification,
    pub accounts: Vec<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
pub struct SendFailure {
    pub account: String,
    pub reason: String,
}

// Change String to Account
// Change String to Error
#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub sent: HashSet<String>,
    pub failed: HashSet<SendFailure>,
    pub not_found: HashSet<String>,
}

pub async fn handler(
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    Path(project_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(cast_args): Json<NotifyBody>,
) -> Result<axum::response::Response, error::Error> {
    // Request id for logs
    let uuid = uuid::Uuid::new_v4().to_string();

    let timer = std::time::Instant::now();
    let db = state.database.clone();
    let NotifyBody {
        notification,
        accounts,
    } = cast_args;
    let mut confirmed_sends = HashSet::new();
    let mut failed_sends: HashSet<SendFailure> = HashSet::new();

    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

    let mut cursor = db
        .collection::<ClientData>(&project_id)
        .find(doc! { "_id": {"$in": &accounts}}, None)
        .await?;

    let mut not_found: HashSet<String> = accounts.into_iter().collect();

    let message = JsonRpcPayload {
        id,
        jsonrpc: "2.0".to_string(),
        params: JsonRpcParams::Push(notification.clone()),
    };

    let mut messages: Vec<(Topic, String)> = vec![];
    let mut mapping: HashMap<Topic, String> = HashMap::new();

    while let Some(client_data) = cursor.try_next().await? {
        not_found.remove(&client_data.id);

        if !client_data.scope.contains(&notification.r#type) {
            failed_sends.insert(SendFailure {
                account: client_data.id.clone(),
                reason: "Client is not subscribed to this topic".into(),
            });
            continue;
        }

        confirmed_sends.insert(client_data.id.clone());

        let envelope = Envelope::<EnvelopeType0>::new(&client_data.sym_key, &message)?;

        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        let topic = Topic::new(sha256::digest(&*hex::decode(client_data.sym_key)?).into());

        state
            .wsclient
            .publish(
                topic.into(),
                base64_notification.clone(),
                4002,
                Duration::from_secs(86400),
            )
            .await?;
        // mapping.insert(topic.clone(), client_data.id.clone());
        // messages.push((topic, base64_notification));
    }

    // let unacked = client
    //     .batch_publish_with_tag(messages, 4002)
    //     .await?
    //     .into_iter()
    //     .filter_map(|topic| mapping.get(&topic))
    //     .cloned()
    //     .collect::<HashSet<String>>();

    // if !unacked.is_empty() {
    //     info!("Unacked messages: {:?} for request {}", unacked, uuid);
    // }

    // for account in unacked {
    //     confirmed_sends.remove(&account);
    //     failed_sends.insert(SendFailure {
    //         account,
    //         reason: "Relay never acknowledged the message".into(),
    //     });
    // }

    if let Some(metrics) = &state.metrics {
        let ctx = Context::current();
        metrics
            .dispatched_notifications
            .add(&ctx, confirmed_sends.len() as u64, &[
                KeyValue::new("type", "sent"),
                KeyValue::new("project_id", project_id.clone()),
            ]);

        metrics
            .dispatched_notifications
            .add(&ctx, failed_sends.len() as u64, &[
                KeyValue::new("type", "failed"),
                KeyValue::new("project_id", project_id.clone()),
            ]);

        metrics
            .dispatched_notifications
            .add(&ctx, not_found.len() as u64, &[
                KeyValue::new("type", "not_found"),
                KeyValue::new("project_id", project_id.clone()),
            ]);

        metrics
            .send_latency
            .record(&ctx, timer.elapsed().as_millis().try_into().unwrap(), &[
                KeyValue::new("project_id", project_id.clone()),
            ])
    }

    // Get them into one struct and serialize as json
    let response = Response {
        sent: confirmed_sends,
        failed: failed_sends,
        not_found,
    };

    info!(
        "Response: {:?} for notify from project: {} for request: {}",
        response, project_id, uuid
    );

    Ok((StatusCode::OK, Json(response)).into_response())
}
