use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, DefaultBodyLimit, Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, RwLock, Semaphore};
use uuid::Uuid;

use crate::{
    collect_lint_warnings, render_scene_to_file,
    schema::Scene,
    security::{validate_remote_url, validate_scene_for_service, ServiceLimits},
};

#[derive(Clone)]
pub struct ServeConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
    pub output_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub ttl_secs: u64,
    pub rate_limit_per_min: usize,
    pub api_key: Option<String>,
    pub limits: ServiceLimits,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum JobStatus {
    Queued,
    Processing,
    Completed,
    Failed,
}

#[derive(Clone)]
struct JobRecord {
    id: String,
    status: JobStatus,
    created_at: Instant,
    finished_at: Option<Instant>,
    render_secs: Option<f32>,
    output_path: Option<PathBuf>,
    error: Option<String>,
}

struct QueuedJob {
    id: String,
    scene: Scene,
    webhook_url: Option<String>,
}

#[derive(Clone)]
struct AppState {
    config: ServeConfig,
    jobs: Arc<RwLock<HashMap<String, JobRecord>>>,
    rate_limits: Arc<RwLock<HashMap<IpAddr, Vec<Instant>>>>,
    tx: mpsc::Sender<QueuedJob>,
    semaphore: Arc<Semaphore>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SubmitJobPayload {
    Wrapped {
        scene: Scene,
        #[serde(default)]
        composition: Option<String>,
        #[serde(default)]
        webhook_url: Option<String>,
    },
    Direct(Scene),
}

#[derive(Serialize)]
struct SubmitJobResponse {
    job_id: String,
    status: JobStatus,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct JobStatusResponse {
    job_id: String,
    status: JobStatus,
    render_secs: Option<f32>,
    video_url: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    workers: usize,
    active_jobs: usize,
    total_tracked_jobs: usize,
}

#[derive(Serialize)]
struct ValidateResponse {
    valid: bool,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

const EMBEDDED_SCHEMA_JSON: &str = include_str!("../schema.json");
const EMBEDDED_GUIDE_MD: &str = include_str!("../SCHEMA.md");

fn extract_clean_json(raw: &str) -> String {
    let no_bom = raw.trim_start_matches('\u{FEFF}').trim();
    if serde_json::from_str::<serde_json::Value>(no_bom).is_ok() {
        return no_bom.to_string();
    }

    // Strip markdown code fences or chatbot intro/outro text by extracting outermost {...}
    if let (Some(start), Some(end)) = (no_bom.find('{'), no_bom.rfind('}'))
        && end >= start {
            let sliced = &no_bom[start..=end];
            if serde_json::from_str::<serde_json::Value>(sliced).is_ok() {
                return sliced.to_string();
            }
            let normalized = sliced
                .replace(['\u{201C}', '\u{201D}'], "\"")
                .replace(['\u{2018}', '\u{2019}'], "'");
            return normalized;
        }

    no_bom.to_string()
}

fn api_error(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ErrorResponse { error: msg.into() })).into_response()
}

fn constant_time_eq_str(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

fn check_auth(headers: &HeaderMap, expected: &Option<String>) -> Result<(), Response> {
    let Some(secret) = expected else {
        return Ok(());
    };
    let bearer_ok = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .is_some_and(|t| constant_time_eq_str(t, secret));
    let header_ok = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| constant_time_eq_str(t, secret));

    if bearer_ok || header_ok {
        Ok(())
    } else {
        Err(api_error(StatusCode::UNAUTHORIZED, "Invalid or missing API key"))
    }
}

const MAX_RATE_LIMIT_IPS: usize = 10_000;

async fn check_rate_limit(state: &AppState, ip: IpAddr) -> Result<(), Response> {
    let mut map = state.rate_limits.write().await;
    let now = Instant::now();
    let window = Duration::from_secs(60);

    if map.len() >= MAX_RATE_LIMIT_IPS && !map.contains_key(&ip) {
        map.retain(|_, ts| {
            ts.retain(|t| now.duration_since(*t) < window);
            !ts.is_empty()
        });
        if map.len() >= MAX_RATE_LIMIT_IPS {
            return Err(api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit table full, please retry later",
            ));
        }
    }

    let timestamps = map.entry(ip).or_default();
    timestamps.retain(|t| now.duration_since(*t) < window);
    if timestamps.len() >= state.config.rate_limit_per_min {
        return Err(api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded, please retry later",
        ));
    }
    timestamps.push(now);
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let jobs = state.jobs.read().await;
    let active_jobs = jobs
        .values()
        .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Processing))
        .count();
    Json(HealthResponse {
        status: "ok",
        workers: state.config.workers,
        active_jobs,
        total_tracked_jobs: jobs.len(),
    })
}

async fn get_schema() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(EMBEDDED_SCHEMA_JSON))
        .unwrap_or_else(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Response error"))
}

async fn get_guide() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
        .body(Body::from(EMBEDDED_GUIDE_MD))
        .unwrap_or_else(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Response error"))
}

async fn get_capabilities() -> Json<crate::capabilities::CapabilityRegistry> {
    Json(crate::capabilities::CapabilityRegistry::current())
}

async fn validate_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = check_auth(&headers, &state.config.api_key) {
        return resp;
    }
    let raw = match std::str::from_utf8(&body) {
        Ok(s) => extract_clean_json(s),
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "Request body must be valid UTF-8"),
    };
    let scene: Scene = match serde_json::from_str(&raw) {
        Ok(s) => s,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, format!("Invalid scene JSON: {e}")),
    };
    if let Err(e) = crate::capabilities::validate_scene_capabilities(&scene) {
        return api_error(StatusCode::BAD_REQUEST, e.to_string());
    }
    if let Err(e) = validate_scene_for_service(&scene, &state.config.limits) {
        return api_error(StatusCode::BAD_REQUEST, e.to_string());
    }
    let warnings = collect_lint_warnings(&scene);
    Json(ValidateResponse {
        valid: true,
        warnings,
    })
    .into_response()
}

async fn submit_job(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = check_auth(&headers, &state.config.api_key) {
        return resp;
    }
    if let Err(resp) = check_rate_limit(&state, addr.ip()).await {
        return resp;
    }

    let raw = match std::str::from_utf8(&body) {
        Ok(s) => extract_clean_json(s),
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "Request body must be valid UTF-8"),
    };
    let payload: SubmitJobPayload = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, format!("Invalid job JSON: {e}")),
    };

    let (scene, webhook_url) = match payload {
        SubmitJobPayload::Wrapped {
            mut scene,
            composition,
            webhook_url,
        } => {
            if let Some(comp_name) = composition
                && let Err(e) = scene.apply_profile(&comp_name) {
                    return api_error(StatusCode::BAD_REQUEST, e.to_string());
                }
            (scene, webhook_url)
        }
        SubmitJobPayload::Direct(scene) => (scene, None),
    };

    if let Err(e) = crate::capabilities::validate_scene_capabilities(&scene) {
        return api_error(StatusCode::BAD_REQUEST, e.to_string());
    }
    if let Err(e) = validate_scene_for_service(&scene, &state.config.limits) {
        return api_error(StatusCode::BAD_REQUEST, e.to_string());
    }
    if let Some(ref url) = webhook_url
        && let Err(e) = validate_remote_url(url) {
            return api_error(StatusCode::BAD_REQUEST, format!("Invalid webhook_url: {e}"));
        }

    let warnings = collect_lint_warnings(&scene);
    let job_id = Uuid::new_v4().to_string();

    {
        let mut jobs = state.jobs.write().await;
        jobs.insert(
            job_id.clone(),
            JobRecord {
                id: job_id.clone(),
                status: JobStatus::Queued,
                created_at: Instant::now(),
                finished_at: None,
                render_secs: None,
                output_path: None,
                error: None,
            },
        );
    }

    if state
        .tx
        .send(QueuedJob {
            id: job_id.clone(),
            scene,
            webhook_url,
        })
        .await
        .is_err()
    {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "Render queue is closed");
    }

    (
        StatusCode::ACCEPTED,
        Json(SubmitJobResponse {
            job_id,
            status: JobStatus::Queued,
            warnings,
        }),
    )
        .into_response()
}

async fn get_job_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if let Err(resp) = check_auth(&headers, &state.config.api_key) {
        return resp;
    }
    let jobs = state.jobs.read().await;
    let Some(job) = jobs.get(&id) else {
        return api_error(StatusCode::NOT_FOUND, "Job not found");
    };
    let video_url = matches!(job.status, JobStatus::Completed)
        .then(|| format!("/jobs/{}/video", job.id));
    Json(JobStatusResponse {
        job_id: job.id.clone(),
        status: job.status.clone(),
        render_secs: job.render_secs,
        video_url,
        error: job.error.clone(),
    })
    .into_response()
}

async fn download_job_video(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if let Err(resp) = check_auth(&headers, &state.config.api_key) {
        return resp;
    }
    let output_path = {
        let jobs = state.jobs.read().await;
        let Some(job) = jobs.get(&id) else {
            return api_error(StatusCode::NOT_FOUND, "Job not found");
        };
        if !matches!(job.status, JobStatus::Completed) {
            return api_error(StatusCode::CONFLICT, "Video is not ready yet");
        }
        job.output_path.clone()
    };

    let Some(path) = output_path else {
        return api_error(StatusCode::NOT_FOUND, "Output file missing");
    };

    match tokio::fs::read(&path).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "video/mp4")
            .header(
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{id}.mp4\""),
            )
            .body(Body::from(bytes))
            .unwrap_or_else(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "Response error")),
        Err(_) => api_error(StatusCode::NOT_FOUND, "Output file expired or deleted"),
    }
}

async fn process_job(state: AppState, job: QueuedJob) {
    let permit = match state.semaphore.clone().acquire_owned().await {
        Ok(p) => p,
        Err(_) => return,
    };

    {
        let mut jobs = state.jobs.write().await;
        if let Some(rec) = jobs.get_mut(&job.id) {
            rec.status = JobStatus::Processing;
        }
    }

    let output_path = state.config.output_dir.join(format!("{}.mp4", job.id));
    let cache_dir = state.config.cache_dir.clone();
    let scene = job.scene;
    let out_clone = output_path.clone();

    let started = Instant::now();
    let render_res = tokio::task::spawn_blocking(move || {
        render_scene_to_file(&scene, &out_clone, &cache_dir)
    })
    .await;

    drop(permit);
    let elapsed = started.elapsed().as_secs_f32();

    let (final_status, err_msg) = match render_res {
        Ok(Ok(())) => (JobStatus::Completed, None),
        Ok(Err(e)) => (JobStatus::Failed, Some(e.to_string())),
        Err(e) => (JobStatus::Failed, Some(format!("Worker panicked: {e}"))),
    };

    {
        let mut jobs = state.jobs.write().await;
        if let Some(rec) = jobs.get_mut(&job.id) {
            rec.status = final_status.clone();
            rec.finished_at = Some(Instant::now());
            rec.render_secs = Some(elapsed);
            rec.error = err_msg.clone();
            if matches!(final_status, JobStatus::Completed) {
                rec.output_path = Some(output_path);
            }
        }
    }

    if let Some(webhook_url) = job.webhook_url {
        let payload = serde_json::json!({
            "job_id": job.id,
            "status": final_status,
            "render_secs": elapsed,
            "video_url": matches!(final_status, JobStatus::Completed).then(|| format!("/jobs/{}/video", job.id)),
            "error": err_msg,
        });
        let _ = tokio::task::spawn_blocking(move || {
            let _ = ureq::post(&webhook_url)
                .timeout(Duration::from_secs(10))
                .set("Content-Type", "application/json")
                .send_string(&payload.to_string());
        })
        .await;
    }
}

async fn cleanup_loop(state: AppState) {
    let ttl = Duration::from_secs(state.config.ttl_secs);
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        let now = Instant::now();
        let mut to_delete = Vec::new();
        {
            let mut jobs = state.jobs.write().await;
            jobs.retain(|_, rec| {
                let ref_time = rec.finished_at.unwrap_or(rec.created_at);
                let expired = matches!(rec.status, JobStatus::Completed | JobStatus::Failed)
                    && now.duration_since(ref_time) > ttl;
                if expired {
                    if let Some(p) = &rec.output_path {
                        to_delete.push(p.clone());
                    }
                    false
                } else {
                    true
                }
            });
        }
        for path in to_delete {
            let _ = tokio::fs::remove_file(path).await;
        }
        {
            let window = Duration::from_secs(60);
            let mut rl = state.rate_limits.write().await;
            rl.retain(|_, ts| {
                ts.retain(|t| now.duration_since(*t) < window);
                !ts.is_empty()
            });
        }
    }
}

pub async fn run_server(config: ServeConfig) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&config.output_dir).await?;
    tokio::fs::create_dir_all(&config.cache_dir).await?;

    let (tx, mut rx) = mpsc::channel::<QueuedJob>(128);
    let state = AppState {
        semaphore: Arc::new(Semaphore::new(config.workers.max(1))),
        config: config.clone(),
        jobs: Arc::new(RwLock::new(HashMap::new())),
        rate_limits: Arc::new(RwLock::new(HashMap::new())),
        tx,
    };

    let worker_state = state.clone();
    tokio::spawn(async move {
        while let Some(job) = rx.recv().await {
            let st = worker_state.clone();
            tokio::spawn(process_job(st, job));
        }
    });

    let cleanup_state = state.clone();
    tokio::spawn(cleanup_loop(cleanup_state));

    let app = Router::new()
        .route("/health", get(health))
        .route("/schema", get(get_schema))
        .route("/guide", get(get_guide))
        .route("/capabilities", get(get_capabilities))
        .route("/validate", post(validate_endpoint))
        .route("/jobs", post(submit_job))
        .route("/jobs/:id", get(get_job_status))
        .route("/jobs/:id/video", get(download_job_video))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    println!(
        "rustren: listening on http://{} (workers={}, ttl={}s, max={}x{}@{}fps/{}s, local_paths={})",
        addr,
        config.workers,
        config.ttl_secs,
        config.limits.max_width,
        config.limits.max_height,
        config.limits.max_fps,
        config.limits.max_duration_secs,
        config.limits.allow_local_paths
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_clean_json_strips_chatbot_wrapper() {
        let noisy = "\u{FEFF}Berikut JSON-nya bro:\n```json\n{\"a\": 1}\n```\nSemoga membantu!";
        assert_eq!(extract_clean_json(noisy), "{\"a\": 1}");
    }
}
