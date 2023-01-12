// mod env;
// mod providers;
// mod store; // Comment this out for now
mod context;
mod functional;

pub type ErrorResult<T> = Result<T, TestError>;

#[derive(Debug, thiserror::Error)]
pub enum TestError {
    #[error(transparent)]
    Elapsed(#[from] tokio::time::error::Elapsed),

    #[error(transparent)]
    RustHttpStarter(#[from] rust_http_starter::error::Error),
}
