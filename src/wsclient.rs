use {
    crate::{auth::jwt_token, error::Result},
    dashmap::DashMap,
    ed25519_dalek::Keypair,
    futures::{future, StreamExt},
    rand::{thread_rng, Rng},
    serde_json::Value,
    std::{net::SocketAddr, str::FromStr, sync::Arc},
    tokio::{select, sync::mpsc},
    tokio_stream::wrappers::ReceiverStream,
    tungstenite::Message,
    walletconnect_sdk::rpc::rpc::{Params, Payload, Publish, Request, RequestPayload, Subscribe},
};

type MessageId = String;

#[derive(Debug)]
pub struct WsClient {
    pub project_id: String,
    pub tx: mpsc::Sender<Message>,
    pub rx: mpsc::Receiver<Result<Message>>,
    /// Received ACKs, contains a set of message IDs.
    pub received_acks: Arc<DashMap<MessageId, serde_json::Value>>,
    pub is_closed: bool,
}

impl WsClient {
    pub async fn reconnect(&mut self, url: &str, keypair: &Keypair) -> Result<()> {
        let jwt = jwt_token(&url, &keypair);
        connect(url, &self.project_id, jwt).await?;
        Ok(())
    }

    pub async fn send(&mut self, msg: Request) {
        self.send_raw(Payload::Request(msg)).await
    }

    pub async fn send_raw(&mut self, msg: Payload) {
        let msg = serde_json::to_string(&msg).unwrap();
        self.tx.send(Message::Text(msg)).await.unwrap()
    }

    pub async fn recv(&mut self) -> Result<Payload> {
        let msg = self.rx.recv().await.unwrap().unwrap();
        // dbg!(&msg);
        match msg {
            Message::Text(msg) => {
                // dbg!(Value::from_str(&msg).unwrap());

                Ok(serde_json::from_str(&msg).unwrap())
            }
            _ => Err(crate::error::Error::RecvError),
        }
    }

    pub async fn publish(&mut self, topic: &str, payload: &str) {
        let msg = Payload::Request(new_rpc_request(
            walletconnect_sdk::rpc::rpc::Params::Publish(Publish {
                topic: topic.into(),
                message: payload.into(),
                ttl_secs: 8400,
                tag: 1000,
                prompt: true,
            }),
        ));

        self.send_raw(msg).await;
    }

    pub async fn subscribe(&mut self, topic: &str) {
        let msg = Payload::Request(new_rpc_request(Params::Subscribe(Subscribe {
            topic: topic.into(),
        })));
        self.send_raw(msg).await;
    }
}

fn new_rpc_request(params: Params) -> Request {
    Request {
        id: thread_rng().gen::<u64>().into(),
        jsonrpc: "2.0".into(),
        params,
    }
}

pub async fn connect(url: &str, project_id: &str, jwt: String) -> Result<WsClient> {
    let relay_query = format!("auth={jwt}&projectId={project_id}");

    let mut url = url::Url::parse(&url)?;
    url.set_query(Some(&relay_query));

    let (connection, _) = async_tungstenite::tokio::connect_async(&url).await?;

    // A channel for passing messages to websocket
    let (wr_tx, wr_rx) = mpsc::channel(1024);

    // A channel for incoming messages from websocket
    let (rd_tx, rd_rx) = mpsc::channel::<Result<_>>(1024);

    tokio::spawn(async move {
        let (tx, rx) = connection.split();
        // Forward messages to the write half
        let write = ReceiverStream::new(wr_rx).map(Ok).forward(tx);

        // Process incoming messages and close the
        // client in case of a close frame. Else forward them.
        let read = rx
            .take_while(|result| match result {
                Err(_) => future::ready(false),
                Ok(m) => future::ready(!m.is_close()),
            })
            .for_each_concurrent(None, |result| async {
                rd_tx
                    .send(result.map_err(Into::into))
                    .await
                    .expect("ws receive error");
            });

        // Keep the thread alive until either
        // read or write complete.
        select! {
            _ = read => {},
            _ = write => {},
        };
    });

    Ok(WsClient {
        project_id: project_id.to_string(),
        tx: wr_tx,
        rx: rd_rx,
        received_acks: Default::default(),
        is_closed: false,
    })
}

#[cfg(test)]
mod test {
    use {
        super::{connect, WsClient},
        crate::auth::jwt_token,
        chacha20poly1305::KeyInit,
        rand::{rngs::StdRng, SeedableRng},
        serde_json::Value,
        std::{str::FromStr, thread::sleep, time::Duration},
        walletconnect_sdk::rpc::rpc::{
            Payload,
            Publish,
            Request,
            Response,
            Subscribe,
            Subscription,
            SubscriptionData,
            SuccessfulResponse,
            Unsubscribe,
        },
    };

    #[tokio::test]
    async fn test() {
        // let relay_url = "wss://staging.relay.walletconnect.com";
        let relay_url = "ws://localhost:8080";
        let jwt_url = "wss://relay.walletconnect.com";

        let key =
            chacha20poly1305::ChaCha20Poly1305::generate_key(&mut chacha20poly1305::aead::OsRng {});
        let hex_key = hex::encode(key);

        let topic = sha256::digest(&*key);
        let topic = "1df17af05005c84e6e29337de84ec4ffce46702e582dfbf2277b71ce64c61311";

        // let relay_url = "ws://localhost:38274";
        // let jwt = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtucEM3TWNFaGhrTjRYUnc3VVl3UWF6aEhqakNWclJTMVBZVXdaTHZTS25KcCIsInN1YiI6ImE4NzgxZmEzMmEwMzkzYjdiNjRlNjNkODgxMDBiZDZlOTAwMTQ5NzkyMDEyM2M3ZTg0YjUxOTU5NmZjMWM4ODIiLCJhdWQiOiJ3c3M6Ly9yZWxheS53YWxsZXRjb25uZWN0LmNvbSIsImlhdCI6MTY2OTQ3MzczOCwiZXhwIjo0ODIzMDczNzM4fQ.noqW7tttY1zk8P4Zy62FsGU7U9Rv78yYLsLVctkIOtQzrzxhjNWTAng-pCVD6IDcX6_HWjp5QSZoS_ppOB6tCw".to_string();
        // let jwt2 =
        // "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSJ9.
        // eyJpc3MiOiJkaWQ6a2V5Ono2TWttQ1QzVkZHb0x5NnJ5RmJvSDNTam9oN2Q5cDNEc3Z3S0RCQ0F0NnM3MkI2MSIsInN1YiI6ImQyMTNmMTY1ZmU0ZGM5NjBlMGMwOTE1YzRlNDVkNTZiZDgzNWI1ODkwMjE0YWNiMjBmZDk3NmRlZjBlMzFmYWEiLCJhdWQiOiJ3c3M6Ly9yZWxheS53YWxsZXRjb25uZWN0LmNvbSIsImlhdCI6MTY2OTQ3MzczOCwiZXhwIjo0ODIzMDczNzM4fQ.
        // dksCJvf0j33ydRtH3qZl39qlMVsxWrlZcPIVC7sihVbBp-w483OM6MD4S1ifDJUGvAV4NRlQEiBlNNV7gfvECQ"
        // .to_string();
        // let project_id = "e1d6f902f8d564b3dbd11c7f4fcd64a2";
        let project_id = "a98ba1147c271b728d31aecc3952ddc3";
        let project_id2 = "c5080ed4772a60c6dd929255054afea4";
        // let project_id2 = "e1d6f902f8d564b3dbd11c7f4fcd64a3";
        // a98ba1147c271b728d31aecc3952ddc3
        // jwt_token(&url, &keypair)

        // dbg!(serde_json::to_string(&Payload::Request(Request {
        //     id: 10680664927050751414.into(),
        //     jsonrpc: "2.0".into(),
        //     params: walletconnect_sdk::rpc::rpc::Params::Subscription(Subscription {
        //         id:
        // "00c4716ba6ece60f11d7fe01169d3e87288f69971a94c7c1430bedb83fd11623".into(),
        //         data: SubscriptionData {
        //             topic: topic.clone().into(),
        //             message: "test".into(),
        //             published_at: 123
        //         }
        //     })
        // }))
        // .unwrap());

        let seed: [u8; 32] = "TO_JEST_TEST_CO_TAM_U_WAS_FAJNE_TESTOWANKO_1231239u127309128309832"
            .to_string()
            .as_bytes()[..32]
            .try_into()
            .unwrap();
        let mut seeded = StdRng::from_seed(seed);
        let keypair = ed25519_dalek::Keypair::generate(&mut seeded);
        let jwt = jwt_token(&jwt_url, &keypair);

        let seed: [u8; 32] = "1TO_JEST_TEST_CO_TAM_U_WAS_FAJNE_TESTOWANKO_1231239u127309128309832"
            .to_string()
            .as_bytes()[..32]
            .try_into()
            .unwrap();
        let mut seeded = StdRng::from_seed(seed);
        let keypair = ed25519_dalek::Keypair::generate(&mut seeded);
        let jwt2 = jwt_token(&jwt_url, &keypair);

        let mut seeded = StdRng::from_seed(seed);
        let keypair = ed25519_dalek::Keypair::generate(&mut seeded);
        let mut client = connect(&relay_url, project_id2, jwt.clone()).await.unwrap();

        client
            .subscribe("1df17af05005c84e6e29337de84ec4ffce46702e582dfbf2277b71ce64c61311")
            .await;

        client
            .publish(
                "1df17af05005c84e6e29337de84ec4ffce46702e582dfbf2277b71ce64c61311",
                "AARMPnqHRFz14/0itwGhld5C3Kpue1WicnQgKY06PmcKpiAxh/JFVgENknNBhv5XIXulUpihKjqrE62Um7/E/UY7obbBrQNiJtsG5RY4J1tySIgZAXjLkSvp0Vqz2bCh7DaGa7DziGgd9rp1sTdUCYsyM3IVpK4T3IpaKlHkjoBzueKO2ePHYFiutiEv2RJcBJp9llSat8jT9NykN9++dQAeAPxW4w==",
            )
            .await;

        // if let Payload::Response(response) = client.recv().await.unwrap() {
        //     if let Response::Success(response) = response {
        //         client
        //             .send(Request {
        //                 id: 3.into(),
        //                 jsonrpc: "2.0".into(),
        //                 params:
        // walletconnect_sdk::rpc::rpc::Params::Unsubscribe(Unsubscribe {
        //                     topic:
        //
        // "1df17af05005c84e6e29337de84ec4ffce46702e582dfbf2277b71ce64c61311"
        //                             .into(),
        //                     subscription_id: response.result.to_string().into(),
        //                 }),
        //             })
        //             .await;
        //     }
        // }

        let mut client2 = connect(&relay_url, project_id2, jwt2.clone())
            .await
            .unwrap();
        // client2.subscribe(&topic).await;
        // dbg!(client2.recv().await.unwrap());
        // client.publish(&topic, "test").await;
        // dbg!(client.recv().await);

        // client.subscribe(&topic).await;
        // dbg!(client.recv().await);

        // dbg!(client2.recv().await);

        // client.send(subscribe.clone()).await;
        // client2.send(subscribe).await;
        // client2
        //     .send_raw(Payload::Response(Response::Success(SuccessfulResponse
        // {         id: 1.into(),
        //         jsonrpc: "2.0".into(),
        //         result: true.into(),
        //     })))
        //     .await;
        // let sub_id = if let Payload::Response(response) =
        // client.recv().await.unwrap() {     if let
        // Response::Success(success) = response {         success.
        // result.to_string()     } else {
        //         "costam".into()
        //     }
        // } else {
        //     "costam".into()
        // };

        // let unsubscribve = Request {
        //     id: 3.into(),
        //     jsonrpc: "2.0".into(),
        //     params:
        // walletconnect_sdk::rpc::rpc::Params::Unsubscribe(Unsubscribe {
        //         subscription_id: sub_id.into(),
        //         topic: topic.clone().into(),
        //     }),
        // };

        // client.send(publish.clone()).await;
        // client
        //     .send_raw(Payload::Response(Response::Success(SuccessfulResponse
        // {         id: 2.into(),
        //         jsonrpc: "2.0".into(),
        //         result: true.into(),
        //     })))
        //     .await;

        // dbg!(client.recv().await);
    }
}
