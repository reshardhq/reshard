//! The reshard relay.
//!
//! Commands come in over HTTP, events go out over one WebSocket. Durable
//! events are appended to SQLite first; ephemeral ones (agent telemetry) are
//! broadcast and forgotten.

mod store;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{
        ws::{close_code, CloseFrame, Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use reshard_core::{
    Approval, Command, Event, HistoryGrant, Message, MessageKind, StatusState, Trigger,
};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::store::{
    ApprovalCreate, ApprovalMetrics, ApprovalTransition, AuditEntry, AuthError, DevicePoll,
    MachineRecord, Store, User,
};

const DEFAULT_PORT: u16 = 8787;

/// Where the log lives. A relative default would make the database depend on
/// the directory you happened to launch from — two shells, two histories, and
/// no hint that anything is wrong. `RESHARD_DB` overrides for tests.
fn db_path() -> String {
    if let Ok(path) = std::env::var("RESHARD_DB") {
        return path;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dir = format!("{home}/.reshard");
    let _ = std::fs::create_dir_all(&dir);
    format!("{dir}/reshard.db")
}

#[derive(Clone)]
struct App {
    store: Arc<Store>,
    events: broadcast::Sender<Event>,
}

#[tokio::main]
async fn main() {
    let db = db_path();
    let store = Store::open(&db).expect("open database");
    let (events, _) = broadcast::channel(1024);
    let app_state = App {
        store: Arc::new(store),
        events,
    };

    tokio::spawn(expiry_sweeper(app_state.clone()));

    let app = router(app_state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");

    println!("reshard relay  ·  http://127.0.0.1:{port}  ·  {db}");
    axum::serve(listener, app).await.unwrap();
}

fn router(app_state: App) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/auth/me", get(me))
        .route("/auth/logout", post(logout))
        .route("/auth/device/start", post(start_device_auth))
        .route("/auth/device/approve", post(approve_device_auth))
        .route("/auth/device/token", post(poll_device_auth))
        .route("/auth/machine", get(machine_me))
        .route("/auth/machine/logout", post(machine_logout))
        .route("/bootstrap", post(bootstrap))
        .route("/pairings", post(create_pairing))
        .route("/pair", post(pair))
        .route("/machines", get(machines))
        .route("/machines/runtimes", post(report_runtime_inventory))
        .route("/machines/{id}", axum::routing::delete(revoke_machine))
        .route("/machines/heartbeat", post(heartbeat))
        .route("/chats", get(chats).post(create_chat))
        .route("/chats/{id}", patch(update_chat))
        .route("/members", get(members))
        .route("/chats/{id}/messages", get(messages))
        .route("/commands", post(command))
        .route("/approvals", get(approvals))
        .route("/approvals/audit", get(approval_audit))
        .route("/approvals/metrics", get(approval_metrics))
        .route("/approvals/{id}", get(approval))
        .route("/messages/{id}", get(message))
        .route("/invites", post(create_invite))
        .route("/connect", post(connect))
        .route("/members/{id}/memberships", get(memberships))
        .route("/chats/{id}/unread", get(unread))
        .route("/chats/{id}/read", post(read))
        .route(
            "/chats/{id}/members/{member}",
            post(set_trigger).delete(kick),
        )
        .route(
            "/chats/{id}/members/{member}/reset-session",
            post(reset_session),
        )
        .route("/stream", get(stream))
        .layer(cors_layer())
        .with_state(app_state)
}

fn cors_layer() -> CorsLayer {
    let mut origins = vec![
        HeaderValue::from_static("http://localhost:1420"),
        HeaderValue::from_static("http://127.0.0.1:1420"),
        HeaderValue::from_static("tauri://localhost"),
        HeaderValue::from_static("http://tauri.localhost"),
        HeaderValue::from_static("https://tauri.localhost"),
    ];
    if let Ok(extra) = std::env::var("RESHARD_ALLOWED_ORIGINS") {
        origins.extend(
            extra
                .split(',')
                .filter_map(|origin| origin.trim().parse::<HeaderValue>().ok()),
        );
    }

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true, "service": "reshard-relay" }))
}

async fn bootstrap(State(app): State<App>) -> Result<impl IntoResponse, Fail> {
    if !bootstrap_enabled() {
        return Err(Fail::not_found("not found".into()));
    }
    let session = app.store.bootstrap_workspace()?;
    Ok(Json(session))
}

fn bootstrap_enabled() -> bool {
    bootstrap_enabled_for(
        std::env::var("RESHARD_ALLOW_BOOTSTRAP").ok().as_deref(),
        cfg!(debug_assertions),
    )
}

fn bootstrap_enabled_for(setting: Option<&str>, debug_build: bool) -> bool {
    match setting {
        Some("1") | Some("true") => true,
        Some("0") | Some("false") => false,
        _ => debug_build,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterRequest {
    email: String,
    name: String,
    password: String,
}

async fn register(
    State(app): State<App>,
    Json(body): Json<RegisterRequest>,
) -> Result<impl IntoResponse, Fail> {
    validate_registration(&body)?;
    let store = app.store.clone();
    let session = tokio::task::spawn_blocking(move || {
        store.register(&body.email, &body.name, &body.password)
    })
    .await
    .map_err(|_| Fail::internal("authentication worker stopped".into()))?
    .map_err(auth_error)?;
    Ok(Json(session))
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

async fn login(
    State(app): State<App>,
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, Fail> {
    if body.email.len() > 254 || body.password.len() > 1_024 {
        return Err(Fail::unauthorized("email or password is incorrect".into()));
    }
    let store = app.store.clone();
    let session = tokio::task::spawn_blocking(move || store.login(&body.email, &body.password))
        .await
        .map_err(|_| Fail::internal("authentication worker stopped".into()))?
        .map_err(auth_error)?;
    Ok(Json(session))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefreshRequest {
    refresh_token: String,
}

async fn refresh(
    State(app): State<App>,
    Json(body): Json<RefreshRequest>,
) -> Result<impl IntoResponse, Fail> {
    let session = app
        .store
        .refresh_session(&body.refresh_token)
        .map_err(auth_error)?;
    Ok(Json(session))
}

fn validate_registration(body: &RegisterRequest) -> Result<(), Fail> {
    let email = body.email.trim();
    let name = body.name.trim();
    let valid_email = email.len() <= 254
        && !email.chars().any(char::is_whitespace)
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'));
    if !valid_email {
        return Err(Fail::bad_request("enter a valid email address".into()));
    }
    if name.is_empty() || name.len() > 80 {
        return Err(Fail::bad_request(
            "name must be between 1 and 80 characters".into(),
        ));
    }
    if !(8..=1_024).contains(&body.password.len()) {
        return Err(Fail::bad_request(
            "password must be between 8 and 1024 characters".into(),
        ));
    }
    Ok(())
}

fn auth_error(error: AuthError) -> Fail {
    match error {
        AuthError::DuplicateEmail => Fail::conflict("an account already uses that email".into()),
        AuthError::InvalidCredentials => {
            Fail::unauthorized("email or password is incorrect".into())
        }
        AuthError::RateLimited => Fail::too_many("try signing in again later".into()),
        AuthError::Password => Fail::internal("could not secure the password".into()),
        AuthError::Database(error) => Fail::from(error),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceStartRequest {
    machine_name: String,
}

async fn start_device_auth(
    State(app): State<App>,
    Json(body): Json<DeviceStartRequest>,
) -> Result<impl IntoResponse, Fail> {
    let machine_name = body.machine_name.trim();
    if machine_name.is_empty() || machine_name.len() > 80 {
        return Err(Fail::bad_request(
            "machine name must be between 1 and 80 characters".into(),
        ));
    }
    let authorization = app.store.start_device_authorization(machine_name)?;
    Ok(Json(json!({
        "deviceCode": authorization.device_code,
        "userCode": authorization.user_code,
        "expiresAt": authorization.expires_at,
        "intervalSeconds": authorization.interval_seconds,
        "verificationUri": std::env::var("RESHARD_DEVICE_URL")
            .unwrap_or_else(|_| "reshard://device".to_string()),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceApproveRequest {
    user_code: String,
}

async fn approve_device_auth(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    Json(body): Json<DeviceApproveRequest>,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    if !app.store.approve_device(&body.user_code, &user.id)? {
        return Err(Fail::bad_request(
            "that device code is invalid, expired, or already used".into(),
        ));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceTokenRequest {
    device_code: String,
}

async fn poll_device_auth(
    State(app): State<App>,
    Json(body): Json<DeviceTokenRequest>,
) -> Result<Response, Fail> {
    let response = match app.store.poll_device_authorization(&body.device_code)? {
        DevicePoll::Pending { interval_seconds } => (
            StatusCode::ACCEPTED,
            Json(json!({ "status": "pending", "intervalSeconds": interval_seconds })),
        )
            .into_response(),
        DevicePoll::SlowDown { interval_seconds } => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "status": "slowDown", "intervalSeconds": interval_seconds })),
        )
            .into_response(),
        DevicePoll::Authorized { token, machine } => (
            StatusCode::OK,
            Json(json!({ "status": "authorized", "token": token, "machine": machine })),
        )
            .into_response(),
        DevicePoll::Expired => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "expired", "error": "device code expired" })),
        )
            .into_response(),
        DevicePoll::Invalid => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "invalid", "error": "invalid device code" })),
        )
            .into_response(),
    };
    Ok(response)
}

async fn create_pairing(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let workspace = user_authenticated(&app, &headers)?;
    Ok(Json(app.store.create_pairing(&workspace.id)?))
}

#[derive(Deserialize)]
struct Pair {
    code: String,
}

async fn pair(State(app): State<App>, Json(body): Json<Pair>) -> Result<impl IntoResponse, Fail> {
    match app.store.redeem_pairing(&body.code)? {
        Ok(token) => Ok(Json(json!({ "token": token }))),
        Err(error) => Err(Fail::bad_request(error)),
    }
}

async fn machines(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let workspace = user_authenticated(&app, &headers)?;
    let (count, online) = app.store.machine_status(&workspace.id)?;
    let machines = app.store.machines(&workspace.id)?;
    Ok(Json(
        json!({ "count": count, "online": online, "machines": machines }),
    ))
}

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeInventoryRequest {
    runtimes: Vec<RuntimeInventoryItem>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeInventoryItem {
    id: String,
    label: String,
    version: Option<String>,
    availability: String,
    auth: String,
    adapter: String,
    capabilities: RuntimeCapabilitySummary,
    selected: bool,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeCapabilitySummary {
    native_acp: bool,
    adapter_backed: bool,
    subscription_compatible: bool,
    resumable_sessions: bool,
    enforceable_tool_approvals: bool,
    cancellation: bool,
    model_switching: bool,
    maximum_parallelism: u16,
    execution_locus: String,
}

async fn report_runtime_inventory(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RuntimeInventoryRequest>,
) -> Result<impl IntoResponse, Fail> {
    let token = bearer_token(&headers)?;
    machine_authenticated(&app, &headers)?;
    validate_runtime_inventory(&body)?;
    let encoded = serde_json::to_string(&body.runtimes)
        .map_err(|_| Fail::bad_request("runtime inventory is invalid".into()))?;
    if encoded.len() > 128 * 1_024 {
        return Err(Fail::bad_request("runtime inventory is too large".into()));
    }
    if !app.store.update_runtime_inventory(token, &encoded)? {
        return Err(Fail::unauthorized("machine credential is invalid".into()));
    }
    Ok(Json(json!({ "ok": true, "count": body.runtimes.len() })))
}

fn validate_runtime_inventory(body: &RuntimeInventoryRequest) -> Result<(), Fail> {
    if body.runtimes.len() > 32 {
        return Err(Fail::bad_request("too many runtime reports".into()));
    }
    let mut ids = HashSet::new();
    let availability = [
        "ready",
        "loginRequired",
        "adapterMissing",
        "unsupportedVersion",
        "configInvalid",
        "notInstalled",
        "probeFailed",
    ];
    let auth = [
        "loggedIn",
        "loginRequired",
        "unknown",
        "notApplicable",
        "probeFailed",
    ];
    let adapter = ["ready", "missing", "notRequired", "unsupported"];
    let locus = ["localProcess", "remoteDaemon", "externalService"];
    for runtime in &body.runtimes {
        let valid_id = !runtime.id.is_empty()
            && runtime.id.len() <= 64
            && runtime.id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':')
            });
        if !valid_id || !ids.insert(runtime.id.as_str()) {
            return Err(Fail::bad_request(
                "runtime ids are invalid or duplicated".into(),
            ));
        }
        if runtime.label.trim().is_empty()
            || runtime.label.len() > 80
            || runtime
                .version
                .as_ref()
                .is_some_and(|value| value.len() > 200)
            || !availability.contains(&runtime.availability.as_str())
            || !auth.contains(&runtime.auth.as_str())
            || !adapter.contains(&runtime.adapter.as_str())
            || !locus.contains(&runtime.capabilities.execution_locus.as_str())
            || runtime.capabilities.maximum_parallelism > 256
        {
            return Err(Fail::bad_request(
                "runtime report contains invalid fields".into(),
            ));
        }
    }
    Ok(())
}

async fn revoke_machine(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let workspace = user_authenticated(&app, &headers)?;
    if !app.store.revoke_machine(&workspace.id, &id)? {
        return Err(Fail::not_found(format!("no machine {id:?}")));
    }
    broadcast_cancelled(
        &app,
        app.store
            .cancel_approvals_for_machine(&id, "requesting machine was revoked")?,
    );
    Ok(Json(json!({ "ok": true })))
}

async fn heartbeat(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    if !app.store.touch_machine(bearer_token(&headers)?)? {
        return Err(Fail::unauthorized("machine credential is invalid".into()));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn me(State(app): State<App>, headers: axum::http::HeaderMap) -> Result<Json<User>, Fail> {
    let token = bearer_token(&headers)?;
    app.store
        .user_for_token(token)?
        .map(Json)
        .ok_or_else(|| Fail::unauthorized("your session has expired".into()))
}

async fn machine_me(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let token = bearer_token(&headers)?;
    let user = machine_authenticated(&app, &headers)?;
    let machine = app
        .store
        .machine_for_credential(token)?
        .ok_or_else(|| Fail::unauthorized("machine credential is invalid".into()))?;
    Ok(Json(json!({ "user": user, "machine": machine })))
}

async fn machine_logout(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let token = bearer_token(&headers)?;
    machine_authenticated(&app, &headers)?;
    let machine = app
        .store
        .machine_for_credential(token)?
        .ok_or_else(|| Fail::unauthorized("machine credential is invalid".into()))?;
    app.store.revoke_machine_token(token)?;
    broadcast_cancelled(
        &app,
        app.store
            .cancel_approvals_for_machine(&machine.id, "requesting machine signed out")?,
    );
    Ok(Json(json!({ "ok": true })))
}

async fn logout(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let token = bearer_token(&headers)?;
    user_authenticated(&app, &headers)?;
    app.store.revoke_session(token)?;
    Ok(Json(json!({ "ok": true })))
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Result<&str, Fail> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
        .ok_or_else(|| Fail::unauthorized("sign in to continue".into()))
}

fn authenticated(app: &App, headers: &axum::http::HeaderMap) -> Result<User, Fail> {
    let token = bearer_token(headers)?;
    app.store
        .user_for_token(token)?
        .or(app.store.machine_for_token(token)?)
        .ok_or_else(|| Fail::unauthorized("your session has expired".into()))
}

fn user_authenticated(app: &App, headers: &axum::http::HeaderMap) -> Result<User, Fail> {
    let token = bearer_token(headers)?;
    app.store
        .user_for_token(token)?
        .ok_or_else(|| Fail::unauthorized("a device credential is required".into()))
}

fn machine_authenticated(app: &App, headers: &axum::http::HeaderMap) -> Result<User, Fail> {
    let token = bearer_token(headers)?;
    app.store
        .machine_for_token(token)?
        .ok_or_else(|| Fail::unauthorized("a paired machine credential is required".into()))
}

async fn chats(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = authenticated(&app, &headers)?;
    Ok(Json(app.store.chats_for_member(&user.id)?))
}

async fn members(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = authenticated(&app, &headers)?;
    Ok(Json(app.store.members_for_member(&user.id)?))
}

async fn messages(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &id, &user.id)?;
    Ok(Json(app.store.messages(&chat)?))
}

async fn message(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Message>, Fail> {
    let viewer = authenticated(&app, &headers)?;
    let message = app
        .store
        .message(&id)?
        .ok_or_else(|| Fail::not_found(format!("no message {id:?}")))?;
    if !app
        .store
        .is_member_of_chat(&message.channel_id, &viewer.id)?
    {
        return Err(Fail::not_found(format!("no message {id:?}")));
    }
    Ok(Json(message))
}

// ---------------------------------------------------------------------------
// Membership
//
// An agent joins a chat exactly like a person does. The invite does not know
// what will redeem it, so both paths land in the same table and emit the same
// system message.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewChat {
    name: String,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

async fn create_chat(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    Json(body): Json<NewChat>,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    if body.name.trim().is_empty() {
        return Err(Fail::bad_request("a chat needs a name".into()));
    }
    let mut chat = app.store.create_chat(
        body.name.trim(),
        body.topic.as_deref(),
        &user.id,
        now_millis(),
    )?;
    if let Some(agent) = body.agent_id.as_deref() {
        app.store
            .attach_owned_agent(&chat.id, agent, &user.id, now_millis())?
            .ok_or_else(|| Fail::bad_request("that local agent does not belong to you".into()))?;
        chat.member_ids.push(agent.to_string());
    }
    Ok(Json(chat))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatPatch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    avatar_seed: Option<String>,
}

async fn update_chat(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
    Json(body): Json<ChatPatch>,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat_id = resolve_chat_for(&app, &id, &user.id)?;

    let name = body.name.as_deref().map(str::trim);
    if name.is_some_and(str::is_empty) {
        return Err(Fail::bad_request("a chat needs a name".into()));
    }
    if name.is_some_and(|value| value.len() > 80) {
        return Err(Fail::bad_request(
            "chat names are limited to 80 characters".into(),
        ));
    }
    let topic = body.topic.as_deref().map(str::trim);
    if topic.is_some_and(|value| value.len() > 240) {
        return Err(Fail::bad_request(
            "chat topics are limited to 240 characters".into(),
        ));
    }
    let avatar_seed = body.avatar_seed.as_deref().map(str::trim);
    if avatar_seed.is_some_and(|value| value.len() > 128) {
        return Err(Fail::bad_request(
            "avatar seeds are limited to 128 characters".into(),
        ));
    }
    if name.is_none() && topic.is_none() && avatar_seed.is_none() {
        return Err(Fail::bad_request("nothing to update".into()));
    }

    let chat = app
        .store
        .update_chat(&chat_id, name, topic, avatar_seed)?
        .ok_or_else(|| Fail::not_found("no chat matching that id".into()))?;
    let _ = app.events.send(Event::ChatUpdated { chat: chat.clone() });
    Ok(Json(chat))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewInvite {
    chat: String,
    /// Defaults to the whole backlog, the way adding someone to a Slack
    /// channel does.
    #[serde(default)]
    history: HistoryGrant,
}

async fn create_invite(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    Json(body): Json<NewInvite>,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &body.chat, &user.id)?;
    match app
        .store
        .create_invite(&chat, &user.id, body.history, now_millis())?
    {
        Ok(invite) => Ok(Json(invite)),
        Err(why) => Err(Fail::bad_request(why)),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Join {
    code: String,
    /// What the agent will be called in the member list.
    name: String,
    /// The machine redeeming, so a code used by the wrong box is visible.
    #[serde(default)]
    host: Option<String>,
}

async fn connect(
    State(app): State<App>,
    Json(body): Json<Join>,
) -> Result<impl IntoResponse, Fail> {
    let owner = app
        .store
        .invite_inviter(&body.code)?
        .ok_or_else(|| Fail::bad_request("no such invite".into()))?;

    let member = app
        .store
        .upsert_agent(&body.name, &owner, body.host.as_deref())?;

    let membership = match app.store.redeem(&body.code, &member.id, now_millis())? {
        Ok(m) => m,
        Err(why) => return Err(Fail::bad_request(why)),
    };

    let chat = app
        .store
        .chats()?
        .into_iter()
        .find(|c| c.id == membership.chat)
        .ok_or_else(|| Fail::not_found("chat vanished mid-connect".into()))?;

    // Joining is a message. The log stays the truth, and the host that
    // redeemed the code is on the record.
    let line = match &member.host {
        Some(host) => format!("added **{}** · {host}", member.name),
        None => format!("added **{}**", member.name),
    };
    let system = app.store.append(new_message(
        &app,
        &membership.chat,
        &owner,
        MessageKind::System,
        line,
        None,
    )?)?;
    let _ = app.events.send(Event::Message { message: system });

    Ok(Json(json!({
        "member": member,
        "membership": membership,
        "chat": { "id": chat.id, "name": chat.name, "kind": chat.kind },
    })))
}

/// Every chat this member belongs to, with its standing in each. The bridge's
/// whole picture of where an agent may speak.
async fn memberships(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let viewer = authenticated(&app, &headers)?;
    let member = resolve_member(&app, &id)?;
    let visible = app
        .store
        .members_for_member(&viewer.id)?
        .into_iter()
        .any(|candidate| candidate.id == member);
    if !visible {
        return Err(Fail::not_found("no such member".into()));
    }
    let memberships = app
        .store
        .memberships_of(&member)?
        .into_iter()
        .filter(|membership| {
            app.store
                .is_member_of_chat(&membership.chat, &viewer.id)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    Ok(Json(memberships))
}

/// Everything this member may see and has not seen. The window the bridge
/// hands over on the next turn — clamped to the floor, capped, oldest first.
#[derive(Deserialize)]
struct As {
    #[serde(rename = "as")]
    member: String,
    #[serde(default = "default_window")]
    limit: usize,
}

fn default_window() -> usize {
    50
}

async fn unread(
    State(app): State<App>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<As>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let workspace = machine_authenticated(&app, &headers)?;
    let member = resolve_member(&app, &q.member)?;
    let chat = resolve_chat_for(&app, &id, &member)?;
    if !app.store.agent_belongs_to(&member, &workspace.id)?
        || !app.store.is_member_of_chat(&chat, &member)?
    {
        return Err(Fail::not_found("no matching agent membership".into()));
    }
    Ok(Json(app.store.unread(&chat, &member, q.limit)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Read {
    #[serde(rename = "as")]
    member: String,
    /// Omit to mark the whole chat read.
    #[serde(default)]
    seq: Option<i64>,
}

async fn read(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Read>,
) -> Result<impl IntoResponse, Fail> {
    let workspace = machine_authenticated(&app, &headers)?;
    let member = resolve_member(&app, &body.member)?;
    let chat = resolve_chat_for(&app, &id, &member)?;
    if !app.store.agent_belongs_to(&member, &workspace.id)?
        || !app.store.is_member_of_chat(&chat, &member)?
    {
        return Err(Fail::not_found("no matching agent membership".into()));
    }
    let seq = match body.seq {
        Some(seq) => seq,
        None => app.store.head_seq(&chat)?,
    };
    app.store.advance_cursor(&chat, &member, seq)?;
    Ok(Json(
        json!({ "chat": chat, "member": member, "cursorSeq": seq }),
    ))
}

#[derive(Deserialize)]
struct SetTrigger {
    trigger: Trigger,
}

async fn set_trigger(
    State(app): State<App>,
    Path((id, member)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(body): Json<SetTrigger>,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &id, &user.id)?;
    let member = resolve_member(&app, &member)?;
    if !app.store.is_member_of_chat(&chat, &member)? {
        return Err(Fail::not_found("that agent is not in this chat".into()));
    }
    let target = app
        .store
        .members_for_member(&user.id)?
        .into_iter()
        .find(|candidate| candidate.id == member)
        .ok_or_else(|| Fail::not_found("no such agent".into()))?;
    if target.kind != reshard_core::MemberKind::Agent {
        return Err(Fail::bad_request("only agents have wake triggers".into()));
    }
    app.store.set_trigger(&chat, &member, body.trigger)?;
    Ok(Json(json!({ "ok": true })))
}

/// Removing a member is the only revocation there is, and it is the same
/// gesture for humans and agents.
async fn kick(
    State(app): State<App>,
    Path((id, member)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &id, &user.id)?;
    let member_id = resolve_member(&app, &member)?;
    if !app.store.kick(&chat, &member_id)? {
        return Err(Fail::not_found(format!("{member:?} is not in that chat")));
    }

    broadcast_cancelled(
        &app,
        app.store.cancel_approvals_for_agent_chat(
            &member_id,
            &chat,
            "agent was removed from the chat",
        )?,
    );

    let _ = app.events.send(Event::SessionReset {
        chat: chat.clone(),
        member: member_id.clone(),
    });

    let name = app
        .store
        .members()?
        .into_iter()
        .find(|m| m.id == member_id)
        .map(|m| m.name)
        .unwrap_or(member_id);
    let system = app.store.append(new_message(
        &app,
        &chat,
        &user.id,
        MessageKind::System,
        format!("removed **{name}**"),
        None,
    )?)?;
    let _ = app.events.send(Event::Message {
        message: system.clone(),
    });
    Ok(Json(system))
}

async fn reset_session(
    State(app): State<App>,
    Path((id, member)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &id, &user.id)?;
    let member = resolve_member(&app, &member)?;
    if !app.store.is_member_of_chat(&chat, &member)? {
        return Err(Fail::not_found("that agent is not in this chat".into()));
    }
    let target = app
        .store
        .members()?
        .into_iter()
        .find(|candidate| candidate.id == member)
        .ok_or_else(|| Fail::not_found("no such agent".into()))?;
    if target.kind != reshard_core::MemberKind::Agent {
        return Err(Fail::bad_request("only agent sessions can be reset".into()));
    }

    app.store.reset_cursor(&chat, &member)?;
    let _ = app.events.send(Event::SessionReset {
        chat: chat.clone(),
        member: member.clone(),
    });
    Ok(Json(json!({ "ok": true, "chat": chat, "member": member })))
}

/// The single write path. Every client — app, CLI, future bridge — posts here.
async fn command(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    Json(mut cmd): Json<Command>,
) -> Result<impl IntoResponse, Fail> {
    let token = bearer_token(&headers)?;
    broadcast_expired(&app)?;
    let machine_owner = app.store.machine_for_token(token)?;
    if let Some(workspace) = machine_owner {
        let machine = app
            .store
            .machine_for_credential(token)?
            .ok_or_else(|| Fail::unauthorized("machine credential is invalid".into()))?;
        match &mut cmd {
            Command::Send { chat, author, .. } | Command::Status { chat, author, .. } => {
                let author_id = app
                    .store
                    .find_agent_for_owner(author, &workspace.id)?
                    .ok_or_else(|| Fail::not_found(format!("no member matching {author:?}")))?;
                if !app.store.agent_belongs_to(&author_id, &workspace.id)? {
                    return Err(Fail::unauthorized(
                        "agent is not paired to this workspace".into(),
                    ));
                }
                let chat_id = resolve_chat_for(&app, chat, &author_id)?;
                *chat = chat_id;
                *author = author_id;
            }
            Command::RequestApproval {
                agent,
                chat,
                run,
                tool_call,
                provider,
                tool,
                display,
                input_digest,
                expires_in_ms,
            } => {
                let agent_id = app
                    .store
                    .find_agent_for_owner(agent, &workspace.id)?
                    .ok_or_else(|| Fail::not_found(format!("no member matching {agent:?}")))?;
                if !app.store.agent_belongs_to(&agent_id, &workspace.id)? {
                    return Err(Fail::unauthorized(
                        "agent is not paired to this workspace".into(),
                    ));
                }
                let chat_id = resolve_chat_for(&app, chat, &agent_id)?;
                validate_approval_request(
                    run,
                    tool_call,
                    provider,
                    tool,
                    display,
                    input_digest,
                    *expires_in_ms,
                )?;
                let expires_at = now_millis().saturating_add(*expires_in_ms);
                let outcome = app.store.create_approval(
                    &workspace.id,
                    &machine.id,
                    &agent_id,
                    &chat_id,
                    run,
                    tool_call,
                    provider,
                    tool,
                    display,
                    input_digest,
                    expires_at,
                )?;
                return match outcome {
                    ApprovalCreate::Created(approval) => {
                        let event = Event::ApprovalRequested {
                            approval: approval.clone(),
                        };
                        let _ = app.events.send(event.clone());
                        // Room members learn only that the agent is paused;
                        // tool details and the action remain owner-scoped.
                        let _ = app.events.send(Event::Status {
                            chat: approval.chat_id.clone(),
                            author: approval.agent_id.clone(),
                            state: StatusState::Thinking,
                            label: Some("Waiting for owner approval".into()),
                            tool: None,
                            target: None,
                        });
                        schedule_approval_expiry(app.clone(), approval.expires_at);
                        Ok(Json(event))
                    }
                    ApprovalCreate::Existing(approval) => {
                        Ok(Json(Event::ApprovalRequested { approval }))
                    }
                    ApprovalCreate::Conflict => Err(Fail::conflict(
                        "that tool-call id was already used with different input".into(),
                    )),
                    ApprovalCreate::TooMany => Err(Fail::too_many(
                        "this machine already has 100 pending approvals".into(),
                    )),
                };
            }
            Command::CancelApproval { approval, reason } => {
                validate_text("cancellation reason", reason, 200)?;
                return match app.store.cancel_approval(approval, &machine.id, reason)? {
                    ApprovalTransition::Updated(approval) => {
                        let event = Event::ApprovalResolved { approval };
                        let _ = app.events.send(event.clone());
                        Ok(Json(event))
                    }
                    ApprovalTransition::NotFound => {
                        Err(Fail::not_found("no matching approval".into()))
                    }
                    ApprovalTransition::NotPending => {
                        Err(Fail::conflict("approval is already terminal".into()))
                    }
                    ApprovalTransition::Expired(approval) => {
                        let _ = app.events.send(Event::ApprovalExpired { approval });
                        Err(Fail::conflict("approval expired".into()))
                    }
                    ApprovalTransition::DigestMismatch(_) => unreachable!(),
                };
            }
            Command::ResolveApproval { .. } | Command::Ask { .. } | Command::Resolve { .. } => {
                return Err(Fail::unauthorized(
                    "machine credentials may only act as their paired agent".into(),
                ))
            }
        }
        let event = apply(&app, cmd)?;
        let _ = app.events.send(event.clone());
        return Ok(Json(event));
    }
    let user = user_authenticated(&app, &headers)?;
    match &mut cmd {
        Command::Send { chat, author, .. } | Command::Ask { chat, author, .. } => {
            let chat_id = resolve_chat_for(&app, chat, &user.id)?;
            *chat = chat_id;
            *author = user.id;
        }
        Command::Resolve { message, by, .. } => {
            let target = app
                .store
                .message(message)?
                .ok_or_else(|| Fail::not_found(format!("no message {message:?}")))?;
            if !app.store.is_member_of_chat(&target.channel_id, &user.id)? {
                return Err(Fail::not_found(format!("no message {message:?}")));
            }
            *by = user.id;
        }
        Command::Status { .. } => {
            return Err(Fail::bad_request(
                "agent status is not available for user sessions".into(),
            ))
        }
        Command::ResolveApproval {
            approval,
            decision,
            input_digest,
        } => {
            if !valid_digest(input_digest) {
                return Err(Fail::bad_request("input digest must be SHA-256 hex".into()));
            }
            return match app
                .store
                .resolve_approval(approval, &user.id, *decision, input_digest)?
            {
                ApprovalTransition::Updated(approval) => {
                    let event = Event::ApprovalResolved { approval };
                    let _ = app.events.send(event.clone());
                    Ok(Json(event))
                }
                ApprovalTransition::Expired(approval) => {
                    let _ = app.events.send(Event::ApprovalExpired { approval });
                    Err(Fail::conflict("approval expired".into()))
                }
                ApprovalTransition::DigestMismatch(approval) => {
                    let _ = app.events.send(Event::ApprovalResolved { approval });
                    Err(Fail::conflict(
                        "approval input changed; request denied closed".into(),
                    ))
                }
                ApprovalTransition::NotPending => {
                    Err(Fail::conflict("approval is already terminal".into()))
                }
                ApprovalTransition::NotFound => Err(Fail::not_found("no matching approval".into())),
            };
        }
        Command::RequestApproval { .. } | Command::CancelApproval { .. } => {
            return Err(Fail::unauthorized(
                "only a paired machine may manage provider requests".into(),
            ))
        }
    }
    let event = apply(&app, cmd)?;
    // A receiver-less broadcast is not an error: nobody is listening yet.
    let _ = app.events.send(event.clone());
    Ok(Json(event))
}

fn apply(app: &App, cmd: Command) -> Result<Event, Fail> {
    match cmd {
        Command::Send {
            chat,
            author,
            text,
            idem: _,
        } => {
            let message = app.store.append(new_message(
                app,
                &chat,
                &author,
                MessageKind::Text,
                text,
                None,
            )?)?;
            Ok(Event::Message { message })
        }

        Command::Ask {
            chat,
            author,
            text,
            options,
        } => {
            let message = app.store.append(new_message(
                app,
                &chat,
                &author,
                MessageKind::Ask,
                text,
                Some(options),
            )?)?;
            Ok(Event::Message { message })
        }

        Command::Resolve {
            message,
            option,
            by: _,
        } => {
            let message = app
                .store
                .resolve(&message, &option)?
                .ok_or_else(|| Fail::not_found(format!("no message {message:?}")))?;
            Ok(Event::MessageUpdated { message })
        }

        // Ephemeral: broadcast, never stored.
        Command::Status {
            chat,
            author,
            state,
            label,
            tool,
            target,
        } => {
            let chat = resolve_chat(app, &chat)?;
            let author = resolve_member(app, &author)?;
            Ok(Event::Status {
                chat,
                author,
                state,
                label,
                tool,
                target,
            })
        }

        Command::RequestApproval { .. }
        | Command::ResolveApproval { .. }
        | Command::CancelApproval { .. } => Err(Fail::internal(
            "approval commands must be authorized before application".into(),
        )),
    }
}

async fn approvals(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<Approval>>, Fail> {
    let owner = user_authenticated(&app, &headers)?;
    broadcast_expired(&app)?;
    Ok(Json(app.store.approvals_for_owner(&owner.id)?))
}

/// Owner-scoped approval audit trail (query/export). Human session only; a
/// machine credential cannot read another owner's audit history.
async fn approval_audit(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<AuditEntry>>, Fail> {
    let owner = user_authenticated(&app, &headers)?;
    broadcast_expired(&app)?;
    Ok(Json(app.store.approval_audit_for_owner(&owner.id, 1000)?))
}

/// Owner-scoped operational metrics: approval counts by state.
async fn approval_metrics(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ApprovalMetrics>, Fail> {
    let owner = user_authenticated(&app, &headers)?;
    Ok(Json(app.store.approval_metrics_for_owner(&owner.id)?))
}

async fn approval(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Approval>, Fail> {
    let token = bearer_token(&headers)?;
    broadcast_expired(&app)?;
    if let Some(owner) = app.store.user_for_token(token)? {
        return app
            .store
            .approval_for_owner(&id, &owner.id)?
            .map(Json)
            .ok_or_else(|| Fail::not_found("no matching approval".into()));
    }
    if app.store.machine_for_token(token)?.is_some() {
        let machine = app
            .store
            .machine_for_credential(token)?
            .ok_or_else(|| Fail::unauthorized("machine credential is invalid".into()))?;
        return app
            .store
            .approval_for_machine(&id, &machine.id)?
            .map(Json)
            .ok_or_else(|| Fail::not_found("no matching approval".into()));
    }
    Err(Fail::unauthorized("your session has expired".into()))
}

fn validate_approval_request(
    run: &str,
    tool_call: &str,
    provider: &str,
    tool: &str,
    display: &reshard_core::ApprovalDisplay,
    digest: &str,
    expires_in_ms: i64,
) -> Result<(), Fail> {
    validate_identifier("run id", run, 128)?;
    validate_identifier("tool-call id", tool_call, 128)?;
    validate_text("provider", provider, 64)?;
    validate_text("tool", tool, 80)?;
    validate_text("approval summary", &display.summary, 500)?;
    for (name, value, max) in [
        ("project", display.project.as_deref(), 300),
        ("target", display.target.as_deref(), 500),
        ("command preview", display.command.as_deref(), 1_000),
    ] {
        if let Some(value) = value {
            validate_text(name, value, max)?;
        }
    }
    if !valid_digest(digest) {
        return Err(Fail::bad_request("input digest must be SHA-256 hex".into()));
    }
    if !(1_000..=24 * 60 * 60 * 1_000).contains(&expires_in_ms) {
        return Err(Fail::bad_request(
            "approval expiry must be between 1 second and 24 hours".into(),
        ));
    }
    Ok(())
}

fn validate_identifier(name: &str, value: &str, max: usize) -> Result<(), Fail> {
    validate_text(name, value, max)?;
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
    {
        return Err(Fail::bad_request(format!(
            "{name} contains invalid characters"
        )));
    }
    Ok(())
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<(), Fail> {
    let unsafe_character = |character: char| {
        character.is_control()
            || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    };
    if value.trim().is_empty() || value.len() > max || value.chars().any(unsafe_character) {
        return Err(Fail::bad_request(format!("{name} is invalid")));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn broadcast_expired(app: &App) -> Result<(), Fail> {
    for approval in app.store.expire_approvals()? {
        let _ = app.events.send(Event::ApprovalExpired { approval });
    }
    Ok(())
}

fn broadcast_cancelled(app: &App, approvals: Vec<Approval>) {
    for approval in approvals {
        let _ = app.events.send(Event::ApprovalResolved { approval });
    }
}

fn schedule_approval_expiry(app: App, expires_at: i64) {
    tokio::spawn(async move {
        let delay = expires_at.saturating_sub(now_millis()) as u64;
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        let _ = broadcast_expired(&app);
    });
}

async fn expiry_sweeper(app: App) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if let Err(error) = broadcast_expired(&app) {
            eprintln!("approval expiry sweep failed: {}", error.message);
        }
    }
}

fn new_message(
    app: &App,
    chat: &str,
    author: &str,
    kind: MessageKind,
    text: String,
    options: Option<Vec<String>>,
) -> Result<Message, Fail> {
    Ok(Message {
        id: format!("m_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
        channel_id: resolve_chat(app, chat)?,
        author_id: resolve_member(app, author)?,
        seq: 0, // assigned by the store, under the same lock as the insert
        kind,
        text,
        options,
        resolved_option: None,
        created_at: now_millis(),
    })
}

fn resolve_chat(app: &App, needle: &str) -> Result<String, Fail> {
    app.store
        .find_chat(needle)?
        .ok_or_else(|| Fail::not_found(format!("no chat matching {needle:?}")))
}

fn resolve_chat_for(app: &App, needle: &str, member: &str) -> Result<String, Fail> {
    app.store
        .find_chat_for_member(needle, member)?
        .ok_or_else(|| Fail::not_found(format!("no chat matching {needle:?}")))
}

fn resolve_member(app: &App, needle: &str) -> Result<String, Fail> {
    app.store
        .find_member(needle)?
        .ok_or_else(|| Fail::not_found(format!("no member matching {needle:?}")))
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ---------------------------------------------------------------------------
// The stream
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StreamQuery {
    token: String,
}

#[derive(Clone)]
enum ConnectionIdentity {
    Human(User),
    Machine { owner: User, machine: MachineRecord },
}

impl ConnectionIdentity {
    fn member_id(&self) -> &str {
        match self {
            Self::Human(user) => &user.id,
            Self::Machine { owner, .. } => &owner.id,
        }
    }

    fn can_receive_approval(&self, approval: &Approval) -> bool {
        match self {
            Self::Human(user) => approval.owner_id == user.id,
            Self::Machine { machine, .. } => approval.machine_id == machine.id,
        }
    }
}

async fn stream(
    ws: WebSocketUpgrade,
    State(app): State<App>,
    axum::extract::Query(query): axum::extract::Query<StreamQuery>,
) -> Result<Response, Fail> {
    let identity = if let Some(user) = app.store.user_for_token(&query.token)? {
        ConnectionIdentity::Human(user)
    } else if let Some(owner) = app.store.machine_for_token(&query.token)? {
        let machine = app
            .store
            .machine_for_credential(&query.token)?
            .ok_or_else(|| Fail::unauthorized("machine credential is invalid".into()))?;
        ConnectionIdentity::Machine { owner, machine }
    } else {
        return Err(Fail::unauthorized("your session has expired".into()));
    };
    Ok(ws.on_upgrade(move |socket| pump(socket, app, identity, query.token)))
}

/// One task per connected client: forward every event for as long as it lives.
async fn pump(socket: WebSocket, app: App, identity: ConnectionIdentity, token: String) {
    let (mut tx, mut rx) = socket.split();
    let mut events = app.events.subscribe();
    let mut auth_check = tokio::time::interval(std::time::Duration::from_secs(30));
    auth_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // `interval` ticks immediately once. Authentication was just checked by
    // the handshake, so begin the recurring checks 30 seconds from now.
    auth_check.tick().await;

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    let permitted = match &event {
                        Event::ApprovalRequested { approval }
                        | Event::ApprovalResolved { approval }
                        | Event::ApprovalExpired { approval } => {
                            identity.can_receive_approval(approval)
                        }
                        Event::Message { message } | Event::MessageUpdated { message } => {
                            app.store.is_member_of_chat(&message.channel_id, identity.member_id()).unwrap_or(false)
                        }
                        Event::ChatUpdated { chat } => app.store.is_member_of_chat(&chat.id, identity.member_id()).unwrap_or(false),
                        Event::Status { chat, .. } | Event::SessionReset { chat, .. } => {
                            app.store.is_member_of_chat(chat, identity.member_id()).unwrap_or(false)
                        }
                    };
                    if !permitted {
                        continue;
                    }
                    let Ok(text) = serde_json::to_string(&event) else {
                        continue;
                    };
                    if tx.send(WsMessage::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                // A lagging client loses the oldest frames rather than the
                // connection; it resyncs on its next fetch.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            message = rx.next() => match message {
                Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            _ = auth_check.tick() => {
                let valid = match &identity {
                    ConnectionIdentity::Human(_) => app.store.user_for_token(&token).ok().flatten().is_some(),
                    ConnectionIdentity::Machine { machine, .. } => app.store
                        .machine_for_credential(&token)
                        .ok()
                        .flatten()
                        .is_some_and(|current| current.id == machine.id),
                };
                if !valid {
                    let _ = tx.send(WsMessage::Close(Some(CloseFrame {
                        code: close_code::POLICY,
                        reason: "session expired or revoked".into(),
                    }))).await;
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Fail {
    status: StatusCode,
    message: String,
}

impl Fail {
    fn not_found(message: String) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message,
        }
    }

    /// A refused invite is the caller's problem, not a server fault — an
    /// expired or already-used code must read differently from a crash.
    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }

    fn unauthorized(message: String) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message,
        }
    }

    fn conflict(message: String) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message,
        }
    }

    fn too_many(message: String) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message,
        }
    }

    fn internal(message: String) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
        }
    }
}

impl From<rusqlite::Error> for Fail {
    fn from(err: rusqlite::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        }
    }
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;

    #[test]
    fn bootstrap_is_off_in_release_unless_explicitly_enabled() {
        assert!(!bootstrap_enabled_for(None, false));
        assert!(!bootstrap_enabled_for(Some("0"), true));
        assert!(bootstrap_enabled_for(Some("1"), false));
    }

    struct Fixture {
        app: Router,
        user_a: String,
        user_b: String,
        machine_a: String,
        machine_b: String,
        chat_a: String,
        message_a: String,
        agent_a: String,
        agent_b: String,
    }

    fn fixture() -> Fixture {
        let store = Arc::new(Store::open(":memory:").unwrap());
        let session_a = store.bootstrap_workspace().unwrap();
        let session_b = store.bootstrap_workspace().unwrap();
        let chat_a = store
            .create_chat("same-name", None, &session_a.user.id, now_millis())
            .unwrap();
        let chat_b = store
            .create_chat("same-name", None, &session_b.user.id, now_millis())
            .unwrap();
        let (events, _) = broadcast::channel(32);
        let state = App {
            store: store.clone(),
            events,
        };
        let message_a = store
            .append(
                new_message(
                    &state,
                    &chat_a.id,
                    &session_a.user.id,
                    MessageKind::Text,
                    "workspace a only".into(),
                    None,
                )
                .unwrap(),
            )
            .unwrap();

        let invite_a = store
            .create_invite(
                &chat_a.id,
                &session_a.user.id,
                HistoryGrant::All,
                now_millis(),
            )
            .unwrap()
            .unwrap();
        let agent_a = store
            .upsert_agent("same-agent", &session_a.user.id, Some("a-host"))
            .unwrap();
        store
            .redeem(&invite_a.code, &agent_a.id, now_millis())
            .unwrap()
            .unwrap();

        let invite_b = store
            .create_invite(
                &chat_b.id,
                &session_b.user.id,
                HistoryGrant::All,
                now_millis(),
            )
            .unwrap()
            .unwrap();
        let agent_b = store
            .upsert_agent("same-agent", &session_b.user.id, Some("b-host"))
            .unwrap();
        store
            .redeem(&invite_b.code, &agent_b.id, now_millis())
            .unwrap()
            .unwrap();

        let pairing_a = store.create_pairing(&session_a.user.id).unwrap();
        let machine_a = store.redeem_pairing(&pairing_a.code).unwrap().unwrap();
        let pairing_b = store.create_pairing(&session_b.user.id).unwrap();
        let machine_b = store.redeem_pairing(&pairing_b.code).unwrap().unwrap();

        Fixture {
            app: router(state),
            user_a: session_a.token,
            user_b: session_b.token,
            machine_a,
            machine_b,
            chat_a: chat_a.id,
            message_a: message_a.id,
            agent_a: agent_a.id,
            agent_b: agent_b.id,
        }
    }

    fn request(method: Method, uri: &str, token: Option<&str>, body: &str) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if !body.is_empty() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        builder.body(Body::from(body.to_owned())).unwrap()
    }

    fn isolated_app() -> Router {
        let store = Arc::new(Store::open(":memory:").unwrap());
        let (events, _) = broadcast::channel(32);
        router(App { store, events })
    }

    async fn response_json(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn account_sessions_register_login_refresh_and_revoke() {
        let app = isolated_app();
        let registered = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/auth/register",
                None,
                r#"{"email":"Person@Example.com","name":"Person","password":"long enough password"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(registered.status(), StatusCode::OK);
        let registered = response_json(registered).await;
        let first_token = registered["token"].as_str().unwrap();
        let refresh_token = registered["refreshToken"].as_str().unwrap();

        let me = app
            .clone()
            .oneshot(request(Method::GET, "/auth/me", Some(first_token), ""))
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);

        let wrong_password = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/auth/login",
                None,
                r#"{"email":"person@example.com","password":"incorrect password"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(wrong_password.status(), StatusCode::UNAUTHORIZED);

        let refreshed = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/auth/refresh",
                None,
                &serde_json::json!({ "refreshToken": refresh_token }).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(refreshed.status(), StatusCode::OK);
        let refreshed = response_json(refreshed).await;
        let second_token = refreshed["token"].as_str().unwrap();
        assert_ne!(first_token, second_token);

        let old_session = app
            .clone()
            .oneshot(request(Method::GET, "/auth/me", Some(first_token), ""))
            .await
            .unwrap();
        assert_eq!(old_session.status(), StatusCode::UNAUTHORIZED);

        let logout = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/auth/logout",
                Some(second_token),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::OK);
        let revoked = app
            .oneshot(request(Method::GET, "/auth/me", Some(second_token), ""))
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn device_flow_binds_machine_to_approver_and_owner_can_revoke_it() {
        let app = isolated_app();
        let owner = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/auth/register",
                None,
                r#"{"email":"owner@example.com","name":"Owner","password":"owner password"}"#,
            ))
            .await
            .unwrap();
        let owner = response_json(owner).await;
        let owner_token = owner["token"].as_str().unwrap();

        let outsider = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/auth/register",
                None,
                r#"{"email":"outsider@example.com","name":"Outsider","password":"outsider password"}"#,
            ))
            .await
            .unwrap();
        let outsider = response_json(outsider).await;
        let outsider_token = outsider["token"].as_str().unwrap();

        let started = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/auth/device/start",
                None,
                r#"{"machineName":"Build Mac"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(started.status(), StatusCode::OK);
        let started = response_json(started).await;

        let approved = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/auth/device/approve",
                Some(owner_token),
                &serde_json::json!({ "userCode": started["userCode"] }).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(approved.status(), StatusCode::OK);

        let authorized = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/auth/device/token",
                None,
                &serde_json::json!({ "deviceCode": started["deviceCode"] }).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::OK);
        let authorized = response_json(authorized).await;
        let machine_token = authorized["token"].as_str().unwrap();
        let machine_id = authorized["machine"]["id"].as_str().unwrap();

        let machine_me = app
            .clone()
            .oneshot(request(
                Method::GET,
                "/auth/machine",
                Some(machine_token),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(machine_me.status(), StatusCode::OK);
        let machine_me = response_json(machine_me).await;
        assert_eq!(machine_me["user"]["email"], "owner@example.com");

        let inventory = serde_json::json!({
            "runtimes": [{
                "id": "claude",
                "label": "Claude Code",
                "version": "2.0.0",
                "availability": "ready",
                "auth": "loggedIn",
                "adapter": "ready",
                "capabilities": {
                    "nativeAcp": false,
                    "adapterBacked": true,
                    "subscriptionCompatible": true,
                    "resumableSessions": true,
                    "enforceableToolApprovals": true,
                    "cancellation": true,
                    "modelSwitching": true,
                    "maximumParallelism": 1,
                    "executionLocus": "localProcess"
                },
                "selected": true
            }]
        })
        .to_string();
        let reported = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/machines/runtimes",
                Some(machine_token),
                &inventory,
            ))
            .await
            .unwrap();
        assert_eq!(reported.status(), StatusCode::OK);

        let mut leaked_inventory: serde_json::Value = serde_json::from_str(&inventory).unwrap();
        leaked_inventory["runtimes"][0]["binaryPath"] = json!("/private/project/provider");
        let leaked = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/machines/runtimes",
                Some(machine_token),
                &leaked_inventory.to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(leaked.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let human_cannot_report = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/machines/runtimes",
                Some(owner_token),
                &inventory,
            ))
            .await
            .unwrap();
        assert_eq!(human_cannot_report.status(), StatusCode::UNAUTHORIZED);

        let owner_machines = app
            .clone()
            .oneshot(request(Method::GET, "/machines", Some(owner_token), ""))
            .await
            .unwrap();
        let owner_machines = response_json(owner_machines).await;
        assert_eq!(owner_machines["machines"][0]["runtimes"][0]["id"], "claude");
        assert!(owner_machines["machines"][0]["runtimes"][0]
            .get("binaryPath")
            .is_none());

        let outsider_revoke = app
            .clone()
            .oneshot(request(
                Method::DELETE,
                &format!("/machines/{machine_id}"),
                Some(outsider_token),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(outsider_revoke.status(), StatusCode::NOT_FOUND);

        let owner_revoke = app
            .clone()
            .oneshot(request(
                Method::DELETE,
                &format!("/machines/{machine_id}"),
                Some(owner_token),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(owner_revoke.status(), StatusCode::OK);

        let revoked_machine = app
            .oneshot(request(
                Method::GET,
                "/auth/machine",
                Some(machine_token),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(revoked_machine.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn messages_are_private_to_chat_members() {
        let f = fixture();
        let anonymous = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/messages/{}", f.message_a),
                None,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

        let outsider = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/messages/{}", f.message_a),
                Some(&f.user_b),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(outsider.status(), StatusCode::NOT_FOUND);

        let member = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/messages/{}", f.message_a),
                Some(&f.user_a),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn same_named_chats_and_agents_do_not_cross_workspaces() {
        let f = fixture();
        assert_ne!(f.agent_a, f.agent_b);

        let own = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                "/chats/same-name/messages",
                Some(&f.user_a),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(own.status(), StatusCode::OK);

        let hidden_memberships = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/members/{}/memberships", f.agent_a),
                Some(&f.user_b),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(hidden_memberships.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn only_the_paired_machine_can_advance_its_agent() {
        let f = fixture();
        let uri = format!("/chats/{}/unread?as={}", f.chat_a, f.agent_a);

        let user_impersonation = f
            .app
            .clone()
            .oneshot(request(Method::GET, &uri, Some(&f.user_a), ""))
            .await
            .unwrap();
        assert_eq!(user_impersonation.status(), StatusCode::UNAUTHORIZED);

        let wrong_machine = f
            .app
            .clone()
            .oneshot(request(Method::GET, &uri, Some(&f.machine_b), ""))
            .await
            .unwrap();
        assert_eq!(wrong_machine.status(), StatusCode::NOT_FOUND);

        let paired_machine = f
            .app
            .clone()
            .oneshot(request(Method::GET, &uri, Some(&f.machine_a), ""))
            .await
            .unwrap();
        assert_eq!(paired_machine.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn machines_cannot_mutate_user_owned_chat_settings() {
        let f = fixture();
        let response = f
            .app
            .clone()
            .oneshot(request(
                Method::PATCH,
                &format!("/chats/{}", f.chat_a),
                Some(&f.machine_a),
                r#"{"name":"hijacked"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn tool_approvals_are_owner_and_exact_machine_scoped() {
        let f = fixture();
        let digest = "a".repeat(64);
        let opened = f
            .app
            .clone()
            .oneshot(request(
                Method::POST,
                "/commands",
                Some(&f.machine_a),
                &json!({
                    "t": "requestApproval",
                    "agent": f.agent_a,
                    "chat": f.chat_a,
                    "run": "run-e2e",
                    "toolCall": "call-e2e",
                    "provider": "fake",
                    "tool": "FakeWrite",
                    "display": { "summary": "Write fixture?", "target": "fixture.txt" },
                    "inputDigest": digest,
                    "expiresInMs": 60_000
                })
                .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(opened.status(), StatusCode::OK);
        let opened = response_json(opened).await;
        let approval_id = opened["approval"]["id"].as_str().unwrap();

        for (token, expected) in [
            (&f.user_b, StatusCode::NOT_FOUND),
            (&f.machine_b, StatusCode::NOT_FOUND),
        ] {
            let hidden = f
                .app
                .clone()
                .oneshot(request(
                    Method::GET,
                    &format!("/approvals/{approval_id}"),
                    Some(token),
                    "",
                ))
                .await
                .unwrap();
            assert_eq!(hidden.status(), expected);
        }

        let outsider_list = f
            .app
            .clone()
            .oneshot(request(Method::GET, "/approvals", Some(&f.user_b), ""))
            .await
            .unwrap();
        assert_eq!(response_json(outsider_list).await, json!([]));

        let wrong_owner = f
            .app
            .clone()
            .oneshot(request(
                Method::POST,
                "/commands",
                Some(&f.user_b),
                &json!({
                    "t": "resolveApproval",
                    "approval": approval_id,
                    "decision": "allowOnce",
                    "inputDigest": digest
                })
                .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(wrong_owner.status(), StatusCode::NOT_FOUND);

        let allowed = f
            .app
            .clone()
            .oneshot(request(
                Method::POST,
                "/commands",
                Some(&f.user_a),
                &json!({
                    "t": "resolveApproval",
                    "approval": approval_id,
                    "decision": "allowOnce",
                    "inputDigest": digest
                })
                .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);
        assert_eq!(response_json(allowed).await["approval"]["state"], "allowed");

        let duplicate = f
            .app
            .clone()
            .oneshot(request(
                Method::POST,
                "/commands",
                Some(&f.user_a),
                &json!({
                    "t": "resolveApproval",
                    "approval": approval_id,
                    "decision": "allowOnce",
                    "inputDigest": digest
                })
                .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);

        let machine_recovery = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/approvals/{approval_id}"),
                Some(&f.machine_a),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(machine_recovery.status(), StatusCode::OK);
        assert_eq!(response_json(machine_recovery).await["state"], "allowed");
    }

    #[test]
    fn approval_push_details_route_only_to_owner_and_requesting_machine() {
        let approval = Approval {
            id: "apr_1".into(),
            owner_id: "u_owner".into(),
            machine_id: "machine_owner".into(),
            agent_id: "a_agent".into(),
            chat_id: "g_shared".into(),
            run_id: "run_1".into(),
            tool_call_id: "call_1".into(),
            provider: "fake".into(),
            tool: "FakeWrite".into(),
            display: reshard_core::ApprovalDisplay {
                summary: "Write fixture?".into(),
                project: None,
                target: Some("fixture.txt".into()),
                command: None,
            },
            input_digest: "a".repeat(64),
            state: reshard_core::ApprovalState::Pending,
            expires_at: 10,
            created_at: 0,
            resolved_at: None,
            resolved_by: None,
            resolution_reason: None,
        };
        let user = |id: &str| User {
            id: id.into(),
            email: format!("{id}@example.com"),
            name: id.into(),
        };
        let machine = |id: &str| MachineRecord {
            id: id.into(),
            name: id.into(),
            created_at: 0,
            last_seen: None,
            online: true,
            runtimes: vec![],
            runtime_updated_at: None,
        };
        assert!(ConnectionIdentity::Human(user("u_owner")).can_receive_approval(&approval));
        assert!(!ConnectionIdentity::Human(user("u_room_member")).can_receive_approval(&approval));
        assert!(ConnectionIdentity::Machine {
            owner: user("u_owner"),
            machine: machine("machine_owner"),
        }
        .can_receive_approval(&approval));
        assert!(!ConnectionIdentity::Machine {
            owner: user("u_owner"),
            machine: machine("machine_other"),
        }
        .can_receive_approval(&approval));
    }

    #[test]
    fn approval_audit_and_metrics_are_owner_scoped() {
        let store = Store::open(":memory:").unwrap();
        let display = reshard_core::ApprovalDisplay {
            summary: "Run command?".into(),
            project: None,
            target: Some("test.txt".into()),
            command: Some("rm test.txt".into()),
        };
        let digest = "a".repeat(64);

        // Seed two independent owners, each with an agent, chat, and machine.
        let seed = |name: &str| {
            let session = store.bootstrap_workspace().unwrap();
            let chat = store
                .create_chat(name, None, &session.user.id, now_millis())
                .unwrap();
            let invite = store
                .create_invite(&chat.id, &session.user.id, HistoryGrant::All, now_millis())
                .unwrap()
                .unwrap();
            let agent = store
                .upsert_agent(name, &session.user.id, Some("host"))
                .unwrap();
            store
                .redeem(&invite.code, &agent.id, now_millis())
                .unwrap()
                .unwrap();
            let pairing = store.create_pairing(&session.user.id).unwrap();
            let machine = store.redeem_pairing(&pairing.code).unwrap().unwrap();
            (session.user.id, agent.id, chat.id, machine)
        };
        let (owner_a, agent_a, chat_a, machine_a) = seed("owner-a");
        let (owner_b, agent_b, chat_b, machine_b) = seed("owner-b");

        // Owner A: one approval, allowed. Owner B: one approval, left pending.
        let a = match store
            .create_approval(
                &owner_a,
                &machine_a,
                &agent_a,
                &chat_a,
                "run_a",
                "call_a",
                "fake",
                "Bash",
                &display,
                &digest,
                now_millis() + 60_000,
            )
            .unwrap()
        {
            ApprovalCreate::Created(a) => a,
            _ => panic!("expected Created"),
        };
        store
            .resolve_approval(&a.id, &owner_a, reshard_core::ApprovalDecision::AllowOnce, &digest)
            .unwrap();
        match store
            .create_approval(
                &owner_b,
                &machine_b,
                &agent_b,
                &chat_b,
                "run_b",
                "call_b",
                "fake",
                "Bash",
                &display,
                &digest,
                now_millis() + 60_000,
            )
            .unwrap()
        {
            ApprovalCreate::Created(_) => {}
            _ => panic!("expected Created"),
        };

        // Audit is owner-scoped: A sees only its own approval's transitions
        // (create -> pending, then resolve -> allowed), newest first.
        let audit_a = store.approval_audit_for_owner(&owner_a, 100).unwrap();
        assert_eq!(audit_a.len(), 2, "{audit_a:#?}");
        assert!(audit_a.iter().all(|entry| entry.approval_id == a.id));
        assert_eq!(audit_a[0].to_state, "allowed");
        assert_eq!(audit_a[0].tool, "Bash");
        // B's transitions never leak into A's view.
        let audit_b = store.approval_audit_for_owner(&owner_b, 100).unwrap();
        assert!(audit_b.iter().all(|entry| entry.approval_id != a.id));

        // Metrics are owner-scoped.
        let metrics_a = store.approval_metrics_for_owner(&owner_a).unwrap();
        assert_eq!(metrics_a.allowed, 1);
        assert_eq!(metrics_a.pending, 0);
        assert_eq!(metrics_a.total, 1);
        let metrics_b = store.approval_metrics_for_owner(&owner_b).unwrap();
        assert_eq!(metrics_b.pending, 1);
        assert_eq!(metrics_b.allowed, 0);
        assert_eq!(metrics_b.total, 1);
    }

    #[tokio::test]
    async fn trigger_changes_require_a_chat_member_user() {
        let f = fixture();
        let uri = format!("/chats/{}/members/{}", f.chat_a, f.agent_a);

        let outsider = f
            .app
            .clone()
            .oneshot(request(
                Method::POST,
                &uri,
                Some(&f.user_b),
                r#"{"trigger":"mention"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(outsider.status(), StatusCode::NOT_FOUND);

        let member = f
            .app
            .clone()
            .oneshot(request(
                Method::POST,
                &uri,
                Some(&f.user_a),
                r#"{"trigger":"mention"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cors_allows_tauri_and_rejects_random_websites() {
        let f = fixture();
        let preflight = |origin: &'static str| {
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/chats")
                .header(header::ORIGIN, origin)
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
                .body(Body::empty())
                .unwrap()
        };

        let allowed = f
            .app
            .clone()
            .oneshot(preflight("http://localhost:1420"))
            .await
            .unwrap();
        assert_eq!(
            allowed.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&HeaderValue::from_static("http://localhost:1420"))
        );

        let rejected = f
            .app
            .clone()
            .oneshot(preflight("https://malicious.example"))
            .await
            .unwrap();
        assert!(rejected
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }
}
