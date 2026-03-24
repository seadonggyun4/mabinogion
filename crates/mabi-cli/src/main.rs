//! Mabinogion CLI
//!
//! Registry-driven command surface for protocol serving, inspection, validation,
//! and controller workflows.

use clap::{Args, Parser, Subcommand, ValueEnum};
use mabi_chaos::{ChaosConfig, ChaosRuntime};
use mabi_cli::commands::{
    run_scenario_on_session, RunCommand, ServeRuntimeCommand, ValidateCommand,
};
use mabi_cli::prelude::*;
use mabi_cli::runtime_registry::{protocol_catalog, workspace_protocol_registry};
use mabi_cli::validation::{parse_nonzero_count, parse_port};
use mabi_runtime::{ProtocolLaunchSpec, RuntimeSession};
use mabi_scenario::prelude::ScenarioValidator;
use mabi_scenario::{Scenario, ScenarioParser};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

/// ASCII Art Logo (Celtic knot: orange knot on dark gray background)
const LOGO: &str = "\x1b[38;5;236m\
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m##\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m#++##++*\x1b[38;5;236m%%\x1b[38;5;208m#+*++*\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m+*\x1b[38;5;236m%%%%%\x1b[38;5;208m++#\x1b[38;5;236m%%%%\x1b[38;5;208m#+*\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m+*\x1b[38;5;236m%%%\x1b[38;5;208m*\x1b[38;5;236m%%%\x1b[38;5;208m++\x1b[38;5;236m%%%\x1b[38;5;208m#+*\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m*+*\x1b[38;5;236m%%%%%%%\x1b[38;5;208m*+*\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m*+*\x1b[38;5;236m%%%%%%%\x1b[38;5;208m#+*\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%\x1b[38;5;208m*+++++#\x1b[38;5;236m%%\x1b[38;5;208m**\x1b[38;5;236m%%%\x1b[38;5;208m+\x1b[38;5;236m%%%%\x1b[38;5;208m++\x1b[38;5;236m%%%\x1b[38;5;208m*#\x1b[38;5;236m%%%\x1b[38;5;208m++++*\x1b[38;5;236m%%%%%%%%%%%%%
%%%%%%%%%%%%\x1b[38;5;208m++\x1b[38;5;236m%%%%\x1b[38;5;208m#++\x1b[38;5;236m%%%%%%\x1b[38;5;208m+*\x1b[38;5;236m%%\x1b[38;5;208m#+#\x1b[38;5;236m%%%\x1b[38;5;208m#++#\x1b[38;5;236m%%%%%\x1b[38;5;208m++\x1b[38;5;236m%%%%%%%%%%%%
%%%%%%%%%%%%\x1b[38;5;208m+*\x1b[38;5;236m%%%%%%\x1b[38;5;208m*+*\x1b[38;5;236m%%\x1b[38;5;208m#++\x1b[38;5;236m%%%%\x1b[38;5;208m++\x1b[38;5;236m%%%%%\x1b[38;5;208m#++\x1b[38;5;236m%%%%\x1b[38;5;208m#+#\x1b[38;5;236m%%%%%%%%%%%
%%%%%%%%%%%%%%%%\x1b[38;5;208m++#\x1b[38;5;236m%%%\x1b[38;5;208m*++*#\x1b[38;5;236m%%%%%%\x1b[38;5;208m#+++#\x1b[38;5;236m%%%\x1b[38;5;208m*#\x1b[38;5;236m%%\x1b[38;5;208m#+*\x1b[38;5;236m%%%%%%%%%%%%
%%%%%%%%%%%%%\x1b[38;5;208m#++#\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m#++\x1b[38;5;236m%%%%%%%%%%%%%%
%%%%%%%%%%%%\x1b[38;5;208m*+#\x1b[38;5;236m%%\x1b[38;5;208m##\x1b[38;5;236m%%%\x1b[38;5;208m#*+*#\x1b[38;5;236m%%%%%%\x1b[38;5;208m#***#\x1b[38;5;236m%%%\x1b[38;5;208m#++\x1b[38;5;236m%%%%%%%%%%%%%%%%
%%%%%%%%%%%\x1b[38;5;208m#+#\x1b[38;5;236m%%%%\x1b[38;5;208m++#\x1b[38;5;236m%%%%\x1b[38;5;208m#++\x1b[38;5;236m%%%%\x1b[38;5;208m++#\x1b[38;5;236m%\x1b[38;5;208m#++#\x1b[38;5;236m%%%%%%\x1b[38;5;208m*+\x1b[38;5;236m%%%%%%%%%%%%
%%%%%%%%%%%%\x1b[38;5;208m++\x1b[38;5;236m%%%%%\x1b[38;5;208m#++\x1b[38;5;236m%%%%%\x1b[38;5;208m+*\x1b[38;5;236m%%\x1b[38;5;208m#+\x1b[38;5;236m%%%%%\x1b[38;5;208m#++#\x1b[38;5;236m%%%%\x1b[38;5;208m++\x1b[38;5;236m%%%%%%%%%%%%
%%%%%%%%%%%%%\x1b[38;5;208m#++++\x1b[38;5;236m%%%\x1b[38;5;208m#+#\x1b[38;5;236m%%\x1b[38;5;208m*+\x1b[38;5;236m%%%%\x1b[38;5;208m+#\x1b[38;5;236m%%\x1b[38;5;208m**\x1b[38;5;236m%%\x1b[38;5;208m#++*+++\x1b[38;5;236m%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m*+*\x1b[38;5;236m%%%%%%%\x1b[38;5;208m*+*\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m#++\x1b[38;5;236m%%%%%%%\x1b[38;5;208m#+*\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m#+#\x1b[38;5;236m%%%\x1b[38;5;208m++\x1b[38;5;236m%%%\x1b[38;5;208m*\x1b[38;5;236m%%%\x1b[38;5;208m*+\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m#+#\x1b[38;5;236m%%%%\x1b[38;5;208m#++\x1b[38;5;236m%%%%%\x1b[38;5;208m*+\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m*++**#\x1b[38;5;236m%%\x1b[38;5;208m*++##++#\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%\x1b[38;5;208m##\x1b[38;5;236m%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%\x1b[0m";

#[derive(Parser)]
#[command(
    name = "mabi",
    version,
    about = "Industrial protocol simulator for testing and development",
    long_about = "Mabinogion is a high-performance simulator for industrial protocols including \
                  Modbus TCP/RTU, OPC UA, BACnet/IP, and KNXnet/IP. It exposes a shared runtime, \
                  controller surfaces for scenario and chaos workflows, and registry-driven CLI \
                  inspection tools.",
    before_help = LOGO
)]
struct Cli {
    #[command(flatten)]
    global: GlobalArgs,

    #[command(flatten)]
    output: OutputArgs,

    #[command(flatten)]
    runtime: RuntimeArgs,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Args, Clone)]
struct GlobalArgs {
    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Suppress all output except errors
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Configuration file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Args, Clone)]
struct OutputArgs {
    /// Output format
    #[arg(long, value_enum, default_value = "table", global = true)]
    format: OutputFormatArg,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,
}

#[derive(Args, Clone)]
struct RuntimeArgs {
    /// Maximum time to wait for service readiness
    #[arg(long, default_value = "5s", global = true)]
    readiness_timeout: String,
}

#[derive(ValueEnum, Clone, Copy, Debug, Default)]
enum OutputFormatArg {
    #[default]
    Table,
    Json,
    Yaml,
    Compact,
}

impl From<OutputFormatArg> for OutputFormat {
    fn from(arg: OutputFormatArg) -> Self {
        match arg {
            OutputFormatArg::Table => OutputFormat::Table,
            OutputFormatArg::Json => OutputFormat::Json,
            OutputFormatArg::Yaml => OutputFormat::Yaml,
            OutputFormatArg::Compact => OutputFormat::Compact,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Serve a protocol simulator through the shared runtime
    Serve(ServeCommandArgs),

    /// Scenario controller commands
    Scenario(ScenarioCommandArgs),

    /// Chaos controller commands
    Chaos(ChaosCommandArgs),

    /// Inspect runtime and schema surfaces
    Inspect(InspectCommandArgs),

    /// Validate scenario or config files
    Validate(ValidateCommandArgs),

    /// Show version information
    Version,
}

#[derive(Args)]
struct ServeCommandArgs {
    #[command(subcommand)]
    protocol: ServeProtocolCommand,
}

#[derive(Subcommand)]
enum ServeProtocolCommand {
    Modbus(ModbusServeArgs),
    Opcua(OpcuaServeArgs),
    Bacnet(BacnetServeArgs),
    Knx(KnxServeArgs),
}

#[derive(Args, Clone, Default)]
struct ServeArgs {
    /// Stable runtime service name
    #[arg(long)]
    name: Option<String>,
}

#[derive(Args, Clone)]
struct ModbusServeArgs {
    #[command(flatten)]
    serve: ServeArgs,

    /// Port to bind to
    #[arg(short, long, default_value = "502", value_parser = parse_port)]
    port: u16,

    /// Bind address
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    /// Number of devices (unit IDs) to simulate
    #[arg(short, long, default_value = "1", value_parser = parse_nonzero_count)]
    devices: usize,

    /// Number of points per device
    #[arg(long, default_value = "100", value_parser = parse_nonzero_count)]
    points: usize,
}

#[derive(Args, Clone)]
struct OpcuaServeArgs {
    #[command(flatten)]
    serve: ServeArgs,

    /// Port to bind to
    #[arg(short, long, default_value = "4840", value_parser = parse_port)]
    port: u16,

    /// Bind address
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    /// Endpoint path
    #[arg(long, default_value = "/")]
    endpoint: String,

    /// Number of nodes to create
    #[arg(short, long, default_value = "1000", value_parser = parse_nonzero_count)]
    nodes: usize,

    /// Security mode (None, Sign, SignAndEncrypt)
    #[arg(long, value_enum, default_value = "none", ignore_case = true)]
    security: SecurityModeArg,
}

#[derive(Args, Clone)]
struct BacnetServeArgs {
    #[command(flatten)]
    serve: ServeArgs,

    /// Port to bind to
    #[arg(short, long, default_value = "47808", value_parser = parse_port)]
    port: u16,

    /// Bind address
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    /// Device instance number
    #[arg(short, long, default_value = "1234")]
    instance: u32,

    /// Number of objects to create
    #[arg(short, long, default_value = "100", value_parser = parse_nonzero_count)]
    objects: usize,

    /// Enable BBMD functionality
    #[arg(long)]
    bbmd: bool,
}

#[derive(Args, Clone)]
struct KnxServeArgs {
    #[command(flatten)]
    serve: ServeArgs,

    /// Port to bind to
    #[arg(short, long, default_value = "3671", value_parser = parse_port)]
    port: u16,

    /// Bind address
    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    /// Individual address (e.g., "1.1.1")
    #[arg(short, long, default_value = "1.1.1")]
    address: String,

    /// Number of group objects
    #[arg(short, long, default_value = "100", value_parser = parse_nonzero_count)]
    groups: usize,
}

#[derive(Args)]
struct ScenarioCommandArgs {
    #[command(subcommand)]
    command: ScenarioSubcommand,
}

#[derive(Subcommand)]
enum ScenarioSubcommand {
    /// Run a scenario controller workflow
    Run(ScenarioRunArgs),
}

#[derive(Args)]
struct ScenarioRunArgs {
    /// Path to scenario file (YAML/JSON)
    #[arg(required = true)]
    scenario: PathBuf,

    /// Time scale factor (1.0 = real-time, 2.0 = 2x faster)
    #[arg(short = 's', long, default_value = "1.0")]
    time_scale: f64,

    /// Maximum duration to run
    #[arg(short, long)]
    duration: Option<String>,

    /// Validate scenario without running
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
struct ChaosCommandArgs {
    #[command(subcommand)]
    command: ChaosSubcommand,
}

#[derive(Subcommand)]
enum ChaosSubcommand {
    /// Validate and stage a chaos configuration
    Run(ChaosRunArgs),
}

#[derive(Args)]
struct ChaosRunArgs {
    /// Path to chaos config file (YAML/JSON)
    #[arg(required = true)]
    config: PathBuf,

    /// Maximum duration to keep the chaos runtime active
    #[arg(short, long)]
    duration: Option<String>,

    /// Validate config without entering the runtime loop
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
struct InspectCommandArgs {
    #[command(subcommand)]
    command: InspectSubcommand,
}

#[derive(Subcommand)]
enum InspectSubcommand {
    /// Show registered protocols
    Protocols,

    /// Show supported schema surfaces
    Schema(InspectSchemaArgs),

    /// Show current process runtime status
    Status,
}

#[derive(Args)]
struct InspectSchemaArgs {
    #[arg(value_enum, default_value = "all")]
    kind: SchemaKindArg,
}

#[derive(ValueEnum, Clone, Copy, Debug, Default)]
enum SchemaKindArg {
    #[default]
    All,
    Scenario,
    Chaos,
    Config,
}

#[derive(Args)]
struct ValidateCommandArgs {
    #[command(subcommand)]
    command: ValidateSubcommand,
}

#[derive(Subcommand)]
enum ValidateSubcommand {
    /// Validate a scenario schema file
    Scenario(ValidateScenarioArgs),

    /// Validate one or more generic config files
    Config(ValidateConfigArgs),
}

#[derive(Args)]
struct ValidateScenarioArgs {
    /// Scenario file to validate
    #[arg(required = true)]
    file: PathBuf,

    /// Show detailed issues
    #[arg(short, long)]
    detailed: bool,

    /// Treat warnings as errors
    #[arg(long)]
    strict: bool,
}

#[derive(Args)]
struct ValidateConfigArgs {
    /// Config files to validate
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Show detailed validation results
    #[arg(short, long)]
    detailed: bool,

    /// Treat warnings as errors
    #[arg(long)]
    strict: bool,
}

/// OPC UA security mode argument.
#[derive(ValueEnum, Clone, Copy, Debug, Default)]
enum SecurityModeArg {
    #[default]
    None,
    Sign,
    #[value(name = "SignAndEncrypt")]
    SignAndEncrypt,
}

impl std::fmt::Display for SecurityModeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Sign => write!(f, "Sign"),
            Self::SignAndEncrypt => write!(f, "SignAndEncrypt"),
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let verbosity = if cli.global.quiet {
        0
    } else {
        cli.global.verbose.saturating_add(1)
    };

    let ctx_result = CliContext::builder()
        .output_format(cli.output.format.into())
        .verbosity(verbosity)
        .colors(!cli.output.no_color && console::colors_enabled())
        .build();

    let ctx = match ctx_result {
        Ok(ctx) => ctx,
        Err(error) => {
            eprintln!("Failed to initialize CLI: {}", error);
            return ExitCode::from(1);
        }
    };

    if verbosity >= 2 {
        let log_level = match verbosity {
            2 => LogLevel::Info,
            3 => LogLevel::Debug,
            _ => LogLevel::Trace,
        };
        let mut log_config = LogConfig::development();
        log_config.level = log_level;
        if let Err(error) = init_logging(&log_config) {
            eprintln!("Warning: Failed to initialize logging: {}", error);
        }
    }

    let mut runner = CommandRunner::new(ctx);
    runner.add_hook(LoggingHook);
    runner.add_hook(MetricsHook::new());

    let readiness_timeout = match parse_duration(&cli.runtime.readiness_timeout) {
        Ok(timeout) => timeout,
        Err(error) => {
            eprintln!("Invalid readiness timeout: {}", error);
            return ExitCode::from(2);
        }
    };

    let result = match cli.command {
        Commands::Serve(args) => match into_launch_spec(args.protocol) {
            Ok(launch) => {
                let cmd = ServeRuntimeCommand::new(launch, readiness_timeout);
                runner.run_with_shutdown(&cmd).await
            }
            Err(error) => Err(error),
        },
        Commands::Scenario(args) => match args.command {
            ScenarioSubcommand::Run(args) => {
                let mut cmd = RunCommand::new(args.scenario)
                    .with_time_scale(args.time_scale)
                    .with_dry_run(args.dry_run)
                    .with_readiness_timeout(readiness_timeout);
                if let Some(duration) = args.duration {
                    match parse_duration(&duration) {
                        Ok(duration) => cmd = cmd.with_duration(duration),
                        Err(error) => {
                            eprintln!("Invalid duration: {}", error);
                            return ExitCode::from(2);
                        }
                    }
                }
                runner.run_with_shutdown(&cmd).await
            }
        },
        Commands::Chaos(args) => {
            let ctx_handle = runner.context();
            let mut ctx = ctx_handle.write().await;
            match args.command {
                ChaosSubcommand::Run(args) => run_chaos(&mut ctx, args, readiness_timeout).await,
            }
        }
        Commands::Inspect(args) => {
            let ctx_handle = runner.context();
            let mut ctx = ctx_handle.write().await;
            match args.command {
                InspectSubcommand::Protocols => inspect_protocols(&mut ctx).await,
                InspectSubcommand::Schema(args) => inspect_schema(&mut ctx, args.kind).await,
                InspectSubcommand::Status => inspect_status(&mut ctx).await,
            }
        }
        Commands::Validate(args) => match args.command {
            ValidateSubcommand::Scenario(args) => {
                let ctx_handle = runner.context();
                let mut ctx = ctx_handle.write().await;
                validate_scenario(&mut ctx, args).await
            }
            ValidateSubcommand::Config(args) => {
                let cmd = ValidateCommand::new(args.files)
                    .with_detailed(args.detailed)
                    .with_strict(args.strict);
                runner.run(&cmd).await
            }
        },
        Commands::Version => {
            println!("mabi {} (Mabinogion)", env!("CARGO_PKG_VERSION"));
            println!("Rust {}", rustc_version());
            println!();
            println!("Registered protocols:");
            for entry in protocol_catalog() {
                println!(
                    "  - {} ({})",
                    entry.descriptor.display_name, entry.descriptor.key
                );
            }
            return ExitCode::SUCCESS;
        }
    };

    match result {
        Ok(output) => {
            if let Some(message) = output.message {
                if output.exit_code == 0 {
                    println!("{}", message);
                } else {
                    eprintln!("{}", message);
                }
            }
            ExitCode::from(output.exit_code as u8)
        }
        Err(error) => {
            if matches!(error, CliError::Interrupted) {
                return ExitCode::from(130);
            }
            eprintln!("Error: {}", error);
            ExitCode::from(error.exit_code() as u8)
        }
    }
}

fn into_launch_spec(command: ServeProtocolCommand) -> CliResult<ProtocolLaunchSpec> {
    match command {
        ServeProtocolCommand::Modbus(args) => Ok(ProtocolLaunchSpec {
            protocol: "modbus".into(),
            name: args.serve.name,
            config: json!({
                "bind_addr": parse_bind_addr(&args.bind, args.port)?,
                "devices": args.devices,
                "points_per_device": args.points,
            }),
        }),
        ServeProtocolCommand::Opcua(args) => Ok(ProtocolLaunchSpec {
            protocol: "opcua".into(),
            name: args.serve.name,
            config: json!({
                "bind_addr": parse_bind_addr(&args.bind, args.port)?,
                "endpoint_path": args.endpoint,
                "nodes": args.nodes,
                "security_mode": args.security.to_string(),
            }),
        }),
        ServeProtocolCommand::Bacnet(args) => Ok(ProtocolLaunchSpec {
            protocol: "bacnet".into(),
            name: args.serve.name,
            config: json!({
                "bind_addr": parse_bind_addr(&args.bind, args.port)?,
                "device_instance": args.instance,
                "objects": args.objects,
                "bbmd_enabled": args.bbmd,
            }),
        }),
        ServeProtocolCommand::Knx(args) => Ok(ProtocolLaunchSpec {
            protocol: "knx".into(),
            name: args.serve.name,
            config: json!({
                "bind_addr": parse_bind_addr(&args.bind, args.port)?,
                "individual_address": args.address,
                "group_objects": args.groups,
            }),
        }),
    }
}

fn parse_bind_addr(bind: &str, port: u16) -> CliResult<std::net::SocketAddr> {
    format!("{}:{}", bind, port)
        .parse()
        .map_err(|error| CliError::InvalidConfig {
            message: format!("invalid bind address {}:{} ({})", bind, port, error),
        })
}

async fn inspect_protocols(ctx: &mut CliContext) -> CliResult<CommandOutput> {
    let catalog = protocol_catalog();
    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&catalog)?;
        return Ok(CommandOutput::quiet_success());
    }

    ctx.output().header("Registered Protocols");
    let mut table =
        TableBuilder::new(ctx.colors_enabled()).header(["Key", "Name", "Port", "Features"]);
    for entry in catalog {
        table = table.row([
            entry.descriptor.key,
            entry.descriptor.display_name,
            &entry.descriptor.default_port.to_string(),
            &entry.features.join(", "),
        ]);
    }
    table.print();
    Ok(CommandOutput::quiet_success())
}

async fn inspect_schema(ctx: &mut CliContext, kind: SchemaKindArg) -> CliResult<CommandOutput> {
    #[derive(Serialize)]
    struct SchemaSurface<'a> {
        kind: &'a str,
        formats: Vec<&'a str>,
        entrypoint: &'a str,
        notes: Vec<&'a str>,
    }

    let surfaces = vec![
        SchemaSurface {
            kind: "scenario",
            formats: vec!["yaml", "json"],
            entrypoint: "mabi scenario run <file> / mabi validate scenario <file>",
            notes: vec!["Validated with mabi-scenario parser and validator"],
        },
        SchemaSurface {
            kind: "chaos",
            formats: vec!["yaml", "json"],
            entrypoint: "mabi chaos run <file>",
            notes: vec!["Validated with mabi-chaos config parser"],
        },
        SchemaSurface {
            kind: "config",
            formats: vec!["yaml", "json", "toml"],
            entrypoint: "mabi validate config <file...>",
            notes: vec!["Generic file validation surface for workspace configs"],
        },
    ];

    let selected: Vec<_> = surfaces
        .into_iter()
        .filter(|surface| match kind {
            SchemaKindArg::All => true,
            SchemaKindArg::Scenario => surface.kind == "scenario",
            SchemaKindArg::Chaos => surface.kind == "chaos",
            SchemaKindArg::Config => surface.kind == "config",
        })
        .collect();

    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&selected)?;
        return Ok(CommandOutput::quiet_success());
    }

    ctx.output().header("Schema Surfaces");
    for surface in selected {
        ctx.output().kv("Kind", surface.kind);
        ctx.output().kv("Formats", surface.formats.join(", "));
        ctx.output().kv("Entrypoint", surface.entrypoint);
        ctx.output().kv("Notes", surface.notes.join("; "));
    }
    Ok(CommandOutput::quiet_success())
}

async fn inspect_status(ctx: &mut CliContext) -> CliResult<CommandOutput> {
    #[derive(Serialize)]
    struct RuntimeStatus<'a> {
        active_services: usize,
        model: &'a str,
        note: &'a str,
    }

    let status = RuntimeStatus {
        active_services: 0,
        model: "shared runtime / per-process lifecycle",
        note: "Status is process-scoped; this invocation has not attached persistent services.",
    };

    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&status)?;
        return Ok(CommandOutput::quiet_success());
    }

    ctx.output().header("Runtime Status");
    ctx.output().kv("Active Services", status.active_services);
    ctx.output().kv("Model", status.model);
    ctx.output().kv("Note", status.note);
    Ok(CommandOutput::quiet_success())
}

async fn validate_scenario(
    ctx: &mut CliContext,
    args: ValidateScenarioArgs,
) -> CliResult<CommandOutput> {
    #[derive(Serialize)]
    struct IssueView {
        severity: String,
        code: String,
        path: String,
        message: String,
    }

    #[derive(Serialize)]
    struct ScenarioValidationReport {
        path: String,
        name: String,
        valid: bool,
        errors: usize,
        warnings: usize,
        issues: Vec<IssueView>,
    }

    let path = ctx.resolve_path(&args.file);
    if !path.exists() {
        return Err(CliError::ScenarioNotFound { path });
    }

    let scenario =
        ScenarioParser::load(&path)
            .await
            .map_err(|error| CliError::InvalidScenario {
                message: error.to_string(),
            })?;
    ScenarioParser::validate(&scenario).map_err(|error| CliError::InvalidScenario {
        message: error.to_string(),
    })?;

    let validator = ScenarioValidator::new();
    let result = validator.validate(&scenario);
    let issues: Vec<IssueView> = result
        .issues()
        .iter()
        .map(|issue| IssueView {
            severity: format!("{:?}", issue.severity).to_lowercase(),
            code: format!("{:?}", issue.code),
            path: issue.path.clone(),
            message: issue.message.clone(),
        })
        .collect();
    let errors = result.errors().len();
    let warnings = result.warnings().len();
    let report = ScenarioValidationReport {
        path: path.display().to_string(),
        name: scenario.name.clone(),
        valid: result.is_valid() && (!args.strict || warnings == 0),
        errors,
        warnings,
        issues,
    };

    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&report)?;
    } else {
        ctx.output().header("Scenario Validation");
        ctx.output().kv("Path", &report.path);
        ctx.output().kv("Name", &report.name);
        ctx.output().kv("Errors", report.errors);
        ctx.output().kv("Warnings", report.warnings);
        if args.detailed && !report.issues.is_empty() {
            for issue in &report.issues {
                ctx.output().kv(
                    format!("{} {}", issue.severity, issue.code),
                    format!("{} ({})", issue.message, issue.path),
                );
            }
        }
    }

    if report.valid {
        if !ctx.is_quiet() {
            ctx.output().success("Scenario validation passed");
        }
        Ok(CommandOutput::quiet_success())
    } else {
        Err(CliError::validation_failed(report.issues.iter().map(
            |issue| format!("{} [{}] {}", issue.path, issue.code, issue.message),
        )))
    }
}

async fn run_chaos(
    ctx: &mut CliContext,
    args: ChaosRunArgs,
    readiness_timeout: Duration,
) -> CliResult<CommandOutput> {
    #[derive(Serialize)]
    struct ChaosSummary {
        path: String,
        enabled: bool,
        faults: usize,
        schedules: usize,
        services: usize,
        scenario: Option<String>,
        state: &'static str,
    }

    let path = ctx.resolve_path(&args.config);
    if !path.exists() {
        return Err(CliError::ConfigNotFound { path });
    }

    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let config = match extension {
        "yaml" | "yml" => ChaosConfig::from_yaml_file(&path),
        "json" => ChaosConfig::from_json_file(&path),
        _ => {
            return Err(CliError::InvalidConfig {
                message: format!("unsupported chaos config extension: {}", extension),
            });
        }
    }
    .map_err(|error| CliError::InvalidConfig {
        message: error.to_string(),
    })?;
    config.validate().map_err(|error| CliError::InvalidConfig {
        message: error.to_string(),
    })?;

    let summary = ChaosSummary {
        path: path.display().to_string(),
        enabled: config.global.enabled,
        faults: config.faults.len(),
        schedules: config.schedules.len(),
        services: config
            .session
            .as_ref()
            .map(|session| session.services.len())
            .unwrap_or(0),
        scenario: config
            .scenario
            .as_ref()
            .map(|scenario| scenario.path.clone()),
        state: if args.dry_run {
            "validated"
        } else {
            "configured"
        },
    };

    if !matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().header("Chaos Runtime");
        ctx.output().kv("Path", &summary.path);
        ctx.output().kv("Enabled", summary.enabled);
        ctx.output().kv("Faults", summary.faults);
        ctx.output().kv("Schedules", summary.schedules);
        ctx.output().kv("Services", summary.services);
        if let Some(scenario) = &summary.scenario {
            ctx.output().kv("Scenario", scenario);
        }
    }

    if args.dry_run {
        if matches!(
            ctx.output().format(),
            OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
        ) {
            ctx.output().write(&summary)?;
        }
        if !ctx.is_quiet() {
            ctx.output().success("Chaos config validation passed");
        }
        return Ok(CommandOutput::quiet_success());
    }

    let session_spec = config
        .session
        .clone()
        .ok_or_else(|| CliError::InvalidConfig {
            message: "chaos execution requires a top-level session block".into(),
        })?;
    if session_spec.services.is_empty() {
        return Err(CliError::InvalidConfig {
            message: "chaos session.services must contain at least one service".into(),
        });
    }

    let chaos_runtime =
        ChaosRuntime::new(config.clone()).map_err(|error| CliError::InvalidConfig {
            message: error.to_string(),
        })?;
    let registry = workspace_protocol_registry();
    let session = RuntimeSession::new(
        session_spec.clone(),
        &registry,
        chaos_runtime.runtime_extensions(),
    )
    .await?;

    chaos_runtime
        .start()
        .await
        .map_err(|error| CliError::ExecutionFailed {
            message: format!("failed to start chaos runtime: {}", error),
        })?;
    if let Err(error) = session.start(readiness_timeout).await {
        let _ = chaos_runtime.stop().await;
        return Err(error.into());
    }

    let run_result = if let Some(invocation) = config.scenario.clone() {
        let scenario_path = resolve_relative_path(path.parent(), &invocation.path);
        let scenario = load_scenario_for_runtime(&scenario_path).await?;
        let (_scenario_summary, _, _) = run_scenario_on_session(
            ctx,
            &scenario_path,
            scenario,
            &session,
            invocation.time_scale.unwrap_or(1.0),
            invocation.duration_secs.map(Duration::from_secs),
        )
        .await?;
        Ok::<(), CliError>(())
    } else {
        if !ctx.is_quiet() {
            ctx.output().info("Press Ctrl+C to stop");
        }
        if let Some(duration) = args.duration {
            let duration = parse_duration(&duration).map_err(|error| CliError::InvalidConfig {
                message: format!("invalid duration: {}", error),
            })?;
            let shutdown_signal = ctx.shutdown_signal();
            tokio::select! {
                _ = tokio::time::sleep(duration) => {}
                _ = shutdown_signal.notified() => {}
            }
        } else {
            ctx.shutdown_signal().notified().await;
        }
        Ok(())
    };

    let session_stop = session.stop().await;
    let chaos_stop = chaos_runtime.stop().await;
    run_result?;
    session_stop?;
    chaos_stop.map_err(|error| CliError::ExecutionFailed {
        message: format!("failed to stop chaos runtime: {}", error),
    })?;

    let final_summary = ChaosSummary {
        state: "completed",
        ..summary
    };
    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&final_summary)?;
    } else if !ctx.is_quiet() {
        ctx.output().success("Chaos runtime stopped");
    }

    Ok(CommandOutput::quiet_success())
}

fn resolve_relative_path(base: Option<&std::path::Path>, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.unwrap_or_else(|| std::path::Path::new(".")).join(path)
    }
}

async fn load_scenario_for_runtime(path: &Path) -> CliResult<Scenario> {
    let scenario = ScenarioParser::load(path)
        .await
        .map_err(|error| CliError::InvalidScenario {
            message: error.to_string(),
        })?;
    ScenarioParser::validate(&scenario).map_err(|error| CliError::InvalidScenario {
        message: error.to_string(),
    })?;
    let validation = ScenarioValidator::new().validate(&scenario);
    if !validation.is_valid() {
        return Err(CliError::validation_failed(validation.errors().iter().map(
            |issue| format!("{} [{:?}] {}", issue.path, issue.code, issue.message),
        )));
    }
    Ok(scenario)
}

fn parse_duration(value: &str) -> std::result::Result<Duration, String> {
    humantime::parse_duration(value).map_err(|error| error.to_string())
}

fn rustc_version() -> &'static str {
    env!("CARGO_PKG_RUST_VERSION")
}

#[cfg(test)]
mod tests {
    use super::{
        into_launch_spec, ModbusServeArgs, SecurityModeArg, ServeArgs, ServeProtocolCommand,
    };

    #[test]
    fn serve_request_maps_to_runtime_request() {
        let request = into_launch_spec(ServeProtocolCommand::Modbus(ModbusServeArgs {
            serve: ServeArgs::default(),
            port: 1502,
            bind: "127.0.0.1".into(),
            devices: 2,
            points: 32,
        }))
        .unwrap();

        assert_eq!(request.protocol, "modbus");
        assert_eq!(request.config["bind_addr"], "127.0.0.1:1502");
        assert_eq!(request.config["devices"], 2);
        assert_eq!(request.config["points_per_device"], 32);
    }

    #[test]
    fn security_mode_display_is_stable() {
        assert_eq!(SecurityModeArg::None.to_string(), "None");
        assert_eq!(SecurityModeArg::Sign.to_string(), "Sign");
        assert_eq!(
            SecurityModeArg::SignAndEncrypt.to_string(),
            "SignAndEncrypt"
        );
    }
}
