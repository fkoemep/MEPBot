use actix_web::{web};
use reqwest::{header, Client};
use serde_json::{json, Value};
use std::env;
use firestore::{FirestoreDb};
use tokio::sync::RwLock;
use crate::{AppError};


/// Holds static configuration loaded once at startup.
#[derive(Clone)]
pub(super) struct BalanzConfig {
    pub(super) gcp_project_id: String,
    pub(super) login_payload: Value,
    pub(super) http_headers: header::HeaderMap,
    pub(super) timeout_secs: u64, // new field
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
            gcp_project_id: env::var("PROJECT_ID")?,
            login_payload,
            http_headers,
            timeout_secs: env::var("BALANZ_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(60),
        })
    }
}

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

/// A single struct to hold all shared application state.
pub(crate) struct BalanzState {
    pub(super) client: Client,
    pub(super) firestore: FirestoreDb,
    pub(super) config: BalanzConfig,
    pub(super) access_token: RwLock<String>,
    pub(super) force_refresh: RwLock<bool>
}

pub(crate) async fn init_state() -> web::Data<BalanzState> {
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
