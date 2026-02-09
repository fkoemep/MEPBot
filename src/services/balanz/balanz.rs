use actix_web::{web, HttpResponse};
use std::time::Duration;
use tokio::time::sleep;
use crate::{AppError};
use crate::services::balanz::balanz_config::{BalanzState};
use super::balanz_ws::fetch_quotes_from_websocket;


#[actix_web::get("/quotes")]
async fn get_quotes(state: web::Data<BalanzState>) -> Result<HttpResponse, AppError> {
    log::info!("Received request for /quotes");
    const MAX_RETRIES: u32 = 1;
    let mut last_error: Option<AppError> = None;

    for attempt in 0..=MAX_RETRIES {
        match fetch_quotes_from_websocket(&state).await {
            Ok(data) => {

                // Immediately return the data to the client.
                log::info!("Fetched quotes: {:?}", data);
                return Ok(HttpResponse::Ok().json(data));
            }
            Err(e) => {
                log::warn!("Attempt {} failed: {}", attempt + 1, e);
                // If the error is due to bad credentials, clear the token to force a re-login on the next attempt.
                if matches!(e, AppError::WebSocketBadCredentials | AppError::BadCredentials) {
                    let mut token_guard = state.access_token.write().await;
                    *token_guard = String::new();
                    log::info!("Cleared invalid access token. Will attempt to re-login.");
                    let mut force_refresh_guard = state.force_refresh.write().await;
                    *force_refresh_guard = true;
                }
                last_error = Some(e);
                sleep(Duration::from_secs(2)).await; // Wait before retrying
            }
        }
    }
    log::error!("All attempts to fetch quotes failed");
    Err(last_error.unwrap_or(AppError::MaxRetriesExceeded))
}

pub(crate) fn scope() -> get_quotes {
    get_quotes
}