use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use clap::{Args, Parser, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use spacl::{
    ApiErrorBody, Approval, AuditLog, AuditRecord, Coordinator, CoordinatorError, ExecuteRequest,
    ExecutionContext, ExecutionError, HybridIdentity, Metrics, PolicyConstraints, PublicIdentity,
    RiskLevel, RobotAction, RobotRegistration, RobotRuntime, TokenRequest,
};
use tokio::sync::Mutex;
use tower_http::{
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::info;

const COORDINATOR_URL: &str = "http://127.0.0.1:8080";
const ROBOT_URL: &str = "http://127.0.0.1:8081";

#[derive(Parser)]
#[command(
    name = "spacl",
    version,
    about = "Secure Physical Agent Coordination Layer",
    long_about = "Issue verified action tokens, run robot execution gates, and inspect signed audit chains for simulated multi-robot fleets.",
    after_help = "Start here:\n  spacl single-agent --watch\n  spacl init\n  spacl demo --interactive --watch\n  spacl status\n\nDocumentation: https://github.com/exocognosis/SPACL"
)]
struct Cli {
    /// Workspace root. Defaults to the operating system local data directory.
    #[arg(long, global = true, env = "SPACL_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Emit structured JSON service logs.
    #[arg(long, global = true)]
    json_logs: bool,

    /// Print compact JSON instead of indented JSON.
    #[arg(long, global = true)]
    compact: bool,

    /// Disable terminal colors.
    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a ready-to-run workspace and enroll one sample robot.
    #[command(
        long_about = "Create secure data, secrets, config, token, and audit directories. Generate a coordinator identity and one sample robot identity. Enroll the sample robot and write TOML configuration files."
    )]
    Init {
        /// Fill missing files in an existing workspace. Existing keys are preserved.
        #[arg(long)]
        force: bool,
    },

    /// Show local state and query running coordinator or robot nodes.
    Status {
        #[arg(long, default_value = COORDINATOR_URL)]
        coordinator_url: String,
        /// Robot runtime URL. Repeat this option for more robots.
        #[arg(long)]
        robot_url: Vec<String>,
        /// Do not make network requests.
        #[arg(long)]
        local_only: bool,
    },

    /// Generate a hybrid ML-DSA-65 and Ed25519 identity.
    Keygen {
        #[arg(long)]
        subject: String,
        #[arg(long)]
        private_out: Option<PathBuf>,
        #[arg(long)]
        public_out: Option<PathBuf>,
    },

    /// Run the coordination API.
    #[command(
        after_help = "Example:\n  spacl coordinator --config ~/.config/spacl/coordinator.toml"
    )]
    Coordinator {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        bind: Option<SocketAddr>,
    },

    /// Run one robot verification and execution gate.
    #[command(after_help = "Example:\n  spacl robot --config <workspace>/config/robot-1.toml")]
    Robot {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        robot_id: Option<String>,
        #[arg(long)]
        identity: Option<PathBuf>,
        #[arg(long)]
        coordinator_public: Option<PathBuf>,
        #[arg(long)]
        bind: Option<SocketAddr>,
    },

    /// Run an in-process three-robot simulation.
    Demo {
        /// Pause before each issue and execution step.
        #[arg(long)]
        interactive: bool,
        /// Print token and audit activity while the demo runs.
        #[arg(long)]
        watch: bool,
        /// Override the generated demo directory.
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Run one complete signed-token loop with one simulated robot.
    #[command(
        after_help = "Example:\n  spacl --data-dir ./.spacl single-agent --skill move --watch"
    )]
    SingleAgent(SingleAgentArgs),

    /// Issue action tokens through the coordinator API.
    Token {
        #[command(subcommand)]
        command: TokenCommand,
    },

    /// Execute a saved token through a robot runtime API.
    Execute(ExecuteArgs),

    /// Read and verify signed audit chains.
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
}

#[derive(Subcommand)]
enum TokenCommand {
    /// Build and issue one action token without hand-written JSON.
    #[command(
        after_help = "Example:\n  spacl token issue --robot-id robot-1 --skill move --task-id task-1 --zone cell-1 --speed 0.5"
    )]
    Issue(TokenIssueArgs),
}

#[derive(Args)]
struct TokenIssueArgs {
    #[arg(long, default_value = COORDINATOR_URL)]
    coordinator_url: String,
    #[arg(long)]
    robot_id: String,
    #[arg(long)]
    skill: String,
    #[arg(long)]
    task_id: String,
    #[arg(long)]
    zone: String,
    #[arg(long, default_value = "sha256:development-world-state")]
    state_hash: String,
    #[arg(long, default_value_t = 30)]
    ttl_seconds: u64,
    #[arg(long)]
    speed: Option<f64>,
    #[arg(long)]
    force: Option<f64>,
    /// Action argument in key=value form. Repeat for more arguments.
    #[arg(long = "arg", value_name = "KEY=VALUE")]
    arguments: Vec<String>,
    #[arg(long)]
    high_risk: bool,
    /// Operator ID. High-risk actions need two distinct IDs.
    #[arg(long = "approver")]
    approvers: Vec<String>,
    /// Token output path. Defaults to <data-dir>/tokens/<token-id>.json.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Print the full token, including both signatures.
    #[arg(long)]
    show_token: bool,
}

#[derive(Args)]
struct SingleAgentArgs {
    /// Simulated action to authorize and execute.
    #[arg(long, default_value = "move", value_parser = ["move", "pick", "place", "wait"])]
    skill: String,
    /// Override the generated output directory.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Print the complete signed token.
    #[arg(long)]
    show_token: bool,
    /// Print both audit timelines after verification.
    #[arg(long)]
    watch: bool,
}

#[derive(Args)]
struct ExecuteArgs {
    #[arg(long, default_value = ROBOT_URL)]
    robot_url: String,
    #[arg(long)]
    token: PathBuf,
    #[arg(long)]
    task_id: String,
    #[arg(long)]
    zone: String,
    #[arg(long, default_value = "sha256:development-world-state")]
    state_hash: String,
}

#[derive(Subcommand)]
enum AuditCommand {
    /// Verify every record hash and signature.
    Verify {
        #[arg(long)]
        audit: PathBuf,
        #[arg(long, required = true)]
        public_identity: Vec<PathBuf>,
    },
    /// Print an audit chain as a readable event timeline.
    Pretty {
        #[arg(long)]
        audit: PathBuf,
    },
    /// Print the last records and optionally follow new records.
    Tail {
        #[arg(long)]
        audit: PathBuf,
        #[arg(long, default_value_t = 10)]
        lines: usize,
        #[arg(long, short = 'f')]
        follow: bool,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct CoordinatorConfig {
    bind: Option<SocketAddr>,
    data_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct RobotConfig {
    robot_id: Option<String>,
    identity: Option<PathBuf>,
    coordinator_public: Option<PathBuf>,
    bind: Option<SocketAddr>,
    data_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if cli.no_color {
        colored::control::set_override(false);
    }
    if let Err(error) = init_logging(cli.json_logs) {
        eprintln!("{} {error:#}", "error:".red().bold());
        std::process::exit(1);
    }
    let compact = cli.compact;
    let data_dir = cli.data_dir.unwrap_or_else(default_data_dir);
    if let Err(error) = run(cli.command, data_dir, compact).await {
        eprintln!("{} {error:#}", "error:".red().bold());
        std::process::exit(1);
    }
}

fn init_logging(json_logs: bool) -> Result<()> {
    let filter =
        tracing_subscriber::EnvFilter::from_default_env().add_directive("spacl=info".parse()?);
    if json_logs {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .try_init()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .compact()
            .try_init()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }
    Ok(())
}

async fn run(command: Command, data_dir: PathBuf, compact: bool) -> Result<()> {
    match command {
        Command::Init { force } => init_workspace(&data_dir, force)?,
        Command::Status {
            coordinator_url,
            robot_url,
            local_only,
        } => show_status(&data_dir, &coordinator_url, &robot_url, local_only, compact).await?,
        Command::Keygen {
            subject,
            private_out,
            public_out,
        } => keygen(&data_dir, subject, private_out, public_out, compact)?,
        Command::Coordinator { config, bind } => {
            let file: CoordinatorConfig = load_optional_config(config.as_deref())?;
            let bind = bind
                .or(file.bind)
                .unwrap_or_else(|| "127.0.0.1:8080".parse().unwrap());
            let node_data = file
                .data_dir
                .unwrap_or_else(|| data_dir.join("data/coordinator"));
            run_coordinator(bind, node_data).await?;
        }
        Command::Robot {
            config,
            robot_id,
            identity,
            coordinator_public,
            bind,
        } => {
            let file: RobotConfig = load_optional_config(config.as_deref())?;
            let robot_id = required(
                robot_id.or(file.robot_id),
                "robot ID",
                "--robot-id or robot_id in the config file",
            )?;
            let identity = required(
                identity.or(file.identity),
                "robot identity",
                "--identity or identity in the config file",
            )?;
            let coordinator_public = required(
                coordinator_public.or(file.coordinator_public),
                "coordinator public identity",
                "--coordinator-public or coordinator_public in the config file",
            )?;
            let bind = bind
                .or(file.bind)
                .unwrap_or_else(|| "127.0.0.1:8081".parse().unwrap());
            let node_data = file
                .data_dir
                .unwrap_or_else(|| data_dir.join(format!("data/{robot_id}")));
            run_robot(bind, robot_id, identity, coordinator_public, node_data).await?;
        }
        Command::Demo {
            interactive,
            watch,
            output,
        } => {
            let output = output.unwrap_or_else(|| {
                data_dir.join(format!("demos/{}", Utc::now().format("%Y%m%d-%H%M%S")))
            });
            run_demo(&output, interactive, watch, compact)?;
        }
        Command::SingleAgent(arguments) => {
            let output = arguments.output.unwrap_or_else(|| {
                data_dir.join(format!("single-agent/{}", Utc::now().timestamp_millis()))
            });
            run_single_agent(
                &output,
                &arguments.skill,
                arguments.show_token,
                arguments.watch,
                compact,
            )?;
        }
        Command::Token { command } => match command {
            TokenCommand::Issue(arguments) => {
                issue_token_cli(&data_dir, arguments, compact).await?
            }
        },
        Command::Execute(arguments) => execute_cli(arguments, compact).await?,
        Command::Audit { command } => match command {
            AuditCommand::Verify {
                audit,
                public_identity,
            } => verify_audit(&audit, &public_identity)?,
            AuditCommand::Pretty { audit } => {
                print_audit(&audit, 0)?;
            }
            AuditCommand::Tail {
                audit,
                lines,
                follow,
            } => tail_audit(&audit, lines, follow).await?,
        },
    }
    Ok(())
}

fn default_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from(".spacl"))
        .join("spacl")
}

fn secure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn init_workspace(root: &Path, force: bool) -> Result<()> {
    let marker = root.join("spacl-workspace.json");
    if marker.exists() && !force {
        bail!(
            "workspace already exists at {}\nNext: use --force to fill missing sample files without replacing keys",
            root.display()
        );
    }
    for directory in ["config", "secrets", "data", "tokens", "audits", "demos"] {
        secure_dir(&root.join(directory))?;
    }
    let coordinator_dir = root.join("data/coordinator");
    let mut coordinator = Coordinator::open(coordinator_dir.clone())?;
    let robot_private = root.join("secrets/robot-1.identity.json");
    let robot_public = root.join("config/robot-1.public.json");
    let robot_identity = if robot_private.exists() {
        HybridIdentity::load_private(&robot_private)?
    } else {
        let identity = HybridIdentity::generate("robot-1");
        identity.save_private(&robot_private)?;
        identity
    };
    if !robot_public.exists() {
        robot_identity.save_public(&robot_public)?;
    }
    if !coordinator
        .robots()
        .iter()
        .any(|robot| robot.robot_id == "robot-1")
    {
        coordinator.enroll(RobotRegistration {
            robot_id: "robot-1".into(),
            display_name: "Sample Robot 1".into(),
            identity: robot_identity.public.clone(),
        })?;
    }

    let coordinator_config = CoordinatorConfig {
        bind: Some("127.0.0.1:8080".parse()?),
        data_dir: Some(coordinator_dir.clone()),
    };
    let robot_config = RobotConfig {
        robot_id: Some("robot-1".into()),
        identity: Some(robot_private),
        coordinator_public: Some(coordinator_dir.join("coordinator.public.json")),
        bind: Some("127.0.0.1:8081".parse()?),
        data_dir: Some(root.join("data/robot-1")),
    };
    write_if_missing(
        &root.join("config/coordinator.toml"),
        toml::to_string_pretty(&coordinator_config)?.as_bytes(),
    )?;
    write_if_missing(
        &root.join("config/robot-1.toml"),
        toml::to_string_pretty(&robot_config)?.as_bytes(),
    )?;
    write_if_missing(
        &root.join("config/sample-token-request.json"),
        &serde_json::to_vec_pretty(&sample_token_request())?,
    )?;
    fs::write(
        marker,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "spacl.workspace.v1",
            "created_at": Utc::now().to_rfc3339(),
            "version": env!("CARGO_PKG_VERSION")
        }))?,
    )?;

    println!("{} {}", "workspace ready:".green().bold(), root.display());
    println!("\nRun these commands in separate terminals:");
    println!(
        "  spacl coordinator --config {}",
        root.join("config/coordinator.toml").display()
    );
    println!(
        "  spacl robot --config {}",
        root.join("config/robot-1.toml").display()
    );
    println!("  spacl status --robot-url {ROBOT_URL}");
    Ok(())
}

fn write_if_missing(path: &Path, bytes: &[u8]) -> Result<()> {
    if !path.exists() {
        fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

fn keygen(
    root: &Path,
    subject: String,
    private_out: Option<PathBuf>,
    public_out: Option<PathBuf>,
    compact: bool,
) -> Result<()> {
    secure_dir(&root.join("secrets"))?;
    secure_dir(&root.join("config"))?;
    let private_out =
        private_out.unwrap_or_else(|| root.join(format!("secrets/{subject}.identity.json")));
    let public_out =
        public_out.unwrap_or_else(|| root.join(format!("config/{subject}.public.json")));
    if private_out.exists() || public_out.exists() {
        bail!(
            "identity output already exists\nNext: choose another --subject or explicit output paths"
        )
    }
    let identity = HybridIdentity::generate(subject);
    identity.save_private(&private_out)?;
    identity.save_public(&public_out)?;
    print_json(&identity.public, compact)?;
    println!("{} {}", "private identity:".green(), private_out.display());
    println!("{} {}", "public identity:".green(), public_out.display());
    Ok(())
}

fn load_optional_config<T: DeserializeOwned + Default>(path: Option<&Path>) -> Result<T> {
    match path {
        Some(path) => toml::from_str(
            &fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?,
        )
        .with_context(|| format!("parse config {}", path.display())),
        None => Ok(T::default()),
    }
}

fn required<T>(value: Option<T>, name: &str, source: &str) -> Result<T> {
    value.ok_or_else(|| anyhow::anyhow!("missing {name}\nNext: set {source}"))
}

async fn show_status(
    root: &Path,
    coordinator_url: &str,
    robot_urls: &[String],
    local_only: bool,
    compact: bool,
) -> Result<()> {
    println!("{} {}", "workspace:".bold(), root.display());
    let coordinator_state = root.join("data/coordinator/state.json");
    if coordinator_state.exists() {
        let state: serde_json::Value = serde_json::from_slice(&fs::read(&coordinator_state)?)?;
        println!("{}", "local coordinator state".cyan().bold());
        print_status_value(&state, compact)?;
    } else {
        println!(
            "{} run `spacl init` to create one",
            "local state: missing;".yellow()
        );
    }
    if local_only {
        return Ok(());
    }
    let client = reqwest::Client::new();
    print_remote_status(
        &client,
        "coordinator",
        &format!("{}/v1/status", trim_url(coordinator_url)),
        compact,
    )
    .await;
    for (index, url) in robot_urls.iter().enumerate() {
        print_remote_status(
            &client,
            &format!("robot {}", index + 1),
            &format!("{}/v1/status", trim_url(url)),
            compact,
        )
        .await;
    }
    Ok(())
}

async fn print_remote_status(client: &reqwest::Client, label: &str, url: &str, compact: bool) {
    match client.get(url).timeout(Duration::from_secs(2)).send().await {
        Ok(response) => match parse_response(response).await {
            Ok(value) => {
                println!("{} {}", label.bold(), "online".green());
                let _ = print_status_value(&value, compact);
            }
            Err(error) => println!("{} {}: {error}", label.bold(), "error".red()),
        },
        Err(_) => println!("{} {} ({url})", label.bold(), "offline".yellow()),
    }
}

async fn issue_token_cli(root: &Path, args: TokenIssueArgs, compact: bool) -> Result<()> {
    let request = TokenRequest {
        robot_id: args.robot_id,
        action: RobotAction {
            skill: args.skill.clone(),
            arguments: parse_arguments(&args.arguments)?,
            requested_speed_mps: args.speed,
            requested_force_newtons: args.force,
        },
        context: ExecutionContext {
            task_id: args.task_id,
            zone: args.zone.clone(),
            state_hash: args.state_hash,
        },
        ttl_seconds: args.ttl_seconds,
        constraints: PolicyConstraints {
            allowed_skills: vec![args.skill],
            allowed_zones: vec![args.zone],
            max_speed_mps: args.speed,
            max_force_newtons: args.force,
        },
        risk: if args.high_risk {
            RiskLevel::High
        } else {
            RiskLevel::Normal
        },
        approvals: args
            .approvers
            .into_iter()
            .map(|operator_id| Approval {
                operator_id,
                approved_at_unix_ms: Utc::now().timestamp_millis(),
            })
            .collect(),
    };
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/tokens", trim_url(&args.coordinator_url)))
        .json(&request)
        .send()
        .await
        .context(
            "contact coordinator\nNext: start `spacl coordinator` or check --coordinator-url",
        )?;
    let value = parse_response(response).await?;
    let token_id = value
        .pointer("/claims/token_id")
        .and_then(|value| value.as_str())
        .unwrap_or("token");
    secure_dir(&root.join("tokens"))?;
    let out = args
        .out
        .unwrap_or_else(|| root.join(format!("tokens/{token_id}.json")));
    fs::write(&out, serde_json::to_vec_pretty(&value)?)?;
    println!("{} {}", "token issued:".green().bold(), token_id);
    println!("{} {}", "saved:".green(), out.display());
    if args.show_token {
        print_json(&value, compact)?;
    } else {
        println!("Use --show-token to print the complete signed token.");
    }
    Ok(())
}

async fn execute_cli(args: ExecuteArgs, compact: bool) -> Result<()> {
    let token = serde_json::from_slice(
        &fs::read(&args.token).with_context(|| format!("read token {}", args.token.display()))?,
    )?;
    let request = ExecuteRequest {
        token,
        context: ExecutionContext {
            task_id: args.task_id,
            zone: args.zone,
            state_hash: args.state_hash,
        },
    };
    let response = reqwest::Client::new()
        .post(format!("{}/v1/execute", trim_url(&args.robot_url)))
        .json(&request)
        .send()
        .await
        .context("contact robot runtime\nNext: start `spacl robot` or check --robot-url")?;
    let value = parse_response(response).await?;
    println!("{}", "action completed".green().bold());
    print_json(&value, compact)
}

fn parse_arguments(values: &[String]) -> Result<BTreeMap<String, serde_json::Value>> {
    values
        .iter()
        .map(|entry| {
            let (key, value) = entry
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("invalid --arg {entry}; use KEY=VALUE"))?;
            let value = serde_json::from_str(value)
                .unwrap_or_else(|_| serde_json::Value::String(value.into()));
            Ok((key.into(), value))
        })
        .collect()
}

async fn parse_response(response: reqwest::Response) -> Result<serde_json::Value> {
    let status = response.status();
    let bytes = response.bytes().await?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("server returned non-JSON data with HTTP {status}"))?;
    if status.is_success() {
        return Ok(value);
    }
    if let Ok(error) = serde_json::from_value::<ApiErrorBody>(value.clone()) {
        bail!("{} ({})\nNext: {}", error.message, error.code, error.action)
    }
    bail!("request failed with HTTP {status}: {value}")
}

fn trim_url(url: &str) -> &str {
    url.trim_end_matches('/')
}

fn print_json<T: Serialize>(value: &T, compact: bool) -> Result<()> {
    if compact {
        println!("{}", serde_json::to_string(value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(value)?);
    }
    Ok(())
}

fn print_status_value(value: &serde_json::Value, compact: bool) -> Result<()> {
    if compact {
        return print_json(value, true);
    }
    if let Some(role) = value.get("role").and_then(|value| value.as_str()) {
        println!("  role: {role}");
    }
    if let Some(activity) = value.get("last_activity").filter(|value| !value.is_null()) {
        println!(
            "  last activity: {} {} at {}",
            activity
                .get("kind")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown"),
            activity
                .get("subject")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown"),
            activity
                .get("at_unix_ms")
                .and_then(|value| value.as_i64())
                .map(format_time)
                .unwrap_or_else(|| "unknown".into())
        );
    }
    if let Some(stop) = value
        .get("emergency_stop")
        .and_then(|value| value.as_bool())
    {
        println!(
            "  emergency stop: {}",
            if stop {
                "active".red().bold()
            } else {
                "clear".green()
            }
        );
    }
    if let Some(sequence) = value.get("next_sequence").and_then(|value| value.as_u64()) {
        println!("  next sequence: {sequence}");
    }
    if let Some(robots) = value.get("robots") {
        let robot_values: Vec<&serde_json::Value> = match robots {
            serde_json::Value::Array(values) => values.iter().collect(),
            serde_json::Value::Object(values) => values.values().collect(),
            _ => vec![],
        };
        println!("  enrolled robots: {}", robot_values.len());
        for robot in robot_values {
            let id = robot
                .get("robot_id")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let sequence = robot
                .get("next_sequence")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let revoked = robot
                .get("revoked")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            println!(
                "    {id:<20} next={sequence:<4} {}",
                if revoked {
                    "revoked".red()
                } else {
                    "active".green()
                }
            );
        }
    }
    Ok(())
}

fn format_time(unix_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(unix_ms)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| unix_ms.to_string())
}

fn verify_audit(audit: &Path, identity_paths: &[PathBuf]) -> Result<()> {
    let identities = identity_paths
        .iter()
        .map(|path| -> Result<PublicIdentity> {
            Ok(serde_json::from_slice(
                &fs::read(path).with_context(|| format!("read {}", path.display()))?,
            )?)
        })
        .collect::<Result<Vec<_>>>()?;
    let records = AuditLog::verify(audit, &identities)?;
    println!(
        "{} {} records in {}",
        "verified".green().bold(),
        records.len(),
        audit.display()
    );
    Ok(())
}

fn print_audit(path: &Path, skip: usize) -> Result<usize> {
    let records = AuditLog::read(path)?;
    for record in records.iter().skip(skip) {
        print_audit_record(record);
    }
    Ok(records.len())
}

fn print_audit_record(record: &AuditRecord) {
    let timestamp = chrono::DateTime::from_timestamp_millis(record.body.timestamp_unix_ms)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| record.body.timestamp_unix_ms.to_string());
    let kind = match record.body.event.kind.as_str() {
        value if value.contains("rejected") || value.contains("revoked") => value.red().bold(),
        value if value.contains("completed") || value.contains("enrolled") => value.green().bold(),
        value => value.cyan().bold(),
    };
    println!(
        "{} {:>4} {} {} → {} {}",
        timestamp.dimmed(),
        record.body.index,
        kind,
        record.body.event.actor,
        record.body.event.subject,
        record.body.event.detail
    );
}

async fn tail_audit(path: &Path, lines: usize, follow: bool) -> Result<()> {
    let records = AuditLog::read(path)?;
    let start = records.len().saturating_sub(lines);
    for record in &records[start..] {
        print_audit_record(record);
    }
    if !follow {
        return Ok(());
    }
    println!("{} Press Ctrl-C to stop.", "following audit chain.".cyan());
    let mut seen = records.len();
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = interval.tick() => { seen = print_audit(path, seen)?; }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    Ok(())
}

fn sample_token_request() -> TokenRequest {
    TokenRequest {
        robot_id: "robot-1".into(),
        action: RobotAction {
            skill: "move".into(),
            arguments: BTreeMap::from([("distance_m".into(), serde_json::json!(2.0))]),
            requested_speed_mps: Some(0.5),
            requested_force_newtons: None,
        },
        context: ExecutionContext {
            task_id: "sample-task".into(),
            zone: "cell-1".into(),
            state_hash: "sha256:development-world-state".into(),
        },
        ttl_seconds: 30,
        constraints: PolicyConstraints {
            allowed_skills: vec!["move".into()],
            allowed_zones: vec!["cell-1".into()],
            max_speed_mps: Some(0.5),
            max_force_newtons: None,
        },
        risk: RiskLevel::Normal,
        approvals: vec![],
    }
}

fn pause(message: &str) -> Result<()> {
    print!(
        "{} {message} Press Enter to continue, or type q to stop: ",
        "interactive:".cyan().bold()
    );
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if input.trim().eq_ignore_ascii_case("q") {
        bail!("demo stopped by user")
    }
    Ok(())
}

fn choose_skill(default_skill: &str, robot_id: &str) -> Result<String> {
    loop {
        print!(
            "{} action for {robot_id} [move/pick/place/wait, Enter={default_skill}, q=quit]: ",
            "interactive:".cyan().bold()
        );
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        match input.trim() {
            "" => return Ok(default_skill.into()),
            "move" | "pick" | "place" | "wait" => return Ok(input.trim().into()),
            value if value.eq_ignore_ascii_case("q") => bail!("demo stopped by user"),
            _ => println!("Choose move, pick, place, wait, or q."),
        }
    }
}

fn run_demo(data_dir: &Path, interactive: bool, watch: bool, compact: bool) -> Result<()> {
    if data_dir.exists() {
        bail!(
            "demo output already exists: {}\nNext: choose another --output path",
            data_dir.display()
        )
    }
    secure_dir(data_dir)?;
    let coordinator_dir = data_dir.join("coordinator");
    let mut coordinator = Coordinator::open(coordinator_dir.clone())?;
    let mut completed = Vec::new();

    for (index, default_skill) in ["move", "pick", "place"].into_iter().enumerate() {
        let robot_id = format!("sim-robot-{}", index + 1);
        let skill = if interactive {
            choose_skill(default_skill, &robot_id)?
        } else {
            default_skill.into()
        };
        let identity = HybridIdentity::generate(&robot_id);
        let robot_dir = data_dir.join(&robot_id);
        secure_dir(&robot_dir)?;
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
                skill: skill.clone(),
                arguments: BTreeMap::new(),
                requested_speed_mps: Some(0.5),
                requested_force_newtons: Some(20.0),
            },
            context: context.clone(),
            ttl_seconds: 30,
            constraints: PolicyConstraints {
                allowed_skills: vec![skill.clone()],
                allowed_zones: vec![context.zone.clone()],
                max_speed_mps: Some(1.0),
                max_force_newtons: Some(50.0),
            },
            risk,
            approvals,
        })?;
        if watch {
            println!(
                "\n{} {}",
                "token issued".green().bold(),
                short_id(&token.claims.token_id.to_string())
            );
            println!(
                "  robot={} sequence={} skill={} risk={:?} expires={}",
                token.claims.robot_id,
                token.claims.sequence,
                token.claims.action.skill,
                token.claims.risk,
                format_time(token.claims.expires_at_unix_ms)
            );
        }
        if interactive {
            pause(&format!("execute `{skill}` on {robot_id}"))?;
        }
        let mut runtime = RobotRuntime::open(
            &robot_id,
            identity,
            coordinator.identity.public.clone(),
            robot_dir.clone(),
        )?;
        let receipt = runtime.execute(&token, &context)?;
        println!("{} {} {}", "completed".green().bold(), robot_id, skill);
        if watch {
            print_json(&receipt, compact)?;
            print_audit(&robot_dir.join("audit.jsonl"), 0)?;
        }
        completed.push(receipt);
    }
    let result = serde_json::json!({
        "status": "completed", "robots": completed.len(), "receipts": completed,
        "coordinator_audit": coordinator_dir.join("audit.jsonl"), "output": data_dir,
    });
    println!("\n{}", "demo complete".green().bold());
    print_json(&result, compact)?;
    println!(
        "You should now see four audit chains under {}.",
        data_dir.display()
    );
    Ok(())
}

fn run_single_agent(
    data_dir: &Path,
    skill: &str,
    show_token: bool,
    watch: bool,
    compact: bool,
) -> Result<()> {
    if data_dir.exists() {
        bail!(
            "single-agent output already exists: {}\nNext: choose another --output path",
            data_dir.display()
        )
    }
    secure_dir(data_dir)?;

    let robot_id = "single-robot-1";
    let coordinator_dir = data_dir.join("coordinator");
    let robot_dir = data_dir.join(robot_id);
    secure_dir(&robot_dir)?;

    let mut coordinator = Coordinator::open(coordinator_dir.clone())?;
    let robot_identity = HybridIdentity::generate(robot_id);
    robot_identity.save_private(&robot_dir.join("identity.json"))?;
    robot_identity.save_public(&robot_dir.join("public.json"))?;
    coordinator.enroll(RobotRegistration {
        robot_id: robot_id.into(),
        display_name: "Single-Agent Simulator".into(),
        identity: robot_identity.public.clone(),
    })?;

    let context = ExecutionContext {
        task_id: "single-agent-loop".into(),
        zone: "simulation-cell-1".into(),
        state_hash: "sha256:single-agent-world-v1".into(),
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
        robot_id: robot_id.into(),
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
    let token_path = data_dir.join("token.json");
    fs::write(&token_path, serde_json::to_vec_pretty(&token)?)?;
    println!(
        "{} {} sequence={} action={}",
        "1. token issued".green().bold(),
        short_id(&token.claims.token_id.to_string()),
        token.claims.sequence,
        token.claims.action.skill
    );
    if show_token {
        print_json(&token, compact)?;
    }

    let robot_public = robot_identity.public.clone();
    let mut runtime = RobotRuntime::open(
        robot_id,
        robot_identity,
        coordinator.identity.public.clone(),
        robot_dir.clone(),
    )?;
    runtime.verify(&token, &context)?;
    println!("{}", "2. token verified".green().bold());

    let receipt = runtime.execute(&token, &context)?;
    let receipt_path = data_dir.join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{} {}", "3. action completed".green().bold(), skill);

    let coordinator_audit = coordinator_dir.join("audit.jsonl");
    let robot_audit = robot_dir.join("audit.jsonl");
    let coordinator_records = AuditLog::verify(
        &coordinator_audit,
        std::slice::from_ref(&coordinator.identity.public),
    )?;
    let robot_records = AuditLog::verify(&robot_audit, std::slice::from_ref(&robot_public))?;
    println!(
        "{} coordinator={} robot={}",
        "4. audit chains verified".green().bold(),
        coordinator_records.len(),
        robot_records.len()
    );
    if watch {
        println!("\n{}", "coordinator audit".cyan().bold());
        print_audit(&coordinator_audit, 0)?;
        println!("\n{}", "robot audit".cyan().bold());
        print_audit(&robot_audit, 0)?;
    }

    let result = serde_json::json!({
        "status": "completed",
        "robot_id": robot_id,
        "token_id": token.claims.token_id,
        "sequence": token.claims.sequence,
        "action": skill,
        "verification": "passed",
        "receipt": receipt,
        "token_file": token_path,
        "receipt_file": receipt_path,
        "coordinator_audit": {
            "file": coordinator_audit,
            "verified_records": coordinator_records.len(),
        },
        "robot_audit": {
            "file": robot_audit,
            "verified_records": robot_records.len(),
        },
        "output": data_dir,
    });
    println!("\n{}", "single-agent loop complete".green().bold());
    print_json(&result, compact)?;
    Ok(())
}

fn short_id(value: &str) -> &str {
    value.get(..8).unwrap_or(value)
}

struct CoordinatorApp {
    coordinator: Mutex<Coordinator>,
    metrics: Metrics,
}

struct RuntimeApp {
    runtime: Mutex<RobotRuntime>,
    metrics: Metrics,
}

type CoordinatorState = Arc<CoordinatorApp>;
type RuntimeState = Arc<RuntimeApp>;

async fn run_coordinator(bind: SocketAddr, data_dir: PathBuf) -> Result<()> {
    secure_dir(&data_dir)?;
    let state = Arc::new(CoordinatorApp {
        coordinator: Mutex::new(Coordinator::open(data_dir)?),
        metrics: Metrics::default(),
    });
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/metrics", get(coordinator_metrics))
        .route("/v1/status", get(coordinator_status))
        .route("/v1/fleet", get(coordinator_status))
        .route("/v1/robots", post(enroll))
        .route("/v1/robots/{robot_id}/revoke", post(revoke))
        .route("/v1/tokens", post(issue_token))
        .with_state(state)
        .layer(development_cors())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "coordination API listening; press Ctrl-C to stop");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    info!("coordination API stopped cleanly");
    Ok(())
}

async fn run_robot(
    bind: SocketAddr,
    robot_id: String,
    identity_path: PathBuf,
    coordinator_public_path: PathBuf,
    data_dir: PathBuf,
) -> Result<()> {
    secure_dir(&data_dir)?;
    let identity = HybridIdentity::load_private(&identity_path)?;
    if identity.public.subject != robot_id {
        bail!("robot ID does not match the identity subject")
    }
    let coordinator_public: PublicIdentity =
        serde_json::from_slice(&fs::read(coordinator_public_path)?)?;
    let runtime = RobotRuntime::open(robot_id, identity, coordinator_public, data_dir)?;
    let state = Arc::new(RuntimeApp {
        runtime: Mutex::new(runtime),
        metrics: Metrics::default(),
    });
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/metrics", get(runtime_metrics))
        .route("/v1/status", get(runtime_status))
        .route("/v1/execute", post(execute))
        .route("/v1/emergency-stop", post(emergency_stop))
        .with_state(state)
        .layer(development_cors())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "robot execution gate listening; press Ctrl-C to stop");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    info!("robot execution gate stopped cleanly");
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
}

async fn coordinator_metrics(State(state): State<CoordinatorState>) -> String {
    state.metrics.request();
    state.metrics.render("coordinator")
}

async fn runtime_metrics(State(state): State<RuntimeState>) -> String {
    state.metrics.request();
    state.metrics.render("robot")
}

async fn coordinator_status(State(state): State<CoordinatorState>) -> Json<serde_json::Value> {
    state.metrics.request();
    Json(state.coordinator.lock().await.status())
}

async fn enroll(
    State(state): State<CoordinatorState>,
    Json(request): Json<RobotRegistration>,
) -> ApiResult {
    state.metrics.request();
    let result = state.coordinator.lock().await.enroll(request);
    match result {
        Ok(record) => api_json(StatusCode::CREATED, record),
        Err(error) => {
            let api_error = ApiError::from(error);
            state.metrics.rejection(&api_error.body.code);
            Err(api_error)
        }
    }
}

async fn revoke(
    State(state): State<CoordinatorState>,
    AxumPath(robot_id): AxumPath<String>,
) -> ApiResult {
    state.metrics.request();
    let result = state.coordinator.lock().await.revoke(&robot_id);
    match result {
        Ok(()) => api_json(
            StatusCode::OK,
            serde_json::json!({"robot_id": robot_id, "revoked": true}),
        ),
        Err(error) => {
            let api_error = ApiError::from(error);
            state.metrics.rejection(&api_error.body.code);
            Err(api_error)
        }
    }
}

async fn issue_token(
    State(state): State<CoordinatorState>,
    Json(request): Json<TokenRequest>,
) -> ApiResult {
    state.metrics.request();
    let started = Instant::now();
    let result = state.coordinator.lock().await.issue_token(request);
    match result {
        Ok(token) => {
            state
                .metrics
                .token_issued(started.elapsed().as_micros() as u64);
            api_json(StatusCode::CREATED, token)
        }
        Err(error) => {
            let api_error = ApiError::from(error);
            state.metrics.rejection(&api_error.body.code);
            Err(api_error)
        }
    }
}

async fn runtime_status(State(state): State<RuntimeState>) -> Json<serde_json::Value> {
    state.metrics.request();
    Json(state.runtime.lock().await.status())
}

async fn execute(
    State(state): State<RuntimeState>,
    Json(request): Json<ExecuteRequest>,
) -> ApiResult {
    state.metrics.request();
    let result = state
        .runtime
        .lock()
        .await
        .execute(&request.token, &request.context);
    match result {
        Ok(receipt) => {
            state.metrics.execution();
            api_json(StatusCode::OK, receipt)
        }
        Err(error) => {
            let api_error = ApiError::from(error);
            state.metrics.rejection(&api_error.body.code);
            Err(api_error)
        }
    }
}

#[derive(Deserialize)]
struct EmergencyStopRequest {
    active: bool,
}

async fn emergency_stop(
    State(state): State<RuntimeState>,
    Json(request): Json<EmergencyStopRequest>,
) -> ApiResult {
    state.metrics.request();
    let mut runtime = state.runtime.lock().await;
    runtime
        .set_emergency_stop(request.active)
        .map_err(ApiError::internal)?;
    api_json(StatusCode::OK, runtime.status())
}

type ApiResult = Result<(StatusCode, Json<serde_json::Value>), ApiError>;

fn api_json<T: Serialize>(status: StatusCode, value: T) -> ApiResult {
    Ok((
        status,
        Json(serde_json::to_value(value).map_err(ApiError::internal)?),
    ))
}

struct ApiError {
    status: StatusCode,
    body: ApiErrorBody,
}

impl ApiError {
    fn new(status: StatusCode, code: &str, message: String, action: &str, retryable: bool) -> Self {
        Self {
            status,
            body: ApiErrorBody {
                code: code.into(),
                message,
                action: action.into(),
                retryable,
            },
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            error.to_string(),
            "Check service logs and persistent storage.",
            false,
        )
    }
}

impl From<CoordinatorError> for ApiError {
    fn from(error: CoordinatorError) -> Self {
        match error {
            CoordinatorError::UnknownRobot(id) => Self::new(
                StatusCode::NOT_FOUND,
                "IDENTITY_NOT_ENROLLED",
                format!("robot is not enrolled: {id}"),
                "Enroll the robot public identity with POST /v1/robots.",
                false,
            ),
            CoordinatorError::RevokedRobot(id) => Self::new(
                StatusCode::FORBIDDEN,
                "IDENTITY_REVOKED",
                format!("robot identity is revoked: {id}"),
                "Use another enrolled identity or complete an audited key-rotation process.",
                false,
            ),
            CoordinatorError::AlreadyEnrolled(id) => Self::new(
                StatusCode::CONFLICT,
                "IDENTITY_ALREADY_ENROLLED",
                format!("robot is already enrolled: {id}"),
                "Use the enrolled identity. Key replacement requires a separate rotation process.",
                false,
            ),
            CoordinatorError::SubjectMismatch => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "IDENTITY_SUBJECT_MISMATCH",
                error.to_string(),
                "Generate an identity whose subject equals the robot ID.",
                false,
            ),
            CoordinatorError::TwoPersonApprovalRequired => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "TWO_PERSON_APPROVAL_REQUIRED",
                error.to_string(),
                "Add two distinct --approver values or submit two approval objects.",
                true,
            ),
            CoordinatorError::InvalidTtl => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_TOKEN_TTL",
                error.to_string(),
                "Set ttl_seconds between 1 and 300.",
                true,
            ),
            CoordinatorError::Internal(error) => Self::internal(error),
        }
    }
}

impl From<ExecutionError> for ApiError {
    fn from(error: ExecutionError) -> Self {
        match &error {
            ExecutionError::EmergencyStop => Self::new(
                StatusCode::LOCKED,
                "EMERGENCY_STOP_ACTIVE",
                error.to_string(),
                "Clear the stop through an authorized local procedure before retrying.",
                true,
            ),
            ExecutionError::Schema => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNSUPPORTED_TOKEN_SCHEMA",
                error.to_string(),
                "Issue a new spacl.action-token.v1 token.",
                false,
            ),
            ExecutionError::WrongRobot => Self::new(
                StatusCode::FORBIDDEN,
                "WRONG_ROBOT",
                error.to_string(),
                "Send the token to the robot ID in the signed claims.",
                false,
            ),
            ExecutionError::InvalidSignature(_) => Self::new(
                StatusCode::FORBIDDEN,
                "INVALID_SIGNATURE",
                error.to_string(),
                "Discard the token and issue a new token from the pinned coordinator.",
                false,
            ),
            ExecutionError::Expired => Self::new(
                StatusCode::GONE,
                "TOKEN_EXPIRED",
                error.to_string(),
                "Issue a fresh token with the current execution context.",
                true,
            ),
            ExecutionError::Sequence { expected, .. } => Self::new(
                StatusCode::CONFLICT,
                "SEQUENCE_GAP",
                error.to_string(),
                &format!("Reconcile or execute sequence {expected} before later tokens."),
                true,
            ),
            ExecutionError::Context => Self::new(
                StatusCode::CONFLICT,
                "CONTEXT_MISMATCH",
                error.to_string(),
                "Refresh the world state and issue a token for the current context.",
                true,
            ),
            ExecutionError::Policy(_) => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "POLICY_DENIED",
                error.to_string(),
                "Reduce the requested limits or select an allowed skill and zone.",
                true,
            ),
            ExecutionError::Replay => Self::new(
                StatusCode::CONFLICT,
                "TOKEN_REPLAY",
                error.to_string(),
                "Do not retry a consumed token. Issue a new token.",
                false,
            ),
            ExecutionError::Internal(error) => Self::internal(error),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn development_cors() -> CorsLayer {
    use axum::http::{HeaderValue, Method, header::CONTENT_TYPE};
    CorsLayer::new()
        .allow_origin([
            HeaderValue::from_static("http://127.0.0.1:8000"),
            HeaderValue::from_static("http://localhost:8000"),
        ])
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE])
}
