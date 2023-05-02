use {
    crate::error::Result,
    serde::{Deserialize, Serialize},
    walletconnect_sdk::rpc::{
        auth::{ed25519_dalek::Keypair, AuthToken},
        domain::{ClientId, DecodedClientId},
    },
};

pub fn jwt_token(url: &str, keypair: &Keypair) -> Result<String> {
    let decoded_client_id = DecodedClientId(*keypair.public_key().as_bytes());
    let client_id = ClientId::from(decoded_client_id);

    AuthToken::new(client_id.value().clone())
        .aud(url)
        .as_jwt(keypair)
        .map(|x| x.to_string())
        .map_err(|e| e.into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionAuth {
    // iat - timestamp when jwt was issued
    pub iat: u64,
    // exp - timestamp when jwt must expire
    pub exp: u64,
    // iss - did:key of an identity key. Enables to resolve attached blockchain
    // account.4,
    pub iss: String,
    // ksu - key server for identity key verification
    pub ksu: String,
    // aud - dapp's url
    pub aud: String,
    // sub - blockchain account that push subscription has been proposed for
    // (did:pkh)
    pub sub: String,
    // act - description of action intent. Must be equal to "push_subscription"
    #[serde(default = "default_act")]
    pub act: String,
    // scp - scope of notification types authorized by the user
    #[serde(default = "default_scope")]
    pub scp: String,
}

fn default_scope() -> String {
    "v1".to_string()
}

fn default_act() -> String {
    "push_subscription".to_string()
}
