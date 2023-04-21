use {
    crate::state::WebhookNotificationEvent,
    serde::{Deserialize, Serialize},
};

pub mod delete_webhook;
pub mod get_webhooks;
pub mod register_webhook;

#[derive(Debug, Deserialize, Serialize)]
pub struct WebhookConfig {
    url: String,
    events: Vec<WebhookNotificationEvent>,
}
