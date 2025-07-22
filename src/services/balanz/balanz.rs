use actix_web::{web, HttpResponse};
use reqwest::{header, Client};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use firestore::{FirestoreDb, FirestoreResult};
use futures_util::StreamExt;
use futures_util::sink::SinkExt;
use serde::Deserialize;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use crate::{AppError};

impl BalanzState {
    async fn new(config: BalanzConfig) -> Result<Self, AppError> {
        let client = Client::new();
        let firestore = FirestoreDb::new(&config.gcp_project_id).await?;
        Ok(Self {
            client,
            firestore,
            config,
            access_token: RwLock::new(String::new()),
            force_refresh: RwLock::new(false), // <-- initialize
        })
    }
}

/// Holds static configuration loaded once at startup.
#[derive(Clone)]
struct BalanzConfig {
    gcp_project_id: String,
    login_payload: Value,
    http_headers: header::HeaderMap,
    timeout_secs: u64, // new field
}

impl BalanzConfig {
    fn from_env() -> Result<Self, AppError> {
        let user = env::var("BALANZ_USER")?;
        let password = env::var("BALANZ_PASSWORD")?;

        let login_payload = json!({
            "user": user,
            "pass": password,
            "source": "WebV2",
            "VersionSO": "10",
            "VersionApp": "2.11.0",
            "TipoDispositivo": "Web",
            "SistemaOperativo": "Windows",
            "NombreDispositivo": "Edge 125.0.0.0",
            "idDispositivo": "84a22d3c-5165-4ed0-b061-0f8b8ddf09d0"
        });

        let mut http_headers = header::HeaderMap::new();
        http_headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        http_headers.insert(header::ACCEPT, "application/json".parse().unwrap());
        http_headers.insert(header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36 Edg/125.0.0.0".parse().unwrap());
        http_headers.insert(header::REFERER, "https://clientes.balanz.com/".parse().unwrap());

        Ok(Self {
            gcp_project_id: env::var("GCP_PROJECT_ID")?,
            login_payload,
            http_headers,
            timeout_secs: env::var("BALANZ_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(15),
        })
    }
}

/// A single struct to hold all shared application state.
pub struct BalanzState {
    client: Client,
    firestore: FirestoreDb,
    config: BalanzConfig,
    access_token: RwLock<String>,
    force_refresh: RwLock<bool>
}


const REQUIRED_KEYS: &[&str] = &[
    "al30_ask", "al30_bid", "al30d_ask", "al30d_bid",
    "gd30_ask", "gd30_bid", "gd30d_ask", "gd30d_bid",
    "al30_ask_24hs", "al30_bid_24hs", "al30d_ask_24hs", "al30d_bid_24hs",
    "gd30_ask_24hs", "gd30_bid_24hs", "gd30d_ask_24hs", "gd30d_bid_24hs",
];

#[derive(Deserialize)]
struct AccessTokenDoc {
    value: String,
}

#[actix_web::get("/quotes")]
async fn get_quotes(state: web::Data<BalanzState>) -> Result<HttpResponse, AppError> {
    log::info!("Received request for /quotes");
    const MAX_RETRIES: u32 = 2;
    let mut last_error: Option<AppError> = None;

    for attempt in 0..=MAX_RETRIES {
        match fetch_quotes_from_websocket(&state).await {
            Ok(data) => {

                // Immediately return the data to the client.
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

    Err(last_error.unwrap_or(AppError::MaxRetriesExceeded))
}

/// Checks if the map contains all the keys we need.
fn all_keys_present(data: &HashMap<String, f64>) -> bool {
    REQUIRED_KEYS.iter().all(|&key| data.contains_key(key))
}

/// Dynamically updates the data map from a WebSocket message.
fn update_data(data: &mut HashMap<String, f64>, message: &Value) {
    if let (Some(plazo), Some(ticker), Some(pv), Some(pc)) = (
        message.get("plazo").and_then(Value::as_str),
        message.get("ticker").and_then(Value::as_str),
        message.get("pv").and_then(Value::as_f64),
        message.get("pc").and_then(Value::as_f64),
    ) {
        let plazo_suffix = match plazo {
            "CI" => "",
            "24hs" => "_24hs",
            _ => return, // Ignore other terms
        };

        let ticker_lower = ticker.to_lowercase();
        data.insert(format!("{}_ask{}", ticker_lower, plazo_suffix), pv * 100.0);
        data.insert(format!("{}_bid{}", ticker_lower, plazo_suffix), pc * 100.0);
    }
}

/// Performs the two-step login process to the Balanz API.
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
async fn get_or_refresh_token(state: &web::Data<BalanzState>) -> Result<String, AppError> {
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
            .by_id_in("MEPBot")
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
            .in_col("MEPBot")
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

/// Connects to the WebSocket, subscribes, and gathers all required data.
async fn fetch_quotes_from_websocket(state: &web::Data<BalanzState>) -> Result<HashMap<String, f64>, AppError> {
    let access_token = get_or_refresh_token(state).await?;
    let ws_url = "wss://clientes.balanz.com/websocket";

    log::info!("Connecting to WebSocket...");
    let (ws_stream, _) = connect_async(ws_url).await?;
    let (mut write, mut read) = ws_stream.split();

    // Subscribe to the quotes panel
    let sub_msg = json!({"panel": 6, "token": &access_token}).to_string();
    write.send(WsMessage::Text(Utf8Bytes::from(sub_msg))).await?;
    log::info!("WebSocket connection established and subscribed to panel.");

    let mut data_map = HashMap::new();
    let timeout_duration = Duration::from_secs(state.config.timeout_secs);

    loop {
        tokio::select! {
            biased; // Prioritize the message stream over the timeout
            maybe_msg = read.next() => {
                match maybe_msg {
                    Some(Ok(WsMessage::Text(text))) => {
                        log::debug!("Received WebSocket message: {:?}", text);
                        if let Ok(message) = serde_json::from_str::<Value>(&text) {
                            update_data(&mut data_map, &message);
                        }
                    }
                    Some(Ok(WsMessage::Close(Some(frame)))) => {
                        log::warn!("WebSocket closed by server: {:?}", frame);
                        if frame.reason.contains("Bad credentials") {
                            return Err(AppError::WebSocketBadCredentials);
                        }
                        break; // Normal close
                    }
                    Some(Err(e)) => {
                        log::error!("WebSocket stream error: {}", e);
                        return Err(e.into());
                    }
                    None => {
                        log::warn!("WebSocket stream ended unexpectedly.");
                        break;
                    }
                    _ => {} // Ignore other message types
                }

                if all_keys_present(&data_map) {
                    log::info!("All required data received from WebSocket.");
                    break;
                }
            },
            _ = tokio::time::sleep(timeout_duration) => {
                log::error!("Timed out waiting for WebSocket data. Received {} of {} keys.", data_map.len(), REQUIRED_KEYS.len());
                return Err(AppError::WebSocketTimeout);
            }
        }
    }

    write.close().await?;
    Ok(data_map)
}

pub async fn init_state() -> web::Data<BalanzState> {
    let config = BalanzConfig::from_env().unwrap_or_else(|e| {
        log::error!("FATAL: Failed to load balanz configuration from environment: {}", e);
        std::process::exit(1);
    });

    let state = BalanzState::new(config).await.unwrap_or_else(|e| {
        log::error!("FATAL: Failed to initialize balanz application state: {}", e);
        std::process::exit(1);
    });

    web::Data::new(state)
}

pub fn scope() -> actix_web::Scope {
    web::scope("")
        .service(get_quotes)
}