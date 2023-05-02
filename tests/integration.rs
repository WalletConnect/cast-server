use {
    base64::Engine,
    cast_server::{
        auth::{jwt_token, SubscriptionAuth},
        handlers::notify::NotifyBody,
        jsonrpc::{JsonRpcParams, JsonRpcPayload, Notification},
        types::{Envelope, EnvelopeType0, EnvelopeType1, RegisterBody},
        wsclient,
    },
    chacha20poly1305::{
        aead::{generic_array::GenericArray, AeadMut, OsRng},
        consts::U12,
        KeyInit,
    },
    parquet::data_type::AsBytes,
    rand::{distributions::Uniform, prelude::Distribution, rngs::StdRng, SeedableRng},
    serde_json::json,
    sha2::Sha256,
    std::{collections::HashSet, time::Duration},
    tokio::time::sleep,
    walletconnect_sdk::rpc::{
        auth::ed25519_dalek::Keypair,
        rpc::{Params, Payload, Publish, Request},
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

// #[tokio::test]
// async fn cast_properly_sending_message() {
//     let env = std::env::var("ENVIRONMENT").unwrap_or("STAGING".to_owned());
//     let project_id = std::env::var("TEST_PROJECT_ID").expect("Tests requires
// PROJECT_ID to be set");

//     let (cast_url, relay_url) = urls(env);

//     // Generate valid JWT
//     let mut rng = StdRng::from_entropy();
//     let keypair = Keypair::generate(&mut rng);
//     let jwt = jwt_token(&relay_url, &keypair).unwrap();

//     // Set up clients
//     let http_client = reqwest::Client::new();
//     let mut ws_client = wsclient::connect(&relay_url, &project_id, jwt)
//         .await
//         .unwrap();

//     // Prepare client key
//     let key =
//         chacha20poly1305::ChaCha20Poly1305::generate_key(&mut
// chacha20poly1305::aead::OsRng {});     let topic = sha256::digest(&*key);
//     let hex_key = hex::encode(key);

//     let test_account = "test_account_send_test".to_owned();

//     // Create valid account
//     let scope: HashSet<String> = std::iter::once("test".into()).collect();

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
//             let encrypted_text = base64::engine::general_purpose::STANDARD
//                 .decode(&*params.data.message)
//                 .unwrap();
//             let envelope = EnvelopeType0::from_bytes(encrypted_text);
//             let decrypted = cipher
//                 .decrypt(&envelope.iv.into(), &*envelope.sealbox)
//                 .unwrap();

//             let push_message: JsonRpcPayload =
// serde_json::from_slice(&decrypted).unwrap();             if let
// JsonRpcParams::Push(notification) = push_message.params {                 
// assert_eq!(notification, test_notification);             } else {
//                 panic!("Notification received not matching notification
// sent")             }
//         } else {
//             panic!("Wrong payload body received");
//         }
//     } else {
//         panic!("Invalid data received");
//     }
// }

// #[tokio::test]
// async fn test_unregister() {
//     let env = std::env::var("ENVIRONMENT").unwrap_or("STAGING".to_owned());
//     let project_id = std::env::var("PROJECT_ID").expect("Tests requires
// PROJECT_ID to be set");

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

    let proj_pub_key = "d5b787c8b6c19a9da1f2f1d745fa2020ce29ba0554e46e622b00a6f7bc051c6d";
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
    let request = json!({"id": "1", "jsonrpc": "2.0", "params": json!({"subscriptionAuth": auth})});

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
