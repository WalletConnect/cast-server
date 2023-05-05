use chrono::{Utc,  DateTime};
use walletconnect_sdk::rpc::auth::cacao::Cacao;

use {
    crate::error::Result,
    base64::Engine,
    serde::{Deserialize, Serialize},
    walletconnect_sdk::rpc::{
        auth::{
            ed25519_dalek::Keypair,
            AuthToken,
            DID_DELIMITER,
            DID_METHOD,
            DID_PREFIX,
            JWT_HEADER_ALG,
        },
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

impl SubscriptionAuth {
    pub fn from_jwt(jwt: &str) -> Result<Self> {
        let mut parts = jwt.splitn(3, '.');
        let (Some(header), Some(claims)) = (parts.next(), parts.next()) else {
            return Err(AuthError::Format)?;
        };

        let header = base64::engine::general_purpose::STANDARD_NO_PAD.decode(header)?;
        let header = serde_json::from_slice::<JwtHeader>(&header)?;

        if header.alg != JWT_HEADER_ALG {
            return Err(AuthError::Algorithm)?;
        }

        let claims = base64::engine::general_purpose::STANDARD_NO_PAD.decode(claims)?;
        let claims = serde_json::from_slice::<SubscriptionAuth>(&claims)?;


        if claims.exp < Utc::now().timestamp() as u64 {
            return Err(AuthError::Expired)?;
        }

        if claims.iat > Utc::now().timestamp_millis() as u64 {
            return Err(AuthError::NotYetValid)?;
        }

        if claims.act != "push_subscription" {
            return Err(AuthError::InvalidAct)?;
        }

        let mut parts = jwt.rsplitn(2, ".");

        let (Some(signature), Some(message)) = (parts.next(), parts.next()) else { 
            return Err(AuthError::Format)?;
        };

        let did_key = claims
            .iss
            .strip_prefix(DID_PREFIX)
            .ok_or(AuthError::IssuerPrefix)?
            .strip_prefix(DID_DELIMITER)
            .ok_or(AuthError::IssuerFormat)?
            .strip_prefix(DID_METHOD)
            .ok_or(AuthError::IssuerMethod)?
            .strip_prefix(DID_DELIMITER)
            .ok_or(AuthError::IssuerFormat)?;

        let pub_key = did_key.parse::<DecodedClientId>()?;

        let key = jsonwebtoken::DecodingKey::from_ed_der(pub_key.as_ref());

        // Finally, verify signature.
        let sig_result = jsonwebtoken::crypto::verify(
            signature,
            message.as_bytes(),
            &key,
            jsonwebtoken::Algorithm::EdDSA,
        );

        match sig_result {
            Ok(true) => Ok(claims),
            Ok(false) | Err(_) => Err(AuthError::InvalidSignature)?,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JwtHeader {
    typ: String,
    alg: String,
}

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("Invalid format")]
    Format,

    #[error("Invalid header")]
    InvalidHeader,

    #[error("Invalid issuer prefix")]
    IssuerPrefix,

    #[error("Invalid issuer format")]
    IssuerFormat,

    #[error("Invalid issuer method")]
    IssuerMethod,

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid algorithm")]
    Algorithm,

    #[error("Failed to verify with keyserver")]
    CacaoValidation,

    #[error("Keyserver account mismatch")]
    CacaoAccountMismatch,

    #[error("Expired")]
    Expired,

    #[error("Not yet valid")]
    NotYetValid,

    #[error("Invalid act")]
    InvalidAct,
   
}

#[tokio::test]
async fn test() {
    let jwt =
"eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtlWnhMV2lDQmtqODFDcDVqcVlIS28yMm84YUQ5RHZDc0dSRWg3b3lCQXV2ZyIsInN1YiI6ImRpZDpwa2g6ZWlwMTU1OjE6MHhmNGY4OWI1Y2I5MzEwOWY1YWM4ZGQyZTFkZjYxYWQ2MTgwYTA0ZGMyIiwiYXVkIjoiaHR0cHM6Ly9nbS53YWxsZXRjb25uZWN0LmNvbSIsImlhdCI6MTY4MzA0Njk1NDkyMiwiZXhwIjoxNjg1NjM4OTU0OTIyLCJrc3UiOiJodHRwczovL2tleXMud2FsbGV0Y29ubmVjdC5jb20ifQ.-DKfJ_GXt1nT_9ksRJCNx3P1mxO7Ey99QTJi7Cok38OBrTYjWppPF8oRZHUMeqxylnWyWghm7SIj0120hvooAw";

    let auth = SubscriptionAuth::from_jwt(jwt).unwrap();

    let verified = verify_identity(&auth.iss.strip_prefix("did:key:").unwrap(), &auth.ksu, &auth.sub).await;
}

pub async fn verify_identity(pubkey: &str, keyserver: &str, account: &str) -> Result<()> {
    let url = format!("{}/identity?publicKey={}", keyserver, pubkey);
    let res = reqwest::get(&url).await?;
    let cacao: KeyServerResponse = res.json().await?;

    if cacao.value.is_none() {
        return Err(AuthError::CacaoValidation)?;
    } 

    let cacao = cacao.value.unwrap().cacao;

    if cacao.p.iss != account {
        return Err(AuthError::CacaoAccountMismatch)?
    }

    if let Some(exp) = cacao.p.exp {
        let exp = DateTime::parse_from_rfc3339(&exp)?;

        if exp.timestamp() < Utc::now().timestamp() {
            return Err(AuthError::CacaoValidation)?
        }
    }

   Ok(()) 
}


#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyServerResponse {
    status: String,
    error: Option<String>,
    value: Option<CacaoValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacaoValue {
    cacao: Cacao,
}
