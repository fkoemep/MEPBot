use tokio::time::{sleep, Duration};
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;
use tokio_tungstenite::connect_async;
use url::Url;
use firestore::*;
use tokio_tungstenite::tungstenite::Utf8Bytes;

type SharedData = Arc<Mutex<HashMap<String, f64>>>;

static ACCESS_TOKEN: OnceCell<String> = OnceCell::const_new();
static FIRESTORE: OnceCell<FirestoreDb> = OnceCell::const_new();

fn update_data(data: &mut HashMap<String, f64>, message: &Value) {
    let plazo = message.get("plazo").and_then(|v| v.as_str()).unwrap_or("");
    let ticker = message.get("ticker").and_then(|v| v.as_str()).unwrap_or("");
    let pv = message.get("pv").and_then(|v| v.as_f64()).unwrap_or(0.0) * 100.0;
    let pc = message.get("pc").and_then(|v| v.as_f64()).unwrap_or(0.0) * 100.0;

    match plazo {
        "CI" => {
            match ticker {
                "AL30" => {
                    data.insert("al30_ask".to_string(), pv);
                    data.insert("al30_bid".to_string(), pc);
                }
                "AL30D" => {
                    data.insert("al30d_ask".to_string(), pv);
                    data.insert("al30d_bid".to_string(), pc);
                }
                "GD30" => {
                    data.insert("gd30_ask".to_string(), pv);
                    data.insert("gd30_bid".to_string(), pc);
                }
                "GD30D" => {
                    data.insert("gd30d_ask".to_string(), pv);
                    data.insert("gd30d_bid".to_string(), pc);
                }
                _ => {}
            }
        }
        "24hs" => {
            match ticker {
                "AL30" => {
                    data.insert("al30_ask_24hs".to_string(), pv);
                    data.insert("al30_bid_24hs".to_string(), pc);
                }
                "AL30D" => {
                    data.insert("al30d_ask_24hs".to_string(), pv);
                    data.insert("al30d_bid_24hs".to_string(), pc);
                }
                "GD30" => {
                    data.insert("gd30_ask_24hs".to_string(), pv);
                    data.insert("gd30_bid_24hs".to_string(), pc);
                }
                "GD30D" => {
                    data.insert("gd30d_ask_24hs".to_string(), pv);
                    data.insert("gd30d_bid_24hs".to_string(), pc);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn all_keys_present(data: &HashMap<String, f64>) -> bool {
    [
        "al30_ask", "al30_bid", "al30d_ask", "al30d_bid",
        "gd30_ask", "gd30_bid", "gd30d_ask", "gd30d_bid",
        "al30_ask_24hs", "al30_bid_24hs", "al30d_ask_24hs", "al30d_bid_24hs",
        "gd30_ask_24hs", "gd30_bid_24hs", "gd30d_ask_24hs", "gd30d_bid_24hs"
    ].iter().all(|k| data.contains_key(*k))
}

async fn login(client: &Client, payload: &Value, headers: &reqwest::header::HeaderMap, firestore: &FirestoreDb) -> String {
    let resp = client
        .post("https://clientes.balanz.com/api/v1/auth/init")
        .json(&json!({"user": payload["user"], "source": "WebV2"}))
        .headers(headers.clone())
        .send()
        .await;

    let resp = match resp {
        Ok(r) => {
            if r.status().is_server_error() {
                eprintln!("Received 5xx error: {}", r.status());
                // Return a special value to indicate a temporary error
                return "__TEMPORARY_ERROR__".to_string();
            }
            r
        }
        Err(e) => {
            eprintln!("HTTP request failed: {:?}", e);
            return String::new();
        }
    };

    let text = resp.text().await;
    let text = match text {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to read response body: {:?}", e);
            return String::new();
        }
    };
    println!("Raw response: {}", text);

    let init_resp = serde_json::from_str::<Value>(&text);
    let init_resp = match init_resp {
        Ok(val) => val,
        Err(e) => {
            eprintln!("Failed to parse JSON: {:?}", e);
            return String::new();
        }
    };

    let mut login_payload = payload.clone();
    login_payload["nonce"] = init_resp["nonce"].clone();

    let login_resp = client
        .post("https://clientes.balanz.com/api/v1/auth/login")
        .json(&login_payload)
        .headers(headers.clone())
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();

    let access_token = login_resp["AccessToken"].as_str().unwrap_or("").to_string();

    // Save the new access token to Firestore
    let _ = firestore.fluent()
        .update()
        .in_col("MEPBot")
        .document_id("AccessToken")
        .object(&json!({ "value": &access_token }))
        .execute::<()>()
        .await;

    access_token
}

async fn get_quotes(_data: web::Data<SharedData>) -> impl Responder {
    let user = env::var("BALANZ_USER").unwrap_or_default();
    let password = env::var("BALANZ_PASSWORD").unwrap_or_default();
    let gcp_project = env::var("GCP_PROJECT_ID").unwrap_or_default();

    let payload = json!({
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

    let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36 Edg/125.0.0.0";
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-type", "application/json".parse().unwrap());
    headers.insert("Accept", "application/json".parse().unwrap());
    headers.insert("User-Agent", user_agent.parse().unwrap());
    headers.insert("Referer", "https://clientes.balanz.com/".parse().unwrap());

    let client = Client::new();

    let firestore = FIRESTORE
        .get_or_init(|| async {
            FirestoreDb::new(&gcp_project).await.unwrap()
        })
        .await;

    let doc: FirestoreResult<Option<Value>> = firestore
        .fluent()
        .select()
        .by_id_in("MEPBot")
        .obj()
        .one("AccessToken")
        .await;

    if let Ok(Some(val)) = doc {
        if let Some(token) = val.get("value").and_then(|v| v.as_str()) {
            let _ = ACCESS_TOKEN.set(token.to_string());
        }
    }

    let mut access_token = {
        let token = ACCESS_TOKEN.get_or_init(|| async { login(&client, &payload, &headers, &firestore).await }).await.clone();
        if token.is_empty() {
            println!("Access token not found, logging in...");
            let new_token = login(&client, &payload, &headers, &firestore).await;
            let _ = ACCESS_TOKEN.set(new_token.clone());
            new_token
        } else {
            token
        }
    };

    if access_token == "__TEMPORARY_ERROR__" {
        return HttpResponse::ServiceUnavailable().body("Temporary server error, please try again later.");
    }

    // Outer loop for reconnecting on "Bad credentials"
    let mut retries = 0;
    const MAX_RETRIES: usize = 3;

    'outer: loop {
        if retries >= MAX_RETRIES {
            return HttpResponse::InternalServerError().body("Failed after 3 retries");
        }

        let ws_url = Url::parse("wss://clientes.balanz.com/websocket").unwrap();
        let msg = json!({"panel": 6, "token": &access_token}).to_string();

        let ws_result = connect_async(ws_url.as_str()).await;

        if ws_result.is_err() {
            println!("WebSocket connection error: {:?}", ws_result.err());
            // break;
            retries += 1;
            sleep(Duration::from_secs(10)).await;
            // access_token = login(&client, &payload, &headers, &firestore).await;
            // let _ = ACCESS_TOKEN.set(access_token.clone());
            continue;
        }

        let (ws_stream, _) = ws_result.unwrap();
        let (mut write, mut read) = ws_stream.split();
        write.send(tokio_tungstenite::tungstenite::Message::Text(Utf8Bytes::from(msg))).await.unwrap();

        let mut local_data = HashMap::new();

        while !all_keys_present(&local_data) {
            match read.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    if let Ok(message) = serde_json::from_str::<Value>(&text) {
                        update_data(&mut local_data, &message);
                    }
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(Some(frame)))) => {
                    if frame.reason == Utf8Bytes::from("Bad credentials") {
                        println!("Bad credentials, refreshing token...");
                        retries += 1;
                        sleep(Duration::from_secs(10)).await;
                        access_token = login(&client, &payload, &headers, &firestore).await;
                        let _ = ACCESS_TOKEN.set(access_token.clone());
                        continue 'outer;
                    } else {
                        println!("WebSocket closed: {:?}", frame);
                        break;
                    }
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(_))) => {}
                Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(_))) => {}
                Some(Ok(tokio_tungstenite::tungstenite::Message::Pong(_))) => {}
                Some(Ok(_)) => {}

                Some(Err(e)) => {
                    println!("WebSocket error: {:?}", e);
                    break;
                }
                None => {
                    println!("Got none, breaking");
                    break;
                }
            }
        }

        println!("Final result: {:?}", local_data);
        let data_json = serde_json::to_string(&local_data).unwrap();
        return HttpResponse::Ok().content_type("application/json").body(data_json);
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let data: SharedData = Arc::new(Mutex::new(HashMap::new()));
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(data.clone()))
            .route("/", web::get().to(get_quotes))
    })
        .bind_auto_h2c(("0.0.0.0", 8080))?
        .run()
        .await
}