pub use walletconnect_sdk::rpc::domain::MessageId;
use {
    crate::error::Result,
    std::time::Duration,
    tokio::sync::mpsc,
    tracing::{info, warn},
    tungstenite::protocol::CloseFrame,
    walletconnect_sdk::{
        client::{websocket::ConnectionHandler, ConnectionOptions},
        rpc::{
            auth::{ed25519_dalek::Keypair, AuthToken},
            user_agent::ValidUserAgent,
        },
    },
};

pub struct RelayConnectionHandler {
    name: &'static str,
    tx: mpsc::UnboundedSender<RelayClientEvent>,
}

pub enum RelayClientEvent {
    Message(walletconnect_sdk::client::websocket::PublishedMessage),
    Error(walletconnect_sdk::client::error::Error),
    Disconnected(Option<CloseFrame<'static>>),
    Connected,
}

impl RelayConnectionHandler {
    pub fn new(name: &'static str, tx: mpsc::UnboundedSender<RelayClientEvent>) -> Self {
        Self { name, tx }
    }
}

impl ConnectionHandler for RelayConnectionHandler {
    fn connected(&mut self) {
        info!("[{}]connection open", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Connected) {
            warn!("[{}] failed to emit the connection event: {}", self.name, e);
        }
    }

    fn disconnected(&mut self, frame: Option<CloseFrame<'static>>) {
        info!("[{}] connection closed: frame={frame:?}", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Disconnected(frame)) {
            warn!(
                "[{}] failed to emit the disconnection event: {}",
                self.name, e
            );
        }
    }

    fn message_received(
        &mut self,
        message: walletconnect_sdk::client::websocket::PublishedMessage,
    ) {
        info!(
            "[{}] inbound message: topic={} message={}",
            self.name, message.topic, message.message
        );
        if let Err(e) = self.tx.send(RelayClientEvent::Message(message)) {
            warn!("[{}] failed to emit the message event: {}", self.name, e);
        }
    }

    fn inbound_error(&mut self, error: walletconnect_sdk::client::error::Error) {
        info!("[{}] inbound error: {error}", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Error(error)) {
            warn!(
                "[{}] failed to emit the inbound error event: {}",
                self.name, e
            );
        }
    }

    fn outbound_error(&mut self, error: walletconnect_sdk::client::error::Error) {
        info!("[{}] outbound error: {error}", self.name);
        if let Err(e) = self.tx.send(RelayClientEvent::Error(error)) {
            warn!(
                "[{}] failed to emit the outbound error event: {}",
                self.name, e
            );
        }
    }
}

pub fn create_connection_opts(
    relay_url: &str,
    project_id: &str,
    keypair: &Keypair,
    cast_url: &str,
) -> Result<ConnectionOptions> {
    let auth = AuthToken::new(cast_url)
        .aud(relay_url)
        .ttl(Duration::from_secs(60 * 60))
        .as_jwt(&keypair)?;

    let ua = ValidUserAgent {
        protocol: walletconnect_sdk::rpc::user_agent::Protocol {
            kind: walletconnect_sdk::rpc::user_agent::ProtocolKind::WalletConnect,
            version: 2,
        },
        sdk: walletconnect_sdk::rpc::user_agent::Sdk {
            language: walletconnect_sdk::rpc::user_agent::SdkLanguage::Rust,
            // TODO: proper version
            version: "1.0".to_string(),
        },
        os: walletconnect_sdk::rpc::user_agent::OsInfo {
            os_family: "ECS".into(),
            ua_family: None,
            version: None,
        },
        id: Some(walletconnect_sdk::rpc::user_agent::Id {
            environment: walletconnect_sdk::rpc::user_agent::Environment::Unknown(
                "Notify Server".into(),
            ),
            host: Some(cast_url.into()),
        }),
    };
    let user_agent = walletconnect_sdk::rpc::user_agent::UserAgent::ValidUserAgent(ua);

    let opts = ConnectionOptions::new(project_id, auth).with_user_agent(user_agent);
    Ok(opts)
}
