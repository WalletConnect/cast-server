use {
    base64::Engine,
    cast_server::auth::SubscriptionAuth,
    ed25519_dalek::Signer,
    walletconnect_sdk::rpc::auth::{
        ed25519_dalek::Keypair,
        JwtHeader,
        JWT_HEADER_ALG,
        JWT_HEADER_TYP,
    },
};

pub mod health;
pub mod notify;
pub mod register;

pub fn encode_subscription_auth(subscription_auth: &SubscriptionAuth, keypair: &Keypair) -> String {
    let data = JwtHeader {
        typ: JWT_HEADER_TYP,
        alg: JWT_HEADER_ALG,
    };

    let header = serde_json::to_string(&data).unwrap();

    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header);

    let claims = {
        let json = serde_json::to_string(subscription_auth).unwrap();
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(json)
    };

    let message = format!("{header}.{claims}");

    let signature =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(keypair.sign(message.as_bytes()));

    format!("{message}.{signature}")
}
