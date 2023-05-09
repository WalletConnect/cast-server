use {
    crate::context::encode_subscription_auth,
    base64::Engine,
    cast_server::{
        auth::{jwt_token, SubscriptionAuth},
        handlers::notify::NotifyBody,
        jsonrpc::{JsonRpcParams, JsonRpcPayload, Notification, PublishParams},
        types::{Envelope, EnvelopeType0, EnvelopeType1, RegisterBody},
        wsclient::{self, new_rpc_request},
    },
    chacha20poly1305::{
        aead::{generic_array::GenericArray, AeadMut, OsRng},
        consts::U12,
        ChaCha20Poly1305,
        KeyInit,
    },
    chrono::Utc,
    parquet::data_type::AsBytes,
    rand::{distributions::Uniform, prelude::Distribution, rngs::StdRng, Rng, SeedableRng},
    serde_json::json,
    sha2::Sha256,
    std::{
        collections::HashSet,
        time::{Duration, SystemTime},
    },
    tokio::time::sleep,
    walletconnect_sdk::rpc::{
        auth::ed25519_dalek::Keypair,
        domain::{ClientId, DecodedClientId},
        rpc::{Params, Payload, Publish, Request, Subscribe, Subscription},
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
            "wss://staging.relay.walletconnect.com".to_owned(),
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

        let dapp_pubkey_response: serde_json::Value = http_client
            .post(format!("{}/{}/subscribe-topic", &cast_url, &project_id))
            .bearer_auth(project_secret)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        if let crate::Payload::Request(Request {
            params: Params::Subscription(Subscription { data, .. }),
            ..
        }) = ws_client.recv().await.unwrap()
        {
            dbg!(data.message);
        }
    } else {
        panic!("wrong response")
    }
    //     // Prepare client key
    //     let key =
    //         chacha20poly1305::ChaCha20Poly1305::generate_key(&mut
    // chacha20poly1305::aead::OsRng {});     let topic =
    // sha256::digest(&*key);     let hex_key = hex::encode(key);

    //     let test_account = "test_account_send_test".to_owned();

    //     // Create valid account
    //     let scope: HashSet<String> =
    // std::iter::once("test".into()).collect();

    //     let body = RegisterBody {
    //         account: test_account.clone(),
    //         relay_url,
    //         sym_key: hex_key,
    //         scope,
    //     };

    //     // Register valid account
    //     let status = http_client
    //         .post(format!("{}/{}/register", &cast_url, &project_id))
    //         .body(serde_json::to_string(&body).unwrap())
    //         .header("Content-Type", "application/json")
    //         .send()
    //         .await
    //         .expect("Failed to call /register")
    //         .status();
    //     assert!(status.is_success());

    //     // Prepare notification
    //     let test_notification = Notification {
    //         title: "test".to_owned(),
    //         body: "test".to_owned(),
    //         icon: "test".to_owned(),
    //         url: "test".to_owned(),
    //         notification_type: "test".to_owned(),
    //     };

    //     // Prepare notify body
    //     let body = NotifyBody {
    //         notification: test_notification.clone(),
    //         accounts: vec![test_account.clone()],
    //     };

    //     // Subscribe client to topic
    //     ws_client.subscribe(&topic).await.unwrap();

    //     // Receive ack to subscribe
    //     ws_client.recv().await.unwrap();

    //     // Send notify
    //     let response = http_client
    //         .post(format!("{}/{}/notify", &cast_url, &project_id))
    //         .body(serde_json::to_string(&body).unwrap())
    //         .header("Content-Type", "application/json")
    //         .send()
    //         .await
    //         .expect("Failed to call /notify")
    //         .text()
    //         .await
    //         .unwrap();
    //     let response: cast_server::handlers::notify::Response =
    //         serde_json::from_str(&response).unwrap();

    //     assert_eq!(response.sent.into_iter().next().unwrap(), test_account);

    //     // Recv the response
    //     if let Payload::Request(request) = ws_client.recv().await.unwrap() {
    //         if let Params::Subscription(params) = request.params {
    //             assert_eq!(params.data.topic, topic.into());
    //             let mut cipher =
    //
    // chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&key));
    //             let encrypted_text =
    // base64::engine::general_purpose::STANDARD
    // .decode(&*params.data.message)                 .unwrap();
    //             let envelope = EnvelopeType0::from_bytes(encrypted_text);
    //             let decrypted = cipher
    //                 .decrypt(&envelope.iv.into(), &*envelope.sealbox)
    //                 .unwrap();

    //             let push_message: JsonRpcPayload =
    // serde_json::from_slice(&decrypted).unwrap();             if let
    // JsonRpcParams::Push(notification) = push_message.params {
    // assert_eq!(notification, test_notification);             } else {
    //                 panic!(
    //                     "Notification received not matching notification
    // sent"
    //                 )
    //             }
    //         } else {
    //             panic!("Wrong payload body received");
    //         }
    //     } else {
    //         panic!("Invalid data received");
    //     }
}

// #[tokio::test]
// async fn test_unregister() {
//     let env = std::env::var("ENVIRONMENT").unwrap_or("STAGING".to_owned());
//     let project_id = std::env::var("PROJECT_ID").expect(
//         "Tests requires
// PROJECT_ID to be set",
//     );

//     let (cast_url, relay_url) = urls(env);

//     let client = reqwest::Client::new();
//     let key =
//         chacha20poly1305::ChaCha20Poly1305::generate_key(&mut
// chacha20poly1305::aead::OsRng {});

//     let mut rng = StdRng::from_entropy();

//     let keypair = Keypair::generate(&mut rng);
//     let jwt = jwt_token(&relay_url, &keypair).unwrap();

//     let mut ws_client = wsclient::connect(&relay_url, &project_id,
// jwt.clone())         .await
//         .unwrap();

//     let hex_key = hex::encode(key.clone());

//     let test_account = "test_account_unregister".to_owned();

//     // Create valid account
//     let scope: HashSet<String> = std::iter::once("test".into()).collect();

//     let body = RegisterBody {
//         account: "test_account".to_owned(),
//         relay_url,
//         sym_key: hex_key.clone(),
//         scope,
//     };

//     // Register valid account
//     let status = client
//         .post(format!("{}/{}/register", &cast_url, &project_id))
//         .body(serde_json::to_string(&body).unwrap())
//         .header("Content-Type", "application/json")
//         .send()
//         .await
//         .expect("Failed to call /register");
//     assert!(status.status().is_success());

//     //  Send unregister on websocket
//     let topic = sha256::digest(key.as_slice());

//     let encryption_key = hex::decode(hex_key).unwrap();

//     let mut cipher =
//         chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&
// encryption_key));

//     let uniform = Uniform::from(0u8..=255);

//     let mut rng = StdRng::from_entropy();

//     let nonce: GenericArray<u8, U12> =
//         GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

//     let message = "{\"code\": 3,\"message\": \"Unregister reason\"}";

//     let encrypted = cipher.encrypt(&nonce, message.as_bytes()).unwrap();

//     let envelope = EnvelopeType0 {
//         envelope_type: 0,
//         iv: nonce.into(),
//         sealbox: encrypted,
//     };

//     let encrypted_msg = envelope.to_bytes();

//     let message =
// base64::engine::general_purpose::STANDARD.encode(encrypted_msg);
//     ws_client.subscribe(&topic).await.unwrap();
//     ws_client
//         .publish_with_tag(&topic, &message, 4004)
//         .await
//         .unwrap();

//     // Time for relay to process message
//     sleep(Duration::from_secs(5)).await;

//     // Prepare notification
//     let notification = Notification {
//         title: "test".to_owned(),
//         body: "test".to_owned(),
//         icon: "test".to_owned(),
//         url: "test".to_owned(),
//         notification_type: "test".to_owned(),
//     };

//     // Prepare notify body
//     let body = NotifyBody {
//         notification,
//         accounts: vec![test_account],
//     };

//     // Send notify
//     let response = client
//         .post(format!("{}/{}/notify", &cast_url, &project_id))
//         .body(serde_json::to_string(&body).unwrap())
//         .header("Content-Type", "application/json")
//         .send()
//         .await
//         .expect("Failed to call /notify")
//         .text()
//         .await
//         .unwrap();

//     let response: cast_server::handlers::notify::Response =
//         serde_json::from_str(&response).unwrap();

//     // Assert that account was deleted and therefore response is not sent
//     assert_eq!(response.sent.len(), 0);
// }

#[test]
fn jwt() {
    let mut rng = StdRng::from_entropy();

    let relay_url = "wss://staging.relay.walletconnect.com";
    let keypair = Keypair::generate(&mut rng);
    let jwt = jwt_token(&relay_url, &keypair).unwrap();
    dbg!(&jwt);
}

#[test]
fn create_envelope() {
    let seed: [u8; 32] = "project_secreasdoiasndioansiodnoainsdioansodit".as_bytes()[..32]
        .try_into()
        .unwrap();
    // let keypair = Keypair::generate(&mut seeded);
    let secret = StaticSecret::from(seed);
    let public = PublicKey::from(&secret);

    let proj_pub_key = "1dfac48297bfd21e11f52c3c7c4b676cecb5233e47b8558503c462012eed355d";
    let sym_key = derive_key(proj_pub_key.to_string(), hex::encode(secret));
    dbg!(&sym_key);
    let encryption_key = hex::decode(sym_key).unwrap();

    // let topic = sha256::digest(encryption_key.as_slice());
    let topic_proj = sha256::digest(hex::decode(proj_pub_key).unwrap().as_slice());
    dbg!(topic_proj);

    let topic_client = sha256::digest(encryption_key.as_bytes());
    dbg!(topic_client);

    let mut cipher =
        chacha20poly1305::ChaCha20Poly1305::new(GenericArray::from_slice(&encryption_key));

    let uniform = Uniform::from(0u8..=255);

    let mut rng = StdRng::from_entropy();

    let nonce: GenericArray<u8, U12> =
        GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

    // let message = "{\"code\": 3,\"message\": \"Unregister reason\"}";
    let sub_auth = SubscriptionAuth {
        iat: 1,
        exp: 3,
        iss: "".into(),
        ksu: "".into(),
        aud: "".into(),
        sub: "did:pkh:eip155:2:0xbE016C33C395A0891A10626Def9c5C13d8699990".into(),
        act: "".into(),
        scp: "test".into(),
    };

    let claims = serde_json::to_string(&sub_auth).unwrap();
    let base64_claims = base64::engine::general_purpose::STANDARD_NO_PAD.encode(claims.as_bytes());

    let auth = format!("test.{base64_claims}.test");
    let request = json!({"id": 1, "jsonrpc": "2.0", "params": json!({"subscriptionAuth": auth})});

    let message = serde_json::to_string(&request).unwrap();
    dbg!(&message);

    let encrypted = cipher.encrypt(&nonce, message.as_bytes()).unwrap();

    let envelope = EnvelopeType1 {
        envelope_type: 1,
        iv: nonce.into(),
        sealbox: encrypted,
        pubkey: *public.as_bytes(),
    };

    // base64 encode envelope
    let env_base64 = base64::engine::general_purpose::STANDARD.encode(envelope.to_bytes());
    dbg!(&env_base64);

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(env_base64)
        .unwrap();
    let _envelope = EnvelopeType1::from_bytes(decoded);

    let uniform = Uniform::from(0u8..=255);

    let mut rng = StdRng::from_entropy();

    let nonce: GenericArray<u8, U12> =
        GenericArray::from_iter(uniform.sample_iter(&mut rng).take(12));

    let message = "{\"code\": 3,\"message\": \"Unregister reason\"}";

    let encrypted = cipher.encrypt(&nonce, message.as_bytes()).unwrap();

    let envelope = EnvelopeType0 {
        envelope_type: 0,
        iv: nonce.into(),
        sealbox: encrypted,
    };

    let encrypted_msg = envelope.to_bytes();

    let message = base64::engine::general_purpose::STANDARD.encode(encrypted_msg);
    dbg!(message);

    let _decrypted = cipher
        .decrypt(
            GenericArray::from_slice(&envelope.iv),
            chacha20poly1305::aead::Payload::from(&*envelope.sealbox),
        )
        .unwrap();
}

#[test]
fn test_derive() {
    let priv1 = StaticSecret::random_from_rng(OsRng);
    let priv2 = StaticSecret::random_from_rng(OsRng);

    let pub1 = PublicKey::from(&priv1);
    let pub2 = PublicKey::from(&priv2);

    let shared1 = derive_key(hex::encode(pub1.as_bytes()), hex::encode(priv2.as_bytes()));
    let shared2 = derive_key(hex::encode(pub2.as_bytes()), hex::encode(priv1.as_bytes()));

    dbg!(shared1);
    dbg!(shared2);
}

#[test]
fn test_derive2() {
    let a = "740139c6c7f6555088670c7893e959ce4fa4b19591cd31b0ef463f8c54664a50";
    let b = "c7ebe3fce8f3923c6880a81da770b4e6d52bd161a2967f6d14bf431c08284ce8";
    let res = "c7be6a1fbefd91eb14009370411892306aa7898759051cf3b5c882820ab05b4e";

    let aa = "540322ceae8bf597dc716158a2353c9d2a9137411cd02c288e0d95885faee9c6";
    let bb = "4065617265722064303430326361312d623439312d343733642d623562352d73";
    let resres = "6b5ab3a5654b0d08cd4dd15aa6612e2c60a352ddcf276a05719b2e9c8bf4d5c3";

    let shared2 = derive_key(aa.to_string(), bb.to_string());
    let shared1 = derive_key(a.to_string(), b.to_string());
    dbg!(res, shared1, resres);
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
