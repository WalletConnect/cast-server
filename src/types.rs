use {
    serde::{Deserialize, Serialize},
    std::env,
};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RegisterBody {
    pub account: String,
    #[serde(default = "default_relay_url")]
    pub relay_url: String,
    pub sym_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientData {
    #[serde(rename = "_id")]
    pub id: String,
    pub relay_url: String,
    pub sym_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LookupEntry {
    #[serde(rename = "_id")]
    pub topic: String,
    pub project_id: String,
    pub account: String,
}

// TODO: Load this from env
fn default_relay_url() -> String {
    env::var("RELAY_URL").unwrap()
}

#[derive(Serialize)]
pub struct EnvelopeType0 {
    pub envelope_type: u8,
    pub iv: [u8; 12],
    pub sealbox: Vec<u8>,
}

#[derive(Debug)]
pub struct EnvelopeType1 {
    pub envelope_type: u8,
    pub pubkey: [u8; 32],
    pub iv: [u8; 12],
    pub sealbox: Vec<u8>,
}

pub trait Envelope {
    fn to_bytes(&self) -> Vec<u8>;
    fn from_bytes(bytes: Vec<u8>) -> Self;
}

impl Envelope for EnvelopeType0 {
    fn to_bytes(&self) -> Vec<u8> {
        let mut serialized = vec![];
        serialized.push(self.envelope_type);
        serialized.extend_from_slice(&self.iv);
        serialized.extend_from_slice(&self.sealbox);
        serialized
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            envelope_type: bytes[0],
            iv: bytes[1..13].try_into().unwrap(),
            sealbox: bytes[13..].to_vec(),
        }
    }
}

impl Envelope for EnvelopeType1 {
    fn to_bytes(&self) -> Vec<u8> {
        let mut serialized = vec![];
        serialized.push(self.envelope_type);
        serialized.extend_from_slice(&self.pubkey);
        serialized.extend_from_slice(&self.iv);
        serialized.extend_from_slice(&self.sealbox);
        serialized
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            envelope_type: bytes[0],
            pubkey: bytes[1..33].try_into().unwrap(),
            iv: bytes[33..45].try_into().unwrap(),
            sealbox: bytes[45..].to_vec(),
        }
    }
}
