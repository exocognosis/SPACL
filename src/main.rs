use std::{
    collections::BTreeMap,
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use spacl::{
    Approval, AuditLog, Coordinator, ExecuteRequest, ExecutionContext, HybridIdentity,
    PolicyConstraints, PublicIdentity, RiskLevel, RobotAction, RobotRegistration, RobotRuntime,
    TokenRequest,
};
use tokio::sync::Mutex;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::info;

#[derive(Parser)]
#[command(
    name = "spacl",
    version,
    about = "Secure Physical Agent Coordination Layer"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a hybrid ML-DSA-65 and Ed25519 identity.
    Keygen {
        #[arg(long)]
        subject: String,
        #[arg(long)]
        private_out: PathBuf,
        #[arg(long)]
        public_out: PathBuf,
    },
    /// Run the coordination API.
    Coordinator {
        #[arg(long, default_value = "127.0.0.1:8080")]
        bind: SocketAddr,
        #[arg(long, default_value = "./data/coordinator")]
        data_dir: PathBuf,
    },
    /// Run one robot execution gate.
    Robot {
        #[arg(long)]
        robot_id: String,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        coordinator_public: PathBuf,
        #[arg(long, default_value = "127.0.0.1:8081")]
        bind: SocketAddr,
        #[arg(long, default_value = "./data/robot")]
        data_dir: PathBuf,
    },
    /// Run an in-process three-robot simulation.
    Demo {
        #[arg(long, default_value = "./data/demo")]
        data_dir: PathBuf,
    },
    /// Verify a signed JSON Lines audit chain.
    VerifyAudit {
        #[arg(long)]
        audit: PathBuf,
        #[arg(long, required = true)]
        public_identity: Vec<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("spacl=info".parse()?),
        )
        .json()
        .init();
    match Cli::parse().command {
        Command::Keygen {
            subject,
            private_out,
            public_out,
        } => {
            let identity = HybridIdentity::generate(subject);
            identity.save_private(&private_out)?;
            identity.save_public(&public_out)?;
            println!("{}", serde_json::to_string_pretty(&identity.public)?);
        }
        Command::Coordinator { bind, data_dir } => run_coordinator(bind, data_dir).await?,
        Command::Robot {
            robot_id,
            identity,
            coordinator_public,
            bind,
            data_dir,
        } => run_robot(bind, robot_id, identity, coordinator_public, data_dir).await?,
        Command::Demo { data_dir } => run_demo(&data_dir)?,
        Command::VerifyAudit {
            audit,
            public_identity,
        } => verify_audit(&audit, &public_identity)?,
    }
    Ok(())
}

type CoordinatorState = Arc<Mutex<Coordinator>>;
type RuntimeState = Arc<Mutex<RobotRuntime>>;

async fn run_coordinator(bind: SocketAddr, data_dir: PathBuf) -> Result<()> {
    let state = Arc::new(Mutex::new(Coordinator::open(data_dir)?));
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/v1/fleet", get(fleet))
        .route("/v1/robots", post(enroll))
        .route("/v1/robots/{robot_id}/revoke", post(revoke))
        .route("/v1/tokens", post(issue_token))
        .with_state(state)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "coordination API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn run_robot(
    bind: SocketAddr,
    robot_id: String,
    identity_path: PathBuf,
    coordinator_public_path: PathBuf,
    data_dir: PathBuf,
) -> Result<()> {
    let identity = HybridIdentity::load_private(&identity_path)?;
    if identity.public.subject != robot_id {
        anyhow::bail!("robot ID does not match the identity subject")
    }
    let coordinator_public: PublicIdentity =
        serde_json::from_slice(&fs::read(coordinator_public_path)?)?;
    let runtime = RobotRuntime::open(robot_id, identity, coordinator_public, data_dir)?;
    let state = Arc::new(Mutex::new(runtime));
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/v1/status", get(runtime_status))
        .route("/v1/execute", post(execute))
        .route("/v1/emergency-stop", post(emergency_stop))
        .with_state(state)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "robot execution gate listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
}

async fn fleet(State(state): State<CoordinatorState>) -> Json<serde_json::Value> {
    let coordinator = state.lock().await;
    Json(
        serde_json::json!({"robots": coordinator.robots(), "coordinator": coordinator.identity.public}),
    )
}

async fn enroll(
    State(state): State<CoordinatorState>,
    Json(request): Json<RobotRegistration>,
) -> ApiResult {
    let record = state.lock().await.enroll(request).map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(record)?)))
}

async fn revoke(
    State(state): State<CoordinatorState>,
    AxumPath(robot_id): AxumPath<String>,
) -> ApiResult {
    state
        .lock()
        .await
        .revoke(&robot_id)
        .map_err(ApiError::from)?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({"robot_id": robot_id, "revoked": true})),
    ))
}

async fn issue_token(
    State(state): State<CoordinatorState>,
    Json(request): Json<TokenRequest>,
) -> ApiResult {
    let token = state
        .lock()
        .await
        .issue_token(request)
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(token)?)))
}

async fn runtime_status(State(state): State<RuntimeState>) -> Json<serde_json::Value> {
    Json(state.lock().await.status())
}

async fn execute(
    State(state): State<RuntimeState>,
    Json(request): Json<ExecuteRequest>,
) -> ApiResult {
    let receipt = state
        .lock()
        .await
        .execute(&request.token, &request.context)
        .map_err(ApiError::from)?;
    Ok((StatusCode::OK, Json(serde_json::to_value(receipt)?)))
}

#[derive(Deserialize)]
struct EmergencyStopRequest {
    active: bool,
}

async fn emergency_stop(
    State(state): State<RuntimeState>,
    Json(request): Json<EmergencyStopRequest>,
) -> ApiResult {
    let mut runtime = state.lock().await;
    runtime.set_emergency_stop(request.active)?;
    Ok((StatusCode::OK, Json(runtime.status())))
}

type ApiResult = Result<(StatusCode, Json<serde_json::Value>), ApiError>;

struct ApiError {
    status: StatusCode,
    message: String,
}

impl<E: std::fmt::Display> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({"error": self.message})),
        )
            .into_response()
    }
}

fn run_demo(data_dir: &Path) -> Result<()> {
    if data_dir.exists() {
        anyhow::bail!("demo data directory already exists: {}", data_dir.display())
    }
    fs::create_dir_all(data_dir)?;
    let coordinator_dir = data_dir.join("coordinator");
    let mut coordinator = Coordinator::open(coordinator_dir.clone())?;
    let mut completed = Vec::new();

    for (index, skill) in ["move", "pick", "place"].into_iter().enumerate() {
        let robot_id = format!("sim-robot-{}", index + 1);
        let identity = HybridIdentity::generate(&robot_id);
        let robot_dir = data_dir.join(&robot_id);
        fs::create_dir_all(&robot_dir)?;
        identity.save_private(&robot_dir.join("identity.json"))?;
        identity.save_public(&robot_dir.join("public.json"))?;
        coordinator.enroll(RobotRegistration {
            robot_id: robot_id.clone(),
            display_name: format!("Simulation Robot {}", index + 1),
            identity: identity.public.clone(),
        })?;
        let context = ExecutionContext {
            task_id: "warehouse-demo".into(),
            zone: format!("cell-{}", index + 1),
            state_hash: "sha256:demo-world-v1".into(),
        };
        let risk = if skill == "pick" {
            RiskLevel::High
        } else {
            RiskLevel::Normal
        };
        let approvals = if risk == RiskLevel::High {
            vec![
                Approval {
                    operator_id: "operator-alice".into(),
                    approved_at_unix_ms: Utc::now().timestamp_millis(),
                },
                Approval {
                    operator_id: "operator-bob".into(),
                    approved_at_unix_ms: Utc::now().timestamp_millis(),
                },
            ]
        } else {
            vec![]
        };
        let token = coordinator.issue_token(TokenRequest {
            robot_id: robot_id.clone(),
            action: RobotAction {
                skill: skill.into(),
                arguments: BTreeMap::new(),
                requested_speed_mps: Some(0.5),
                requested_force_newtons: Some(20.0),
            },
            context: context.clone(),
            ttl_seconds: 30,
            constraints: PolicyConstraints {
                allowed_skills: vec![skill.into()],
                allowed_zones: vec![context.zone.clone()],
                max_speed_mps: Some(1.0),
                max_force_newtons: Some(50.0),
            },
            risk,
            approvals,
        })?;
        let mut runtime = RobotRuntime::open(
            &robot_id,
            identity,
            coordinator.identity.public.clone(),
            robot_dir,
        )?;
        completed.push(runtime.execute(&token, &context)?);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "completed", "robots": completed.len(), "receipts": completed,
            "coordinator_audit": coordinator_dir.join("audit.jsonl"),
        }))?
    );
    Ok(())
}

fn verify_audit(audit: &Path, identity_paths: &[PathBuf]) -> Result<()> {
    let identities = identity_paths
        .iter()
        .map(|path| -> Result<PublicIdentity> {
            let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
            Ok(serde_json::from_slice(&bytes)?)
        })
        .collect::<Result<Vec<_>>>()?;
    let records = AuditLog::verify(audit, &identities)?;
    println!("verified {} audit records", records.len());
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
