use {
    crate::{
        analytics::message_info::MessageInfo,
        error,
        extractors::AuthedProjectId,
        jsonrpc::{JsonRpcParams, JsonRpcPayload},
        state::AppState,
        types::{ClientData, Envelope, EnvelopeType0, Notification},
    },
    axum::{
        extract::{ConnectInfo, State},
        http::StatusCode,
        response::IntoResponse,
        Json,
    },
    base64::Engine,
    error::Result,
    futures::FutureExt,
    log::warn,
    mongodb::bson::doc,
    relay_rpc::{
        domain::Topic,
        rpc::{msg_id::MsgId, Publish},
    },
    serde::{Deserialize, Serialize},
    std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration},
    tokio_stream::StreamExt,
    tracing::info,
    wc::metrics::otel::{Context, KeyValue},
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

struct PublishJob {
    account: String,
    topic: Topic,
    message: String,
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
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(cast_args): Json<NotifyBody>,
) -> Result<axum::response::Response> {
    // Request id for logs
    let request_id = uuid::Uuid::new_v4();
    let timer = std::time::Instant::now();

    let mut response = Response {
        sent: HashSet::new(),
        failed: HashSet::new(),
        not_found: HashSet::new(),
    };

    let NotifyBody {
        notification,
        accounts,
    } = cast_args;

    // We assume all accounts were not found untill found
    accounts.iter().for_each(|account| {
        response.not_found.insert(account.clone());
    });

    // Get the accounts
    let cursor = state
        .database
        .collection::<ClientData>(&project_id)
        .find(doc! { "_id": {"$in": &accounts}}, None)
        .await?;

    // Generate publish jobs - this will also remove accounts from not_found
    // Prepares the encrypted message and gets the topic for each account
    let jobs = generate_publish_jobs(notification, cursor, &mut response).await?;

    // Attempts to send to all found accounts, waiting for relay ack for
    // NOTIFY_TIMEOUT seconds
    process_publish_jobs(
        jobs,
        state.http_relay_client.clone(),
        &mut response,
        request_id,
        &addr,
        &state,
        &project_id,
    )
    .await?;

    info!("[{request_id}] Response: {response:?} for notify from project: {project_id}");

    if let Some(metrics) = &state.metrics {
        send_metrics(metrics, &response, project_id, timer);
    }

    Ok((StatusCode::OK, Json(response)).into_response())
}

const NOTIFY_TIMEOUT: u64 = 45;

async fn process_publish_jobs(
    jobs: Vec<PublishJob>,
    client: Arc<relay_client::http::Client>,
    response: &mut Response,
    request_id: uuid::Uuid,
    addr: &SocketAddr,
    state: &Arc<AppState>,
    project_id: &str,
) -> Result<()> {
    let timer = std::time::Instant::now();
    let futures = jobs.into_iter().map(|job| {
        let remaining_time = timer.elapsed();
        let timeout_duration = Duration::from_secs(NOTIFY_TIMEOUT) - remaining_time;

        {
            let (country, continent, region) = state
                .analytics
                .geoip
                .lookup_geo_data(addr.ip())
                .map_or((None, None, None), |geo| {
                    (geo.country, geo.continent, geo.region)
                });

            let msg_id = Publish {
                topic: job.topic.clone(),
                message: job.message.clone().into(),
                ttl_secs: 86400,
                tag: 4002,
                prompt: true,
            }
            .msg_id();

            info!(
                "[{request_id}] Sending notification for {account} on topic: {topic} with {msg_id}",
                topic = job.topic,
                account = job.account,
                msg_id = msg_id
            );

            state.analytics.message(MessageInfo {
                region: region.map(|r| Arc::from(r.join(", "))),
                country,
                continent,
                project_id: project_id.into(),
                msg_id: msg_id.into(),
                topic: job.topic.to_string().into(),
                account: job.account.clone().into(),
                sent_at: gorgon::time::now(),
            })
        };

        tokio::time::timeout(
            timeout_duration,
            client.publish(
                job.topic.clone(),
                job.message,
                4002,
                Duration::from_secs(2592000),
                true,
            ),
        )
        .map(|result| match result {
            Ok(_) => Ok((job.account, job.topic)),
            Err(e) => Err((e, job.account)),
        })
    });

    let results = futures::future::join_all(futures).await;

    for result in results {
        match result {
            Ok((account, topic)) => {
                response.sent.insert(account.to_string());
                info!(
                    "[{request_id}] Successfully sent notification to {account} on topic: {topic}",
                );
            }
            Err(e) => {
                warn!(
                    "[{request_id}] Error sending notification: {} for {}",
                    e.0, e.1
                );
                response.failed.insert(SendFailure {
                    account: e.1.to_string(),
                    reason: "[{request_id}] Timed out while waiting for acknowledgement".into(),
                });
            }
        }
    }

    Ok(())
}

async fn generate_publish_jobs(
    notification: Notification,
    mut cursor: mongodb::Cursor<ClientData>,
    response: &mut Response,
) -> Result<Vec<PublishJob>> {
    let mut jobs = vec![];

    let id = chrono::Utc::now().timestamp_millis().unsigned_abs();

    let message = JsonRpcPayload {
        id,
        jsonrpc: "2.0".to_string(),
        params: JsonRpcParams::Push(notification.clone()),
    };

    while let Some(client_data) = cursor.try_next().await? {
        response.not_found.remove(&client_data.id);

        if !client_data.scope.contains(&notification.r#type) {
            response.failed.insert(SendFailure {
                account: client_data.id.clone(),
                reason: "Client is not subscribed to this topic".into(),
            });
            continue;
        }

        let envelope = Envelope::<EnvelopeType0>::new(&client_data.sym_key, &message)?;

        let base64_notification =
            base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

        let topic = Topic::new(sha256::digest(&*hex::decode(client_data.sym_key)?).into());

        jobs.push(PublishJob {
            topic,
            message: base64_notification,
            account: client_data.id,
        })
    }
    Ok(jobs)
}

fn send_metrics(
    metrics: &crate::metrics::Metrics,
    response: &Response,
    project_id: String,
    timer: std::time::Instant,
) {
    let ctx = Context::current();
    metrics
        .dispatched_notifications
        .add(&ctx, response.sent.len() as u64, &[
            KeyValue::new("type", "sent"),
            KeyValue::new("project_id", project_id.clone()),
        ]);

    metrics
        .dispatched_notifications
        .add(&ctx, response.failed.len() as u64, &[
            KeyValue::new("type", "failed"),
            KeyValue::new("project_id", project_id.clone()),
        ]);

    metrics
        .dispatched_notifications
        .add(&ctx, response.not_found.len() as u64, &[
            KeyValue::new("type", "not_found"),
            KeyValue::new("project_id", project_id.clone()),
        ]);

    metrics
        .send_latency
        .record(&ctx, timer.elapsed().as_millis().try_into().unwrap(), &[
            KeyValue::new("project_id", project_id),
        ])
}
