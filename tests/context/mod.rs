use {
    self::server::CastServer,
    async_trait::async_trait,
    cast_server::log::Logger,
    test_context::AsyncTestContext,
};

pub type ErrorResult<T> = Result<T, TestError>;

#[derive(Debug, thiserror::Error)]
pub enum TestError {
    #[error(transparent)]
    Elapsed(#[from] tokio::time::error::Elapsed),

    #[error(transparent)]
    CastServer(#[from] cast_server::error::Error),
}

mod server;

pub struct ServerContext {
    pub server: CastServer,
    pub project_id: String,
    pub relay_url: String,
    logger: Logger,
}

#[async_trait]
impl AsyncTestContext for ServerContext {
    async fn setup() -> Self {
        let logger = Logger::init().expect("Failed to start logging");
        let server = CastServer::start().await;

        let project_id = std::env::var("TEST_PROJECT_ID").unwrap();
        let relay_url = std::env::var("RELAY_URL").unwrap();

        Self {
            server,
            project_id,
            relay_url,
            logger,
        }
    }

    async fn teardown(mut self) {
        self.logger.stop();
        self.server.shutdown().await;
    }
}
