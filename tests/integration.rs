use {
    crate::context::encode_subscription_auth,
    base64::Engine,
    cast_server::{
        auth::{jwt_token, SubscriptionAuth},
        types::{Envelope, EnvelopeType0, EnvelopeType1},
        wsclient,
    },
    chacha20poly1305::{
        aead::{generic_array::GenericArray, AeadMut},
        consts::U12,
        ChaCha20Poly1305,
        KeyInit,
    },
    chrono::Utc,
    rand::{distributions::Uniform, prelude::Distribution, rngs::StdRng, Rng, SeedableRng},
    serde_json::json,
    sha2::Sha256,
    std::time::SystemTime,
    walletconnect_sdk::rpc::{
        auth::ed25519_dalek::Keypair,
        domain::{ClientId, DecodedClientId},
        rpc::{Params, Payload, Request, Subscription},
    },
    x25519_dalek::{PublicKey, StaticSecret},
};

mod context;

fn urls(env: String) -> (String, String) {
    match env.as_str() {
        "STAGING" => (
            "https://staging.cast.walletconnect.com".to_owned(),
            "wss://staging.relay.walletconnect.com".to_owned(),
        ),
        "PROD" => (
            "https://cast.walletconnect.com".to_owned(),
            "wss://relay.walletconnect.com".to_owned(),
        ),
        "LOCAL" => (
            "http://127.0.0.1:3000".to_owned(),
            "ws://127.0.0.1:8080".to_owned(),
        ),
        _ => panic!("Invalid environment"),
    }
}

#[tokio::test]
async fn cast_properly_sending_message() {
    let env = std::env::var("ENVIRONMENT").unwrap_or("STAGING".to_owned());
    let project_id = std::env::var("TEST_PROJECT_ID").expect(
        "Tests requires
PROJECT_ID to be set",
    );

    let (cast_url, relay_url) = urls(env);

    // Generate valid JWT
    let mut rng = StdRng::from_entropy();
    let keypair = Keypair::generate(&mut rng);
    let jwt = jwt_token(&relay_url, &keypair).unwrap();

    let seed: [u8; 32] = rng.gen();

    // let keypair = Keypair::generate(&mut seeded);
    let secret = StaticSecret::from(seed);
    let public = PublicKey::from(&secret);

    // Set up clients
    let http_client = reqwest::Client::new();
    let mut ws_client = wsclient::connect(&relay_url, &project_id, jwt)
        .await
        .unwrap();
    let project_secret = uuid::Uuid::new_v4().to_string();

    let dapp_pubkey_response: serde_json::Value = http_client
        .get(format!("{}/{}/subscribe-topic", &cast_url, &project_id))
        .bearer_auth(project_secret)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let dapp_pubkey = dapp_pubkey_response
        .get("publicKey")
        .unwrap()
        .as_str()
        .unwrap();
    dbg!(&dapp_pubkey);
    dbg!(&hex::encode(keypair.secret_key().to_bytes()));

    let subscribe_topic = sha256::digest(&*hex::decode(dapp_pubkey).unwrap());

    let decoded_client_id = DecodedClientId(*keypair.public_key().as_bytes());
    let client_id = ClientId::from(decoded_client_id);

    let subscription_auth = SubscriptionAuth {
        iat: Utc::now().timestamp() as u64,
        exp: Utc::now().timestamp() as u64 + 3600,
        iss: format!("did:key:{}", client_id),
        ksu: "https://keys.walletconnect.com".to_owned(),
        sub: "did:pkh:test_account".to_owned(),
        aud: "https://my-test-app.com".to_owned(),
        scp: "test test1".to_owned(),
        act: "push_subscription".to_owned(),
    };

    let subscription_auth = encode_subscription_auth(&subscription_auth, &keypair);

    let id = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let id = id * 1000 + rand::thread_rng().gen_range(100, 1000);

    let sub_auth = json!({ "subscriptionAuth": subscription_auth });
    let message = json!({"id": id,  "jsonrpc": "2.0", "params": sub_auth});

    let mut rng = rand_core::OsRng {};

    dbg!(&subscribe_topic);
    let response_topic_key = derive_key(dapp_pubkey.to_string(), hex::encode(secret.to_bytes()));
    dbg!(&response_topic_key);

    let mut cipher = ChaCha20Poly1305::new(GenericArray::from_slice(
        &hex::decode(response_topic_key.clone()).unwrap(),
    ));
    let uniform = Uniform::from(0u8..=255);
    let nonce: GenericArray<u8, U12> =
        GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

    let encrypted = cipher
        .encrypt(&nonce, message.to_string().as_bytes())
        .unwrap();
    dbg!(hex::encode(public.as_bytes()));

    let envelope = EnvelopeType1 {
        envelope_type: 1,
        pubkey: public.to_bytes(),
        iv: nonce.into(),
        sealbox: encrypted,
    };
    let message = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

    let response_topic = sha256::digest(&*hex::decode(response_topic_key.clone()).unwrap());
    dbg!(&response_topic);

    ws_client
        .publish_with_tag(&subscribe_topic, &message, 4006)
        .await
        .unwrap();

    ws_client.subscribe(&response_topic).await.unwrap();
    let resp = {
        ws_client.recv().await.unwrap();
        ws_client.recv().await.unwrap();
        ws_client.recv().await.unwrap()
    };
    dbg!(&resp);
    if let crate::Payload::Request(Request {
        params: Params::Subscription(Subscription { data, .. }),
        id,
        ..
    }) = resp
    {
        ws_client.send_ack(id).await.unwrap();

        let EnvelopeType0 { sealbox, iv, .. } = EnvelopeType0::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(data.message.as_bytes())
                .unwrap(),
        );

        let decrypted_response = cipher
            .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
            .unwrap();

        let response: serde_json::Value = serde_json::from_slice(&decrypted_response).unwrap();

        let pubkey = response
            .get("result")
            .unwrap()
            .get("publicKey")
            .unwrap()
            .as_str()
            .unwrap();

        let notify_key = derive_key(pubkey.to_string(), hex::encode(secret.to_bytes()));
        let notify_topic = sha256::digest(&*hex::decode(&notify_key).unwrap());

        ws_client.subscribe(&notify_topic).await.unwrap();
        ws_client.recv().await.unwrap();

        let notification = json!({
            "title": "string",
            "body": "string",
            "icon": "string",
            "url": "string",
            "type": "test"
        });

        let notify_body = json!({
            "notification": notification,
            "accounts": ["test_account"]
        });

        http_client
            .post(format!("{}/{}/notify", &cast_url, &project_id))
            .json(&notify_body)
            .send()
            .await
            .unwrap();

        if let crate::Payload::Request(Request {
            params: Params::Subscription(Subscription { data, .. }),
            ..
        }) = ws_client.recv().await.unwrap()
        {
            let mut cipher =
                ChaCha20Poly1305::new(GenericArray::from_slice(&hex::decode(notify_key).unwrap()));

            let EnvelopeType0 { iv, sealbox, .. } = EnvelopeType0::from_bytes(
                base64::engine::general_purpose::STANDARD
                    .decode(data.message.as_bytes())
                    .unwrap(),
            );

            let decrypted_notification: serde_json::Value = serde_json::from_slice(
                &cipher
                    .decrypt(&iv.into(), chacha20poly1305::aead::Payload::from(&*sealbox))
                    .unwrap(),
            )
            .unwrap();

            let received_notification = decrypted_notification.get("params").unwrap().to_owned();

            assert_eq!(received_notification, notification);

            let id = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            let id = id * 1000 + rand::thread_rng().gen_range(100, 1000);

            let delete_params = json!({
              "code": 400,
              "message": "test"
            });

            let delete_message = json! ({
                "id": id,
                "jsonrpc": "2.0",
                "params": base64::engine::general_purpose::STANDARD.encode(delete_params.to_string().as_bytes())
            });

            let nonce: GenericArray<u8, U12> =
                GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

            let encrypted_message = cipher
                .encrypt(
                    &nonce,
                    chacha20poly1305::aead::Payload::from(delete_message.to_string().as_bytes()),
                )
                .unwrap();

            let envelope = EnvelopeType0 {
                envelope_type: 0,
                iv: nonce.into(),
                sealbox: encrypted_message,
            };

            let encoded_message =
                base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());

            ws_client
                .publish_with_tag(&notify_topic, &encoded_message, 4004)
                .await
                .unwrap();
        }
    } else {
        panic!("wrong response")
    }
}

fn derive_key(pubkey: String, privkey: String) -> String {
    let pubkey: [u8; 32] = hex::decode(pubkey).unwrap()[..32].try_into().unwrap();

    let privkey: [u8; 32] = hex::decode(privkey).unwrap()[..32].try_into().unwrap();

    let secret_key = x25519_dalek::StaticSecret::from(privkey);

    let public_key = x25519_dalek::PublicKey::from(pubkey);

    let shared_key = secret_key.diffie_hellman(&public_key);

    let derived_key = hkdf::Hkdf::<Sha256>::new(None, shared_key.as_bytes());

    let mut expanded_key = [0u8; 32];

    derived_key.expand(b"", &mut expanded_key).unwrap();

    hex::encode(expanded_key)
}
