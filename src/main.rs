use std::error;
use actix_web::{App, HttpServer, ResponseError, http::StatusCode};
use std::env;
use actix_web::rt::signal;
use thiserror::Error;
use log::error;
use firestore::{errors::FirestoreError};
mod services;
use services::balanz::balanz_config::init_state as balanz_init_state;
use services::balanz::balanz::scope as balanz_scope;

#[derive(Error, Debug)]
enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] env::VarError),
    #[error("HTTP request error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("JSON serialization/deserialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Firestore error: {0}")]
    Firestore(#[from] FirestoreError),
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("URL parsing error: {0}")]
    UrlParse(#[from] url::ParseError),
    #[error("Login failed: Invalid credentials or blocked user")]
    BadCredentials,
    #[error("Login failed: Temporary server error")]
    LoginServerError,
    #[error("Login failed: Could not parse access token from response")]
    LoginResponseMissingToken,
    #[error("Login failed: Could not get nonce from init response")]
    LoginNonceMissing,
    #[error("WebSocket connection closed with bad credentials")]
    WebSocketBadCredentials,
    #[error("Timeout while waiting for WebSocket data")]
    WebSocketTimeout,
    #[error("Maximum retries exceeded")]
    MaxRetriesExceeded,
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            AppError::BadCredentials | AppError::WebSocketBadCredentials => StatusCode::UNAUTHORIZED,
            AppError::LoginServerError => StatusCode::SERVICE_UNAVAILABLE,
            AppError::WebSocketTimeout => StatusCode::GATEWAY_TIMEOUT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}



#[actix_web::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    // For local development, load .env file if it exists.
    dotenv::dotenv().ok();
    // Initialize logger
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let balanz_state = balanz_init_state().await;

    let host = "0.0.0.0";
    let port = env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(balanz_state.clone())
            .service(balanz_scope())
    })
        .bind_auto_h2c((host, port))?
        .run();

    // Wait for either server completion or Ctrl+C
    tokio::select! {
    res = server => res.map_err(|e| e.into()),
    _ = signal::ctrl_c() => {
        error!("Shutdown signal received, stopping server.");
        Ok(())
        }
    }
}