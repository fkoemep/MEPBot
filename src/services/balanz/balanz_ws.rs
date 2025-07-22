use actix_web::{web};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use futures_util::StreamExt;
use futures_util::sink::SinkExt;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use crate::{AppError};
use crate::services::balanz::balanz_auth::get_or_refresh_token;
use crate::services::balanz::balanz_config::BalanzState;

const REQUIRED_KEYS: &[&str] = &[
    "al30_ask", "al30_bid", "al30d_ask", "al30d_bid",
    "gd30_ask", "gd30_bid", "gd30d_ask", "gd30d_bid",
    "al30_ask_24hs", "al30_bid_24hs", "al30d_ask_24hs", "al30d_bid_24hs",
    "gd30_ask_24hs", "gd30_bid_24hs", "gd30d_ask_24hs", "gd30d_bid_24hs",
];

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
        let ask_key = format!("{}_ask{}", ticker_lower, plazo_suffix);
        let bid_key = format!("{}_bid{}", ticker_lower, plazo_suffix);

        if REQUIRED_KEYS.contains(&ask_key.as_str()) {
            data.insert(ask_key, pv * 100.0);
        }
        if REQUIRED_KEYS.contains(&bid_key.as_str()) {
            data.insert(bid_key, pc * 100.0);
        }
    }
}

/// Connects to the WebSocket, subscribes, and gathers all required data.
pub(super) async fn fetch_quotes_from_websocket(state: &web::Data<BalanzState>) -> Result<HashMap<String, f64>, AppError> {
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

    let mut timeout = Box::pin(sleep(timeout_duration));

    loop {
        tokio::select! {
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
            _ = &mut timeout => {
                log::error!("Timed out waiting for WebSocket data. Received {} of {} keys.", data_map.len(), REQUIRED_KEYS.len());
                return Err(AppError::WebSocketTimeout);
            }
        }
    }

    write.close().await?;
    Ok(data_map)
}