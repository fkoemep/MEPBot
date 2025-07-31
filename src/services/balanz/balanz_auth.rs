use actix_web::{web};
use reqwest::{Client};
use serde_json::{json, Value};
use firestore::{FirestoreResult};
use serde::Deserialize;
use crate::{AppError};
use crate::services::balanz::balanz_config::{BalanzConfig, BalanzState};

/// Performs the two-step login process to the Balanz API.

#[derive(Deserialize)]
struct AccessTokenDoc {
    value: String,
}

async fn login(client: &Client, config: &BalanzConfig) -> Result<String, AppError> {
    // Step 1: Get nonce
    let init_resp = client
        .post("https://clientes.balanz.com/api/v1/auth/init")
        .json(&json!({"user": config.login_payload["user"], "source": "WebV2"}))
        .headers(config.http_headers.clone())
        .send()
        .await?;

    if init_resp.status().is_server_error() {
        return Err(AppError::LoginServerError);
    }
    let init_json: Value = init_resp.json().await?;
    let nonce = init_json["nonce"].as_str().ok_or(AppError::LoginNonceMissing)?;

    // Step 2: Login with nonce
    let mut final_payload = config.login_payload.clone();
    final_payload["nonce"] = json!(nonce);

    let login_resp = client
        .post("https://clientes.balanz.com/api/v1/auth/login")
        .json(&final_payload)
        .headers(config.http_headers.clone())
        .send()
        .await?;

    if login_resp.status().is_client_error() {
        return Err(AppError::BadCredentials);
    }
    if !login_resp.status().is_success() {
        return Err(AppError::LoginServerError);
    }

    let login_json: Value = login_resp.json().await?;
    let access_token = login_json["AccessToken"].as_str().ok_or(AppError::LoginResponseMissingToken)?;

    Ok(access_token.to_string())
}

/// Gets a valid access token, trying Firestore first, then logging in if necessary.
/// Uses RwLock to prevent multiple concurrent login attempts.
pub(super) async fn get_or_refresh_token(state: &web::Data<BalanzState>) -> Result<String, AppError> {
    // Fast path: Try to get a read lock and return the existing token.
    let token_read_guard = state.access_token.read().await;
    if !token_read_guard.is_empty() {
        return Ok(token_read_guard.clone());
    }
    drop(token_read_guard); // Drop read lock before acquiring write lock

    // Slow path: Acquire a write lock to perform login or DB fetch.
    let mut token_write_guard = state.access_token.write().await;

    // Double-check: another request might have populated the token while we waited for the lock.
    if !token_write_guard.is_empty() {
        return Ok(token_write_guard.clone());
    }

    let force_refresh_read_guard = state.force_refresh.read().await;

    if !*force_refresh_read_guard {
        // Try to get token from Firestore first
        log::info!("Token not in memory, checking Firestore...");
        let doc: FirestoreResult<Option<AccessTokenDoc>> = state.firestore.fluent()
            .select()
            .by_id_in("mep-bot")
            .obj()
            .one("AccessToken")
            .await;

        if let Ok(Some(token_doc)) = doc {
            if !token_doc.value.is_empty() {
                log::info!("Found valid token in Firestore.");
                *token_write_guard = token_doc.value.clone();
                return Ok(token_doc.value);
            }
        }
    }

    // If not in memory or Firestore, perform a fresh login.
    log::info!("No valid token found. Logging in to Balanz...");
    let new_token = login(&state.client, &state.config).await?;
    *token_write_guard = new_token.clone();
    log::info!("Login successful. New token acquired.");

    // Asynchronously save the new token to Firestore for persistence.
    let firestore_clone = state.firestore.clone();
    let token_to_save = new_token.clone();
    tokio::spawn(async move {
        let update_result = firestore_clone.fluent()
            .update()
            .in_col("mep-bot")
            .document_id("AccessToken")
            .object(&json!({ "value": token_to_save }))
            .execute::<()>()
            .await;
        if let Err(e) = update_result {
            log::error!("Failed to save new access token to Firestore: {}", e);
        }
    });

    Ok(new_token)
}
