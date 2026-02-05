//! Mabinogion CLI
//!
//! A command-line interface for running industrial protocol simulators.

use clap::{Args, Parser, Subcommand, ValueEnum};
use mabi_cli::prelude::*;
use mabi_cli::validation::{parse_nonzero_count, parse_port, parse_tag, tags_from_entries, TagEntry};
use std::path::PathBuf;
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

/// Mabinogion - Industrial protocol simulation tool
#[derive(Parser)]
#[command(
    name = "mabi",
    version,
    about = "Industrial protocol simulator for testing and development",
    long_about = "Mabinogion is a high-performance simulator for industrial protocols including \
                  Modbus TCP/RTU, OPC UA, BACnet/IP, and KNXnet/IP. It supports scenario-based \
                  simulation, chaos engineering, and stress testing.",
    before_help = LOGO
)]
struct Cli {
    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Suppress all output except errors
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Output format
    #[arg(long, value_enum, default_value = "table", global = true)]
    format: OutputFormatArg,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,

    /// Configuration file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
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
    /// Run a simulation scenario
    Run(RunArgs),

    /// Start a Modbus TCP/RTU simulator
    Modbus(ModbusArgs),

    /// Start an OPC UA server simulator
    Opcua(OpcuaArgs),

    /// Start a BACnet/IP simulator
    Bacnet(BacnetArgs),

    /// Start a KNXnet/IP simulator
    Knx(KnxArgs),

    /// Validate scenario or configuration files
    Validate(ValidateArgs),

    /// List resources (devices, protocols, points, scenarios)
    List(ListArgs),

    /// Show version information
    Version,
}

// =============================================================================
// Command Arguments
// =============================================================================

#[derive(Args)]
struct RunArgs {
    /// Path to scenario file (YAML)
    #[arg(required = true)]
    scenario: PathBuf,

    /// Time scale factor (1.0 = real-time, 2.0 = 2x faster)
    #[arg(short = 's', long, default_value = "1.0")]
    time_scale: f64,

    /// Maximum duration to run (e.g., "10s", "5m", "1h")
    #[arg(short, long)]
    duration: Option<String>,

    /// Validate scenario without running
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
struct ModbusArgs {
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

    /// Use RTU mode (requires --serial)
    #[arg(long)]
    rtu: bool,

    /// Serial port for RTU mode
    #[arg(long, requires = "rtu")]
    serial: Option<String>,

    /// Device tags (format: key=value or label, can be repeated)
    #[arg(long = "tag", value_parser = parse_tag)]
    tags: Vec<TagEntry>,
}

#[derive(Args)]
struct OpcuaArgs {
    /// Port to bind to
    #[arg(short, long, default_value = "4840", value_parser = parse_port)]
    port: u16,

    /// Endpoint path
    #[arg(long, default_value = "/")]
    endpoint: String,

    /// Number of nodes to create
    #[arg(short, long, default_value = "1000", value_parser = parse_nonzero_count)]
    nodes: usize,

    /// Security mode (None, Sign, SignAndEncrypt)
    #[arg(long, value_enum, default_value = "none", ignore_case = true)]
    security: SecurityModeArg,

    /// Device tags (format: key=value or label, can be repeated)
    #[arg(long = "tag", value_parser = parse_tag)]
    tags: Vec<TagEntry>,
}

#[derive(Args)]
struct BacnetArgs {
    /// Port to bind to
    #[arg(short, long, default_value = "47808", value_parser = parse_port)]
    port: u16,

    /// Device instance number
    #[arg(short, long, default_value = "1234")]
    instance: u32,

    /// Number of objects to create
    #[arg(short, long, default_value = "100", value_parser = parse_nonzero_count)]
    objects: usize,

    /// Enable BBMD functionality
    #[arg(long)]
    bbmd: bool,

    /// Device tags (format: key=value or label, can be repeated)
    #[arg(long = "tag", value_parser = parse_tag)]
    tags: Vec<TagEntry>,
}

#[derive(Args)]
struct KnxArgs {
    /// Port to bind to
    #[arg(short, long, default_value = "3671", value_parser = parse_port)]
    port: u16,

    /// Individual address (e.g., "1.1.1")
    #[arg(short, long, default_value = "1.1.1")]
    address: String,

    /// Number of group objects
    #[arg(short, long, default_value = "100", value_parser = parse_nonzero_count)]
    groups: usize,

    /// Device tags (format: key=value or label, can be repeated)
    #[arg(long = "tag", value_parser = parse_tag)]
    tags: Vec<TagEntry>,
}

#[derive(Args)]
struct ValidateArgs {
    /// Files to validate
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Show detailed validation results
    #[arg(short, long)]
    detailed: bool,

    /// Treat warnings as errors
    #[arg(long)]
    strict: bool,
}

#[derive(Args)]
struct ListArgs {
    /// Resource type to list
    #[arg(value_enum, default_value = "devices")]
    resource: ListResourceArg,

    /// Filter by protocol
    #[arg(short, long)]
    protocol: Option<String>,

    /// Filter by device ID pattern
    #[arg(short, long)]
    filter: Option<String>,

    /// Maximum number of items to show
    #[arg(short, long)]
    limit: Option<usize>,
}

/// OPC UA security mode argument.
#[derive(ValueEnum, Clone, Copy, Debug, Default)]
enum SecurityModeArg {
    /// No security
    #[default]
    None,
    /// Messages are signed
    Sign,
    /// Messages are signed and encrypted
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

#[derive(ValueEnum, Clone, Copy, Debug, Default)]
enum ListResourceArg {
    #[default]
    Devices,
    Protocols,
    Points,
    Scenarios,
}

// =============================================================================
// Main Entry Point
// =============================================================================

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // Determine verbosity
    let verbosity = if cli.quiet {
        0
    } else {
        cli.verbose.saturating_add(1)
    };

    // Build context
    let ctx_result = CliContext::builder()
        .output_format(cli.format.into())
        .verbosity(verbosity)
        .colors(!cli.no_color && console::colors_enabled())
        .build();

    let ctx = match ctx_result {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to initialize CLI: {}", e);
            return ExitCode::from(1);
        }
    };

    // Initialize logging if verbose
    if verbosity >= 2 {
        let log_level = match verbosity {
            2 => LogLevel::Info,
            3 => LogLevel::Debug,
            _ => LogLevel::Trace,
        };
        let mut log_config = LogConfig::development();
        log_config.level = log_level;
        if let Err(e) = init_logging(&log_config) {
            eprintln!("Warning: Failed to initialize logging: {}", e);
        }
    }

    // Create runner with hooks
    let mut runner = CommandRunner::new(ctx);
    runner.add_hook(LoggingHook);
    runner.add_hook(MetricsHook::new());

    // Execute command
    let result = match cli.command {
        Commands::Run(args) => {
            let mut cmd = RunCommand::new(args.scenario)
                .with_time_scale(args.time_scale)
                .with_dry_run(args.dry_run);

            if let Some(duration_str) = args.duration {
                match parse_duration(&duration_str) {
                    Ok(d) => cmd = cmd.with_duration(d),
                    Err(e) => {
                        eprintln!("Invalid duration: {}", e);
                        return ExitCode::from(2);
                    }
                }
            }

            runner.run_with_shutdown(&cmd).await
        }

        Commands::Modbus(args) => {
            let bind_addr = format!("{}:{}", args.bind, args.port);
            let addr = match bind_addr.parse() {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("Invalid bind address: {}", e);
                    return ExitCode::from(2);
                }
            };

            let tags = tags_from_entries(&args.tags);

            let mut cmd = ModbusCommand::new()
                .with_bind_addr(addr)
                .with_devices(args.devices)
                .with_points(args.points)
                .with_tags(tags);

            if args.rtu {
                if let Some(serial) = args.serial {
                    cmd = cmd.with_rtu_mode(serial);
                }
            }

            runner.run_with_shutdown(&cmd).await
        }

        Commands::Opcua(args) => {
            let tags = tags_from_entries(&args.tags);

            let cmd = OpcuaCommand::new()
                .with_port(args.port)
                .with_endpoint(args.endpoint)
                .with_nodes(args.nodes)
                .with_security(args.security.to_string())
                .with_tags(tags);

            runner.run_with_shutdown(&cmd).await
        }

        Commands::Bacnet(args) => {
            let tags = tags_from_entries(&args.tags);

            let cmd = BacnetCommand::new()
                .with_port(args.port)
                .with_device_instance(args.instance)
                .with_objects(args.objects)
                .with_bbmd(args.bbmd)
                .with_tags(tags);

            runner.run_with_shutdown(&cmd).await
        }

        Commands::Knx(args) => {
            let tags = tags_from_entries(&args.tags);

            let cmd = KnxCommand::new()
                .with_port(args.port)
                .with_individual_address(args.address)
                .with_group_objects(args.groups)
                .with_tags(tags);

            runner.run_with_shutdown(&cmd).await
        }

        Commands::Validate(args) => {
            let cmd = ValidateCommand::new(args.files)
                .with_detailed(args.detailed)
                .with_strict(args.strict);

            runner.run(&cmd).await
        }

        Commands::List(args) => {
            let resource = match args.resource {
                ListResourceArg::Devices => mabi_cli::commands::list::ListResource::Devices,
                ListResourceArg::Protocols => mabi_cli::commands::list::ListResource::Protocols,
                ListResourceArg::Points => mabi_cli::commands::list::ListResource::Points,
                ListResourceArg::Scenarios => mabi_cli::commands::list::ListResource::Scenarios,
            };

            let mut cmd = ListCommand::new(resource);

            if let Some(proto) = args.protocol {
                match proto.to_lowercase().as_str() {
                    "modbus" | "modbus_tcp" => cmd = cmd.with_protocol(Protocol::ModbusTcp),
                    "modbus_rtu" => cmd = cmd.with_protocol(Protocol::ModbusRtu),
                    "opcua" => cmd = cmd.with_protocol(Protocol::OpcUa),
                    "bacnet" => cmd = cmd.with_protocol(Protocol::BacnetIp),
                    "knx" => cmd = cmd.with_protocol(Protocol::KnxIp),
                    _ => {
                        eprintln!("Unknown protocol: {}", proto);
                        return ExitCode::from(3);
                    }
                }
            }

            if let Some(filter) = args.filter {
                cmd = cmd.with_device_filter(filter);
            }

            if let Some(limit) = args.limit {
                cmd = cmd.with_limit(limit);
            }

            runner.run(&cmd).await
        }

        Commands::Version => {
            println!("mabi {} (Mabinogion)", env!("CARGO_PKG_VERSION"));
            println!("Rust {}", rustc_version());
            println!();
            println!("Supported protocols:");
            println!("  - Modbus TCP/RTU");
            println!("  - OPC UA");
            println!("  - BACnet/IP");
            println!("  - KNXnet/IP");
            return ExitCode::SUCCESS;
        }
    };

    // Handle result
    match result {
        Ok(output) => {
            if let Some(msg) = output.message {
                if output.exit_code == 0 {
                    println!("{}", msg);
                } else {
                    eprintln!("{}", msg);
                }
            }
            ExitCode::from(output.exit_code as u8)
        }
        Err(e) => {
            // Special handling for interrupts
            if matches!(e, CliError::Interrupted) {
                return ExitCode::from(130);
            }
            eprintln!("Error: {}", e);
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

/// Parse a duration string like "10s", "5m", "1h".
fn parse_duration(s: &str) -> std::result::Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

/// Get the Rust compiler version.
fn rustc_version() -> &'static str {
    env!("CARGO_PKG_RUST_VERSION")
}
