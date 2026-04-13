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
use mabi_modbus::{
    BehaviorSetPort, CompiledModbusSession, FaultPresetPort, ModbusConfigSummary,
    ModbusControlSession, ModbusSimulatorConfig, PointCatalogPort, PointCatalogQuery, PointTarget,
    RegisterControlPort, SessionControlPort as ModbusSessionControlPort, TracePort,
};
use mabi_opcua::{
    compile_session_with_report as compile_opcua_session_with_report,
    generate_types_with_report as generate_opcua_types_with_report,
    modeling::OpcUaCompiledLaunchConfig, CompiledOpcUaSession, NodeCatalogPort, NodeTarget,
    NodeValueControlPort, OpcUaConfigSummary, OpcUaControlSession, OpcUaSimulatorConfig,
    SecurityControlPort, SessionControlPort as OpcuaSessionControlPort,
};
use mabi_runtime::{ProtocolLaunchSpec, RuntimeSession};
use mabi_scenario::prelude::ScenarioValidator;
use mabi_scenario::{Scenario, ScenarioParser};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const OPCUA_LEGACY_COMPAT_MESSAGE: &str =
    "legacy OPC UA serve flow has been removed; use --config <file> --session <name>";

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

impl From<ModbusRegisterTypeArg> for mabi_core::types::ModbusRegisterType {
    fn from(arg: ModbusRegisterTypeArg) -> Self {
        match arg {
            ModbusRegisterTypeArg::Coil => Self::Coil,
            ModbusRegisterTypeArg::DiscreteInput => Self::DiscreteInput,
            ModbusRegisterTypeArg::HoldingRegister => Self::HoldingRegister,
            ModbusRegisterTypeArg::InputRegister => Self::InputRegister,
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

    /// Runtime control commands for simulator sessions
    Control(ControlCommandArgs),

    /// Deterministic code generation from canonical simulator configs
    Generate(GenerateCommandArgs),

    /// Show version information
    Version,
}

#[derive(Args)]
struct GenerateCommandArgs {
    #[command(subcommand)]
    target: GenerateTargetCommand,
}

#[derive(Subcommand)]
enum GenerateTargetCommand {
    #[command(name = "opcua-types")]
    OpcuaTypes(GenerateOpcuaTypesArgs),
}

#[derive(Args, Clone)]
struct GenerateOpcuaTypesArgs {
    #[arg(long)]
    config: PathBuf,

    #[arg(long)]
    session: String,

    #[arg(long)]
    out: PathBuf,
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

    /// Named session from a Modbus simulator config file
    #[arg(long)]
    session: Option<String>,

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

    /// Named session from an OPC UA simulator config file
    #[arg(long)]
    session: Option<String>,

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

    /// Show supported generic schema surfaces
    Schema(InspectSchemaArgs),

    /// Show the typed Modbus simulator schema
    #[command(name = "modbus-schema")]
    ModbusSchema,

    /// Inspect a Modbus simulator config file
    #[command(name = "modbus-config")]
    ModbusConfig(InspectModbusConfigArgs),

    /// Show the typed OPC UA simulator schema
    #[command(name = "opcua-schema")]
    OpcuaSchema,

    /// Inspect an OPC UA simulator config file
    #[command(name = "opcua-config")]
    OpcuaConfig(InspectOpcuaConfigArgs),

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

    /// Validate a typed Modbus simulator config file
    #[command(name = "modbus-config")]
    ModbusConfig(ValidateModbusConfigArgs),

    /// Validate a typed OPC UA simulator config file
    #[command(name = "opcua-config")]
    OpcuaConfig(ValidateOpcuaConfigArgs),
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

#[derive(Args)]
struct ValidateModbusConfigArgs {
    /// Modbus config file to validate
    #[arg(required = true)]
    file: PathBuf,
}

#[derive(Args)]
struct InspectModbusConfigArgs {
    /// Modbus config file to inspect
    #[arg(required = true)]
    file: PathBuf,
}

#[derive(Args)]
struct InspectOpcuaConfigArgs {
    /// OPC UA config file to inspect
    #[arg(required = true)]
    file: PathBuf,
}

#[derive(Args)]
struct ValidateOpcuaConfigArgs {
    /// OPC UA config file to validate
    #[arg(required = true)]
    file: PathBuf,
}

#[derive(Args)]
struct ControlCommandArgs {
    #[command(subcommand)]
    protocol: ControlProtocolCommand,
}

#[derive(Subcommand)]
enum ControlProtocolCommand {
    Modbus(ModbusControlArgs),
    Opcua(OpcuaControlArgs),
}

#[derive(Args)]
struct ModbusControlArgs {
    /// Named session from the shared Modbus simulator config file
    #[arg(long)]
    session: String,

    #[command(subcommand)]
    command: ModbusControlSubcommand,
}

#[derive(Subcommand)]
enum ModbusControlSubcommand {
    Session(ModbusSessionCommandArgs),
    Point(ModbusPointCommandArgs),
    Trace(ModbusTraceCommandArgs),
    Faults(ModbusFaultCommandArgs),
    Behavior(ModbusBehaviorCommandArgs),
}

#[derive(Args)]
struct ModbusSessionCommandArgs {
    #[command(subcommand)]
    command: ModbusSessionSubcommand,
}

#[derive(Subcommand)]
enum ModbusSessionSubcommand {
    Status,
    Reset,
    Snapshot,
}

#[derive(Args)]
struct ModbusPointCommandArgs {
    #[command(subcommand)]
    command: ModbusPointSubcommand,
}

#[derive(Subcommand)]
enum ModbusPointSubcommand {
    List(ModbusPointListArgs),
    Read(ModbusPointReadArgs),
    Write(ModbusPointWriteArgs),
}

#[derive(Args, Default)]
struct ModbusPointSelectorArgs {
    /// Explicit device id (defaults to inferred modbus-<unit> when --unit is supplied)
    #[arg(long)]
    device: Option<String>,

    /// Stable point id
    #[arg(long)]
    point: Option<String>,

    /// Modbus unit id for address-based selection
    #[arg(long)]
    unit: Option<u8>,

    /// Register family for address-based selection
    #[arg(long, value_enum)]
    register_type: Option<ModbusRegisterTypeArg>,

    /// 0-based Modbus address for address-based selection
    #[arg(long)]
    address: Option<u16>,
}

#[derive(Args)]
struct ModbusPointListArgs {
    /// Restrict output to a single device id
    #[arg(long)]
    device: Option<String>,

    /// Filter by key=value tag pairs
    #[arg(long = "tag")]
    tags: Vec<String>,

    /// Filter by label tags
    #[arg(long = "label")]
    labels: Vec<String>,
}

#[derive(Args)]
struct ModbusPointReadArgs {
    #[command(flatten)]
    selector: ModbusPointSelectorArgs,
}

#[derive(Args)]
struct ModbusPointWriteArgs {
    #[command(flatten)]
    selector: ModbusPointSelectorArgs,

    /// JSON literal or raw string value to write
    value: String,
}

#[derive(Args)]
struct ModbusTraceCommandArgs {
    #[command(subcommand)]
    command: ModbusTraceSubcommand,
}

#[derive(Subcommand)]
enum ModbusTraceSubcommand {
    Tail(ModbusTraceTailArgs),
    Clear,
}

#[derive(Args)]
struct ModbusTraceTailArgs {
    #[arg(long, default_value = "50")]
    limit: usize,
}

#[derive(Args)]
struct ModbusFaultCommandArgs {
    #[command(subcommand)]
    command: ModbusFaultSubcommand,
}

#[derive(Subcommand)]
enum ModbusFaultSubcommand {
    Apply(ModbusFaultApplyArgs),
    Clear,
}

#[derive(Args)]
struct ModbusFaultApplyArgs {
    /// Named fault preset from the selected session
    preset: String,
}

#[derive(Args)]
struct ModbusBehaviorCommandArgs {
    #[command(subcommand)]
    command: ModbusBehaviorSubcommand,
}

#[derive(Subcommand)]
enum ModbusBehaviorSubcommand {
    List,
    Apply(ModbusBehaviorApplyArgs),
    Clear,
}

#[derive(Args)]
struct ModbusBehaviorApplyArgs {
    /// Named behavior set from the selected session
    behavior_set: String,
}

#[derive(Args)]
struct OpcuaControlArgs {
    /// Named session from the shared OPC UA simulator config file
    #[arg(long)]
    session: String,

    #[command(subcommand)]
    command: OpcuaControlSubcommand,
}

#[derive(Subcommand)]
enum OpcuaControlSubcommand {
    Session(OpcuaSessionCommandArgs),
    Node(OpcuaNodeCommandArgs),
    Security(OpcuaSecurityCommandArgs),
}

#[derive(Args)]
struct OpcuaSessionCommandArgs {
    #[command(subcommand)]
    command: OpcuaSessionSubcommand,
}

#[derive(Subcommand)]
enum OpcuaSessionSubcommand {
    Status,
    Reset,
    Snapshot,
}

#[derive(Args)]
struct OpcuaNodeCommandArgs {
    #[command(subcommand)]
    command: OpcuaNodeSubcommand,
}

#[derive(Subcommand)]
enum OpcuaNodeSubcommand {
    List(OpcuaNodeListArgs),
    Read(OpcuaNodeReadArgs),
    Write(OpcuaNodeWriteArgs),
}

#[derive(Args)]
struct OpcuaSecurityCommandArgs {
    #[command(subcommand)]
    command: OpcuaSecuritySubcommand,
}

#[derive(Subcommand)]
enum OpcuaSecuritySubcommand {
    Status,
    TrustReload,
    Rotate(OpcuaSecurityRotateArgs),
    AuditSummary,
}

#[derive(Args)]
struct OpcuaSecurityRotateArgs {
    #[arg(long)]
    certificate: PathBuf,
    #[arg(long = "private-key")]
    private_key: PathBuf,
}

#[derive(Args, Default)]
struct OpcuaNodeSelectorArgs {
    /// Explicit device id
    #[arg(long)]
    device: Option<String>,

    /// Stable point id from the compiled session
    #[arg(long)]
    point: Option<String>,

    /// Raw OPC UA node id (for example ns=2;s=Temperature)
    #[arg(long = "node-id")]
    node_id: Option<String>,
}

#[derive(Args)]
struct OpcuaNodeListArgs {
    /// Restrict output to a single device id
    #[arg(long)]
    device: Option<String>,
}

#[derive(Args)]
struct OpcuaNodeReadArgs {
    #[command(flatten)]
    selector: OpcuaNodeSelectorArgs,
}

#[derive(Args)]
struct OpcuaNodeWriteArgs {
    #[command(flatten)]
    selector: OpcuaNodeSelectorArgs,

    /// JSON literal or raw string value to write
    value: String,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ModbusRegisterTypeArg {
    Coil,
    DiscreteInput,
    HoldingRegister,
    InputRegister,
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
    let global_config = cli.global.config.clone();

    let result = match cli.command {
        Commands::Serve(args) => {
            let serve_plan = {
                let ctx_handle = runner.context();
                let ctx = ctx_handle.read().await;
                into_launch_spec(&ctx, global_config.as_deref(), args.protocol).await
            };
            match serve_plan {
                Ok((launch, extensions)) => {
                    let cmd = ServeRuntimeCommand::new(launch, readiness_timeout, extensions);
                    runner.run_with_shutdown(&cmd).await
                }
                Err(error) => Err(error),
            }
        }
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
                InspectSubcommand::ModbusSchema => inspect_modbus_schema(&mut ctx).await,
                InspectSubcommand::ModbusConfig(args) => {
                    inspect_modbus_config(&mut ctx, args.file).await
                }
                InspectSubcommand::OpcuaSchema => inspect_opcua_schema(&mut ctx).await,
                InspectSubcommand::OpcuaConfig(args) => {
                    inspect_opcua_config(&mut ctx, args.file).await
                }
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
            ValidateSubcommand::ModbusConfig(args) => {
                let ctx_handle = runner.context();
                let mut ctx = ctx_handle.write().await;
                validate_modbus_config(&mut ctx, args.file).await
            }
            ValidateSubcommand::OpcuaConfig(args) => {
                let ctx_handle = runner.context();
                let mut ctx = ctx_handle.write().await;
                validate_opcua_config(&mut ctx, args.file).await
            }
        },
        Commands::Control(args) => {
            let ctx_handle = runner.context();
            let mut ctx = ctx_handle.write().await;
            match args.protocol {
                ControlProtocolCommand::Modbus(args) => {
                    control_modbus(&mut ctx, global_config.as_deref(), args, readiness_timeout)
                        .await
                }
                ControlProtocolCommand::Opcua(args) => {
                    control_opcua(&mut ctx, global_config.as_deref(), args, readiness_timeout).await
                }
            }
        }
        Commands::Generate(args) => {
            let ctx_handle = runner.context();
            let mut ctx = ctx_handle.write().await;
            match args.target {
                GenerateTargetCommand::OpcuaTypes(args) => {
                    generate_opcua_types_command(&mut ctx, args).await
                }
            }
        }
        Commands::Version => {
            println!("mabi {} (Mabinogion)", mabi_core::RELEASE_VERSION);
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

async fn into_launch_spec(
    ctx: &CliContext,
    global_config: Option<&Path>,
    command: ServeProtocolCommand,
) -> CliResult<(ProtocolLaunchSpec, mabi_runtime::RuntimeExtensions)> {
    match command {
        ServeProtocolCommand::Modbus(args) => {
            if let Some(path) = global_config {
                let session_name =
                    args.session
                        .as_deref()
                        .ok_or_else(|| CliError::InvalidConfig {
                            message: "mabi serve modbus --config <file> requires --session <name>"
                                .into(),
                        })?;
                let (_, compiled) = load_compiled_modbus_session(ctx, path, session_name)?;
                let mut launch = compiled.launch.clone();
                if args.serve.name.is_some() {
                    launch.name = args.serve.name;
                }
                Ok((launch, compiled.runtime_extensions()))
            } else {
                Ok((
                    ProtocolLaunchSpec {
                        protocol: "modbus".into(),
                        name: args.serve.name,
                        config: json!({
                            "transport": {
                                "kind": "tcp",
                                "bind_addr": parse_bind_addr(&args.bind, args.port)?,
                                "performance_preset": "default",
                            },
                            "devices": args.devices,
                            "points_per_device": args.points,
                        }),
                    },
                    mabi_runtime::RuntimeExtensions::default(),
                ))
            }
        }
        ServeProtocolCommand::Opcua(args) => {
            if let Some(path) = global_config {
                let session_name =
                    args.session
                        .as_deref()
                        .ok_or_else(|| CliError::InvalidConfig {
                            message: "mabi serve opcua --config <file> requires --session <name>"
                                .into(),
                        })?;
                let (_, compiled) = load_compiled_opcua_session(ctx, path, session_name)?;
                let mut launch = compiled.launch.clone();
                if args.serve.name.is_some() {
                    launch.name = args.serve.name;
                }
                Ok((launch, compiled.runtime_extensions()))
            } else {
                let compiled = compile_legacy_opcua_session(&args, args.serve.name.clone())?;
                Ok((compiled.launch.clone(), compiled.runtime_extensions()))
            }
        }
        ServeProtocolCommand::Bacnet(args) => Ok((
            ProtocolLaunchSpec {
                protocol: "bacnet".into(),
                name: args.serve.name,
                config: json!({
                    "bind_addr": parse_bind_addr(&args.bind, args.port)?,
                    "device_instance": args.instance,
                    "objects": args.objects,
                    "bbmd_enabled": args.bbmd,
                }),
            },
            mabi_runtime::RuntimeExtensions::default(),
        )),
        ServeProtocolCommand::Knx(args) => Ok((
            ProtocolLaunchSpec {
                protocol: "knx".into(),
                name: args.serve.name,
                config: json!({
                    "bind_addr": parse_bind_addr(&args.bind, args.port)?,
                    "individual_address": args.address,
                    "group_objects": args.groups,
                }),
            },
            mabi_runtime::RuntimeExtensions::default(),
        )),
    }
}

fn parse_bind_addr(bind: &str, port: u16) -> CliResult<std::net::SocketAddr> {
    format!("{}:{}", bind, port)
        .parse()
        .map_err(|error| CliError::InvalidConfig {
            message: format!("invalid bind address {}:{} ({})", bind, port, error),
        })
}

fn load_modbus_config(
    ctx: &CliContext,
    path: &Path,
) -> CliResult<(PathBuf, ModbusSimulatorConfig)> {
    let resolved = ctx.resolve_path(path);
    if !resolved.exists() {
        return Err(CliError::ConfigNotFound { path: resolved });
    }

    let config =
        ModbusSimulatorConfig::from_path(&resolved).map_err(|error| CliError::InvalidConfig {
            message: error.to_string(),
        })?;
    Ok((resolved, config))
}

fn load_compiled_modbus_session(
    ctx: &CliContext,
    path: &Path,
    session_name: &str,
) -> CliResult<(PathBuf, CompiledModbusSession)> {
    let (resolved, config) = load_modbus_config(ctx, path)?;
    let compiled =
        config
            .compile_session(session_name)
            .map_err(|error| CliError::InvalidConfig {
                message: error.to_string(),
            })?;
    Ok((resolved, compiled))
}

fn load_opcua_config(ctx: &CliContext, path: &Path) -> CliResult<(PathBuf, OpcUaSimulatorConfig)> {
    let resolved = ctx.resolve_path(path);
    if !resolved.exists() {
        return Err(CliError::ConfigNotFound { path: resolved });
    }

    let config =
        OpcUaSimulatorConfig::from_path(&resolved).map_err(|error| CliError::InvalidConfig {
            message: error.to_string(),
        })?;
    Ok((resolved, config))
}

fn load_compiled_opcua_session(
    ctx: &CliContext,
    path: &Path,
    session_name: &str,
) -> CliResult<(PathBuf, CompiledOpcUaSession)> {
    let (resolved, config) = load_opcua_config(ctx, path)?;
    let compiled = config
        .compile_session(session_name, Some(&resolved))
        .map_err(|error| CliError::InvalidConfig {
            message: error.to_string(),
        })?;
    Ok((resolved, compiled))
}

fn compile_legacy_opcua_session(
    _args: &OpcuaServeArgs,
    _service_name: Option<String>,
) -> CliResult<CompiledOpcUaSession> {
    Err(CliError::InvalidConfig {
        message: OPCUA_LEGACY_COMPAT_MESSAGE.into(),
    })
}

fn parse_tag_filters(values: &[String]) -> CliResult<Vec<(String, String)>> {
    values
        .iter()
        .map(|value| {
            let (key, tag_value) =
                value
                    .split_once('=')
                    .ok_or_else(|| CliError::InvalidConfig {
                        message: format!("invalid tag filter '{}', expected key=value", value),
                    })?;
            Ok((key.to_string(), tag_value.to_string()))
        })
        .collect()
}

fn point_target_from_selector(selector: &ModbusPointSelectorArgs) -> CliResult<PointTarget> {
    if selector.point.is_none()
        && (selector.unit.is_none()
            || selector.register_type.is_none()
            || selector.address.is_none())
    {
        return Err(CliError::InvalidConfig {
            message:
                "point selection requires either --point, or --unit + --register-type + --address"
                    .into(),
        });
    }

    Ok(PointTarget {
        device_id: selector.device.clone(),
        point_id: selector.point.clone(),
        unit_id: selector.unit,
        register_type: selector.register_type.map(Into::into),
        address: selector.address,
    })
}

fn parse_modbus_value(raw: &str) -> CliResult<mabi_core::Value> {
    if let Ok(value) = serde_json::from_str::<mabi_core::Value>(raw) {
        return Ok(value);
    }

    if raw.eq_ignore_ascii_case("true") || raw.eq_ignore_ascii_case("false") {
        return serde_json::from_str::<mabi_core::Value>(&raw.to_ascii_lowercase())
            .map_err(Into::into);
    }

    Ok(mabi_core::Value::String(raw.to_string()))
}

fn node_target_from_selector(selector: &OpcuaNodeSelectorArgs) -> CliResult<NodeTarget> {
    if selector.point.is_none() && selector.node_id.is_none() {
        return Err(CliError::InvalidConfig {
            message: "node selection requires either --point or --node-id".into(),
        });
    }

    Ok(NodeTarget {
        device_id: selector.device.clone(),
        point_id: selector.point.clone(),
        node_id: selector.node_id.clone(),
    })
}

fn parse_opcua_value(raw: &str) -> CliResult<mabi_core::Value> {
    parse_modbus_value(raw)
}

async fn inspect_modbus_schema(ctx: &mut CliContext) -> CliResult<CommandOutput> {
    let schema = mabi_modbus::schema_summary();

    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&schema)?;
        return Ok(CommandOutput::quiet_success());
    }

    ctx.output().header("Modbus Schema");
    ctx.output().kv("Kind", schema.kind);
    ctx.output().kv("Formats", schema.formats.join(", "));
    for section in &schema.top_level_sections {
        ctx.output().kv(
            format!("Section {}", section.name),
            format!(
                "{} [{}]",
                section.purpose,
                if section.required {
                    "required"
                } else {
                    "optional"
                }
            ),
        );
    }
    ctx.output().kv("Commands", schema.commands.join(" | "));
    ctx.output().kv("Notes", schema.notes.join("; "));
    Ok(CommandOutput::quiet_success())
}

async fn inspect_modbus_config(ctx: &mut CliContext, file: PathBuf) -> CliResult<CommandOutput> {
    #[derive(Serialize)]
    struct CompiledSessionView {
        name: String,
        service_name: Option<String>,
        transport: String,
        units: usize,
        points: usize,
        trace_enabled: bool,
        active_fault_preset: Option<String>,
        active_behavior_set: Option<String>,
    }

    #[derive(Serialize)]
    struct ModbusConfigInspectView {
        path: String,
        summary: ModbusConfigSummary,
        sessions: Vec<CompiledSessionView>,
    }

    let (resolved, config) = load_modbus_config(ctx, &file)?;
    let mut sessions = Vec::new();
    for name in config.sessions.keys() {
        let compiled = config
            .compile_session(name)
            .map_err(|error| CliError::InvalidConfig {
                message: error.to_string(),
            })?;
        sessions.push(CompiledSessionView {
            name: name.clone(),
            service_name: compiled.launch.name.clone(),
            transport: compiled.launch.config["transport"]["kind"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            units: compiled.profile.units.len(),
            points: compiled
                .profile
                .units
                .iter()
                .map(|unit| unit.points.len())
                .sum(),
            trace_enabled: compiled.trace.enabled,
            active_fault_preset: compiled.active_fault_preset.clone(),
            active_behavior_set: compiled.active_behavior_set.clone(),
        });
    }

    let view = ModbusConfigInspectView {
        path: resolved.display().to_string(),
        summary: config.inspect_summary(),
        sessions,
    };

    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&view)?;
        return Ok(CommandOutput::quiet_success());
    }

    ctx.output().header("Modbus Config");
    ctx.output().kv("Path", &view.path);
    ctx.output()
        .kv("Transports", view.summary.transports.join(", "));
    ctx.output()
        .kv("Datastores", view.summary.datastores.join(", "));
    ctx.output().kv("Devices", view.summary.devices.join(", "));
    ctx.output().kv("Presets", view.summary.presets.join(", "));
    for session in &view.sessions {
        ctx.output().kv(
            format!("Session {}", session.name),
            format!(
                "service={}, transport={}, units={}, points={}, trace={}, fault={}, behavior={}",
                session.service_name.as_deref().unwrap_or(&session.name),
                session.transport,
                session.units,
                session.points,
                session.trace_enabled,
                session.active_fault_preset.as_deref().unwrap_or("none"),
                session.active_behavior_set.as_deref().unwrap_or("none")
            ),
        );
    }
    Ok(CommandOutput::quiet_success())
}

async fn validate_modbus_config(ctx: &mut CliContext, file: PathBuf) -> CliResult<CommandOutput> {
    #[derive(Serialize)]
    struct ModbusValidationView {
        path: String,
        transports: usize,
        devices: usize,
        sessions: usize,
        presets: usize,
        behaviors: usize,
    }

    let (resolved, config) = load_modbus_config(ctx, &file)?;
    let view = ModbusValidationView {
        path: resolved.display().to_string(),
        transports: config.transports.len(),
        devices: config.devices.len(),
        sessions: config.sessions.len(),
        presets: config.presets.len(),
        behaviors: config.behaviors.len(),
    };

    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&view)?;
    } else {
        ctx.output().header("Modbus Config Validation");
        ctx.output().kv("Path", &view.path);
        ctx.output().kv("Transports", view.transports);
        ctx.output().kv("Devices", view.devices);
        ctx.output().kv("Sessions", view.sessions);
        ctx.output().kv("Presets", view.presets);
        ctx.output().kv("Behaviors", view.behaviors);
    }

    if !ctx.is_quiet() {
        ctx.output().success("Modbus config validation passed");
    }
    Ok(CommandOutput::quiet_success())
}

async fn inspect_opcua_schema(ctx: &mut CliContext) -> CliResult<CommandOutput> {
    let schema = mabi_opcua::schema_summary();

    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&schema)?;
        return Ok(CommandOutput::quiet_success());
    }

    ctx.output().header("OPC UA Schema");
    ctx.output().kv("Kind", schema.kind);
    ctx.output().kv("Formats", schema.formats.join(", "));
    for section in &schema.top_level_sections {
        ctx.output().kv(
            format!("Section {}", section.name),
            format!(
                "{} [{}]",
                section.purpose,
                if section.required {
                    "required"
                } else {
                    "optional"
                }
            ),
        );
    }
    ctx.output().kv("Commands", schema.commands.join(" | "));
    ctx.output().kv("Notes", schema.notes.join("; "));
    Ok(CommandOutput::quiet_success())
}

async fn inspect_opcua_config(ctx: &mut CliContext, file: PathBuf) -> CliResult<CommandOutput> {
    #[derive(Serialize)]
    struct CompiledSessionView {
        name: String,
        service_name: Option<String>,
        protocol: String,
        connection_mode: String,
        reverse_connect_target: Option<String>,
        endpoint: String,
        nodes: usize,
        devices: usize,
        namespaces: usize,
        points: usize,
        cache: String,
    }

    #[derive(Serialize)]
    struct OpcuaConfigInspectView {
        path: String,
        summary: OpcUaConfigSummary,
        sessions: Vec<CompiledSessionView>,
    }

    let (resolved, config) = load_opcua_config(ctx, &file)?;
    let mut sessions = Vec::new();
    for name in config.sessions.keys() {
        let (compiled, cache_report) =
            compile_opcua_session_with_report(&config, name, Some(&resolved)).map_err(|error| {
                CliError::InvalidConfig {
                    message: error.to_string(),
                }
            })?;
        let endpoint = compiled.launch.config["server_config"]["endpoint_url"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let launch: OpcUaCompiledLaunchConfig =
            serde_json::from_value(compiled.launch.config.clone()).map_err(|error| {
                CliError::ExecutionFailed {
                    message: format!(
                    "failed to decode compiled OPC UA launch config for inspect output: {error}"
                    ),
                }
            })?;
        sessions.push(CompiledSessionView {
            name: name.clone(),
            service_name: compiled.launch.name.clone(),
            protocol: launch.server_config.endpoint_protocol.scheme().to_string(),
            connection_mode: launch.server_config.connection_mode.as_str().to_string(),
            reverse_connect_target: launch.server_config.reverse_connect_target.clone(),
            endpoint,
            nodes: compiled.catalog.nodes.len(),
            devices: compiled.devices.len(),
            namespaces: compiled.catalog.namespace_table.len(),
            points: compiled
                .devices
                .iter()
                .map(|device| device.points.len())
                .sum(),
            cache: format!(
                "{} (imports {}/{})",
                if cache_report.compilation_hit {
                    "hit"
                } else {
                    "miss"
                },
                cache_report.import_hits,
                cache_report.import_misses
            ),
        });
    }

    let view = OpcuaConfigInspectView {
        path: resolved.display().to_string(),
        summary: config.inspect_summary(),
        sessions,
    };

    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&view)?;
        return Ok(CommandOutput::quiet_success());
    }

    ctx.output().header("OPC UA Config");
    ctx.output().kv("Path", &view.path);
    ctx.output()
        .kv("Transports", view.summary.transports.join(", "));
    ctx.output().kv(
        "Security Profiles",
        view.summary.security_profiles.join(", "),
    );
    ctx.output()
        .kv("NodeSets", view.summary.nodesets.join(", "));
    ctx.output()
        .kv("Companion Packs", view.summary.companion_packs.join(", "));
    ctx.output().kv("Models", view.summary.models.join(", "));
    ctx.output().kv("Devices", view.summary.devices.join(", "));
    ctx.output().kv("Presets", view.summary.presets.join(", "));
    ctx.output()
        .kv("Generated Types", view.summary.generated_types_enabled);
    for session in &view.sessions {
        let endpoint_summary = format!(
            "{}{}",
            session.endpoint,
            session
                .reverse_connect_target
                .as_ref()
                .map(|target| format!(", reverse_target={target}"))
                .unwrap_or_default()
        );
        ctx.output().kv(
            format!("Session {}", session.name),
            format!(
                "service={}, protocol={}, mode={}, endpoint={}, nodes={}, devices={}, points={}, namespaces={}, cache={}",
                session.service_name.as_deref().unwrap_or(&session.name),
                session.protocol,
                session.connection_mode,
                endpoint_summary,
                session.nodes,
                session.devices,
                session.points,
                session.namespaces,
                session.cache
            ),
        );
    }
    Ok(CommandOutput::quiet_success())
}

async fn validate_opcua_config(ctx: &mut CliContext, file: PathBuf) -> CliResult<CommandOutput> {
    #[derive(Serialize)]
    struct OpcuaValidationView {
        path: String,
        transports: usize,
        transport_protocols: Vec<String>,
        transport_connection_modes: Vec<String>,
        reverse_connect_transports: usize,
        security_profiles: usize,
        nodesets: usize,
        companion_packs: usize,
        models: usize,
        devices: usize,
        sessions: usize,
        presets: usize,
        generated_types_enabled: bool,
        cache_hits: usize,
        cache_misses: usize,
    }

    let (resolved, config) = load_opcua_config(ctx, &file)?;
    let mut cache_hits = 0usize;
    let mut cache_misses = 0usize;
    for name in config.sessions.keys() {
        let (_, report) = compile_opcua_session_with_report(&config, name, Some(&resolved))
            .map_err(|error| CliError::InvalidConfig {
                message: error.to_string(),
            })?;
        if report.compilation_hit {
            cache_hits += 1;
        } else {
            cache_misses += 1;
        }
    }
    let view = OpcuaValidationView {
        path: resolved.display().to_string(),
        transports: config.transports.len(),
        transport_protocols: {
            let mut protocols = config
                .transports
                .values()
                .map(|transport| transport.protocol.scheme().to_string())
                .collect::<Vec<_>>();
            protocols.sort();
            protocols.dedup();
            protocols
        },
        transport_connection_modes: {
            let mut modes = config
                .transports
                .values()
                .map(|transport| transport.connection_mode.as_str().to_string())
                .collect::<Vec<_>>();
            modes.sort();
            modes.dedup();
            modes
        },
        reverse_connect_transports: config
            .transports
            .values()
            .filter(|transport| {
                transport.connection_mode == mabi_opcua::TransportConnectionMode::ReverseConnect
            })
            .count(),
        security_profiles: config.security_profiles.len(),
        nodesets: config.nodesets.len(),
        companion_packs: config.companion_packs.len(),
        models: config.models.len(),
        devices: config.devices.len(),
        sessions: config.sessions.len(),
        presets: config.presets.len(),
        generated_types_enabled: config.generated_types.enabled,
        cache_hits,
        cache_misses,
    };

    if matches!(
        ctx.output().format(),
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    ) {
        ctx.output().write(&view)?;
    } else {
        ctx.output().header("OPC UA Config Validation");
        ctx.output().kv("Path", &view.path);
        ctx.output().kv("Transports", view.transports);
        ctx.output()
            .kv("Transport Protocols", view.transport_protocols.join(", "));
        ctx.output().kv(
            "Transport Connection Modes",
            view.transport_connection_modes.join(", "),
        );
        ctx.output().kv(
            "Reverse Connect Transports",
            view.reverse_connect_transports,
        );
        ctx.output().kv("Security Profiles", view.security_profiles);
        ctx.output().kv("NodeSets", view.nodesets);
        ctx.output().kv("Companion Packs", view.companion_packs);
        ctx.output().kv("Models", view.models);
        ctx.output().kv("Devices", view.devices);
        ctx.output().kv("Sessions", view.sessions);
        ctx.output().kv("Presets", view.presets);
        ctx.output()
            .kv("Generated Types", view.generated_types_enabled);
        ctx.output().kv("Compilation Cache Hits", view.cache_hits);
        ctx.output()
            .kv("Compilation Cache Misses", view.cache_misses);
    }

    if !ctx.is_quiet() {
        ctx.output().success("OPC UA config validation passed");
    }
    Ok(CommandOutput::quiet_success())
}

async fn generate_opcua_types_command(
    ctx: &mut CliContext,
    args: GenerateOpcuaTypesArgs,
) -> CliResult<CommandOutput> {
    #[derive(Serialize)]
    struct GenerateView {
        config_path: String,
        session: String,
        output_dir: String,
        module_name: String,
        source_path: String,
        manifest_path: String,
        cache: String,
    }

    let (resolved, config) = load_opcua_config(ctx, &args.config)?;
    let (generated, cache_report) =
        generate_opcua_types_with_report(&config, &args.session, Some(&resolved)).map_err(
            |error: mabi_opcua::OpcUaError| CliError::InvalidConfig {
                message: error.to_string(),
            },
        )?;
    let output_dir = ctx.resolve_path(&args.out);
    std::fs::create_dir_all(&output_dir).map_err(|error| CliError::ExecutionFailed {
        message: format!(
            "failed to create output directory '{}': {}",
            output_dir.display(),
            error
        ),
    })?;

    let source_path = output_dir.join(format!("{}.rs", generated.module_name));
    let manifest_path = output_dir.join(format!("{}.manifest.json", generated.module_name));
    std::fs::write(&source_path, &generated.source).map_err(|error| CliError::ExecutionFailed {
        message: format!("failed to write '{}': {}", source_path.display(), error),
    })?;
    std::fs::write(&manifest_path, &generated.manifest_json).map_err(|error| {
        CliError::ExecutionFailed {
            message: format!("failed to write '{}': {}", manifest_path.display(), error),
        }
    })?;

    let view = GenerateView {
        config_path: resolved.display().to_string(),
        session: args.session,
        output_dir: output_dir.display().to_string(),
        module_name: generated.module_name,
        source_path: source_path.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        cache: format!(
            "{} (imports {}/{})",
            if cache_report.compilation_hit {
                "hit"
            } else {
                "miss"
            },
            cache_report.import_hits,
            cache_report.import_misses
        ),
    };

    ctx.output().write(&view)?;
    Ok(CommandOutput::quiet_success())
}

async fn control_modbus(
    ctx: &mut CliContext,
    global_config: Option<&Path>,
    args: ModbusControlArgs,
    readiness_timeout: Duration,
) -> CliResult<CommandOutput> {
    let config_path = global_config.ok_or_else(|| CliError::InvalidConfig {
        message: "mabi control modbus requires --config <file>".into(),
    })?;
    let (_, compiled) = load_compiled_modbus_session(ctx, config_path, &args.session)?;
    let registry = workspace_protocol_registry();
    let mut session = ModbusControlSession::new(registry, compiled, readiness_timeout)
        .await
        .map_err(|error| CliError::ExecutionFailed {
            message: error.to_string(),
        })?;

    let result: CliResult<CommandOutput> = match args.command {
        ModbusControlSubcommand::Session(args) => match args.command {
            ModbusSessionSubcommand::Status => {
                let status = session
                    .status()
                    .await
                    .map_err(|error| CliError::ExecutionFailed {
                        message: error.to_string(),
                    })?;
                if matches!(
                    ctx.output().format(),
                    OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
                ) {
                    ctx.output().write(&status)?;
                } else {
                    ctx.output().header("Modbus Session Status");
                    ctx.output().kv("Session", &status.session_name);
                    ctx.output().kv("Services", status.services);
                    ctx.output().kv("Devices", status.devices);
                    ctx.output().kv("Trace Enabled", status.trace_enabled);
                    ctx.output().kv("Trace Entries", status.trace_entries);
                    ctx.output().kv(
                        "Active Fault Preset",
                        status.active_fault_preset.as_deref().unwrap_or("none"),
                    );
                    ctx.output().kv(
                        "Active Behavior Set",
                        status.active_behavior_set.as_deref().unwrap_or("none"),
                    );
                }
                Ok(CommandOutput::quiet_success())
            }
            ModbusSessionSubcommand::Reset => {
                let snapshot =
                    session
                        .reset()
                        .await
                        .map_err(|error| CliError::ExecutionFailed {
                            message: error.to_string(),
                        })?;

                if matches!(
                    ctx.output().format(),
                    OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
                ) {
                    ctx.output().write(&snapshot)?;
                } else {
                    ctx.output().header("Modbus Session Reset");
                    ctx.output().kv("Session", &snapshot.status.session_name);
                    ctx.output().kv("Services", snapshot.status.services);
                    ctx.output().kv("Devices", snapshot.status.devices);
                    ctx.output()
                        .kv("Trace Entries", snapshot.status.trace_entries);
                    ctx.output().kv(
                        "Active Fault Preset",
                        snapshot
                            .status
                            .active_fault_preset
                            .as_deref()
                            .unwrap_or("none"),
                    );
                    ctx.output().kv(
                        "Active Behavior Set",
                        snapshot
                            .status
                            .active_behavior_set
                            .as_deref()
                            .unwrap_or("none"),
                    );
                }
                Ok(CommandOutput::quiet_success())
            }
            ModbusSessionSubcommand::Snapshot => {
                let snapshot =
                    session
                        .snapshot()
                        .await
                        .map_err(|error| CliError::ExecutionFailed {
                            message: error.to_string(),
                        })?;

                if matches!(
                    ctx.output().format(),
                    OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
                ) {
                    ctx.output().write(&snapshot)?;
                } else {
                    ctx.output().header("Modbus Session Snapshot");
                    ctx.output().kv("Session", &snapshot.status.session_name);
                    ctx.output().kv("Services", snapshot.status.services);
                    ctx.output().kv("Devices", snapshot.status.devices);
                    ctx.output()
                        .kv("Trace Entries", snapshot.status.trace_entries);
                    ctx.output().kv(
                        "Active Fault Preset",
                        snapshot
                            .status
                            .active_fault_preset
                            .as_deref()
                            .unwrap_or("none"),
                    );
                    ctx.output().kv(
                        "Active Behavior Set",
                        snapshot
                            .status
                            .active_behavior_set
                            .as_deref()
                            .unwrap_or("none"),
                    );
                }
                Ok(CommandOutput::quiet_success())
            }
        },
        ModbusControlSubcommand::Point(args) => match args.command {
            ModbusPointSubcommand::List(args) => {
                let query = PointCatalogQuery {
                    device_id: args.device,
                    tag_filters: parse_tag_filters(&args.tags)?,
                    labels: args.labels,
                };
                let points =
                    session
                        .list_points(&query)
                        .map_err(|error| CliError::ExecutionFailed {
                            message: error.to_string(),
                        })?;
                if matches!(
                    ctx.output().format(),
                    OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
                ) {
                    ctx.output().write(&points)?;
                } else {
                    ctx.output().header("Modbus Points");
                    let mut table = TableBuilder::new(ctx.colors_enabled())
                        .header(["Device", "Unit", "Point", "Address", "Type", "Access"]);
                    for point in &points {
                        table = table.row([
                            point.device_id.as_str(),
                            &point
                                .unit_id
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "-".into()),
                            point.point_id.as_str(),
                            &point
                                .address
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "-".into()),
                            &point
                                .register_type
                                .map(|value| format!("{:?}", value))
                                .unwrap_or_else(|| "-".into()),
                            point.access.as_str(),
                        ]);
                    }
                    table.print();
                }
                Ok(CommandOutput::quiet_success())
            }
            ModbusPointSubcommand::Read(args) => {
                let target = point_target_from_selector(&args.selector)?;
                let point =
                    session
                        .read(&target)
                        .await
                        .map_err(|error| CliError::ExecutionFailed {
                            message: error.to_string(),
                        })?;
                ctx.output().write(&point)?;
                Ok(CommandOutput::quiet_success())
            }
            ModbusPointSubcommand::Write(args) => {
                let target = point_target_from_selector(&args.selector)?;
                let value = parse_modbus_value(&args.value)?;
                session
                    .write(&target, value)
                    .await
                    .map_err(|error| CliError::ExecutionFailed {
                        message: error.to_string(),
                    })?;
                if !ctx.is_quiet() {
                    ctx.output().success("Point write applied");
                }
                Ok(CommandOutput::quiet_success())
            }
        },
        ModbusControlSubcommand::Trace(args) => match args.command {
            ModbusTraceSubcommand::Tail(args) => {
                let traces = session.tail(args.limit);
                ctx.output().write(&traces)?;
                Ok(CommandOutput::quiet_success())
            }
            ModbusTraceSubcommand::Clear => {
                session.clear();
                if !ctx.is_quiet() {
                    ctx.output().success("Trace buffer cleared");
                }
                Ok(CommandOutput::quiet_success())
            }
        },
        ModbusControlSubcommand::Faults(args) => match args.command {
            ModbusFaultSubcommand::Apply(args) => {
                let snapshot = session
                    .apply_fault_preset(&args.preset)
                    .await
                    .map_err(|error| CliError::ExecutionFailed {
                        message: error.to_string(),
                    })?;
                ctx.output().write(&snapshot)?;
                Ok(CommandOutput::quiet_success())
            }
            ModbusFaultSubcommand::Clear => {
                let snapshot = session.clear_fault_preset().await.map_err(|error| {
                    CliError::ExecutionFailed {
                        message: error.to_string(),
                    }
                })?;
                ctx.output().write(&snapshot)?;
                Ok(CommandOutput::quiet_success())
            }
        },
        ModbusControlSubcommand::Behavior(args) => match args.command {
            ModbusBehaviorSubcommand::List => {
                let payload = json!({
                    "available": session.available_behavior_sets(),
                    "active": session.active_behavior_set(),
                });
                ctx.output().write(&payload)?;
                Ok(CommandOutput::quiet_success())
            }
            ModbusBehaviorSubcommand::Apply(args) => {
                let snapshot = session
                    .apply_behavior_set(&args.behavior_set)
                    .await
                    .map_err(|error| CliError::ExecutionFailed {
                        message: error.to_string(),
                    })?;
                ctx.output().write(&snapshot)?;
                Ok(CommandOutput::quiet_success())
            }
            ModbusBehaviorSubcommand::Clear => {
                let snapshot = session.clear_behavior_set().await.map_err(|error| {
                    CliError::ExecutionFailed {
                        message: error.to_string(),
                    }
                })?;
                ctx.output().write(&snapshot)?;
                Ok(CommandOutput::quiet_success())
            }
        },
    };

    let stop_result = session
        .stop()
        .await
        .map_err(|error| CliError::ExecutionFailed {
            message: error.to_string(),
        });
    result?;
    stop_result?;
    Ok(CommandOutput::quiet_success())
}

async fn control_opcua(
    ctx: &mut CliContext,
    global_config: Option<&Path>,
    args: OpcuaControlArgs,
    readiness_timeout: Duration,
) -> CliResult<CommandOutput> {
    let config_path = global_config.ok_or_else(|| CliError::InvalidConfig {
        message: "mabi control opcua requires --config <file>".into(),
    })?;
    let (_, compiled) = load_compiled_opcua_session(ctx, config_path, &args.session)?;
    let registry = workspace_protocol_registry();
    let mut session = OpcUaControlSession::new(registry, compiled, readiness_timeout)
        .await
        .map_err(|error| CliError::ExecutionFailed {
            message: error.to_string(),
        })?;

    let result: CliResult<CommandOutput> =
        match args.command {
            OpcuaControlSubcommand::Session(args) => {
                match args.command {
                    OpcuaSessionSubcommand::Status => {
                        let status =
                            session
                                .status()
                                .await
                                .map_err(|error| CliError::ExecutionFailed {
                                    message: error.to_string(),
                                })?;
                        ctx.output().write(&status)?;
                        Ok(CommandOutput::quiet_success())
                    }
                    OpcuaSessionSubcommand::Reset => {
                        let snapshot =
                            session
                                .reset()
                                .await
                                .map_err(|error| CliError::ExecutionFailed {
                                    message: error.to_string(),
                                })?;
                        ctx.output().write(&snapshot)?;
                        Ok(CommandOutput::quiet_success())
                    }
                    OpcuaSessionSubcommand::Snapshot => {
                        let snapshot = session.snapshot().await.map_err(|error| {
                            CliError::ExecutionFailed {
                                message: error.to_string(),
                            }
                        })?;
                        ctx.output().write(&snapshot)?;
                        Ok(CommandOutput::quiet_success())
                    }
                }
            }
            OpcuaControlSubcommand::Node(args) => {
                match args.command {
                    OpcuaNodeSubcommand::List(args) => {
                        let mut nodes =
                            session
                                .list_nodes()
                                .map_err(|error| CliError::ExecutionFailed {
                                    message: error.to_string(),
                                })?;
                        if let Some(device) = args.device {
                            nodes.retain(|node| node.device_id == device);
                        }

                        if matches!(
                            ctx.output().format(),
                            OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
                        ) {
                            ctx.output().write(&nodes)?;
                        } else {
                            ctx.output().header("OPC UA Nodes");
                            let mut table = TableBuilder::new(ctx.colors_enabled())
                                .header(["Device", "Point", "NodeId", "Class", "Writable"]);
                            for node in &nodes {
                                table = table.row([
                                    node.device_id.as_str(),
                                    node.point_id.as_str(),
                                    node.node_id.as_str(),
                                    node.node_class.as_str(),
                                    if node.writable { "yes" } else { "no" },
                                ]);
                            }
                            table.print();
                        }
                        Ok(CommandOutput::quiet_success())
                    }
                    OpcuaNodeSubcommand::Read(args) => {
                        let target = node_target_from_selector(&args.selector)?;
                        let point = session.read(&target).await.map_err(|error| {
                            CliError::ExecutionFailed {
                                message: error.to_string(),
                            }
                        })?;
                        ctx.output().write(&point)?;
                        Ok(CommandOutput::quiet_success())
                    }
                    OpcuaNodeSubcommand::Write(args) => {
                        let target = node_target_from_selector(&args.selector)?;
                        let value = parse_opcua_value(&args.value)?;
                        session.write(&target, value).await.map_err(|error| {
                            CliError::ExecutionFailed {
                                message: error.to_string(),
                            }
                        })?;
                        if !ctx.is_quiet() {
                            ctx.output().success("OPC UA node write completed");
                        }
                        Ok(CommandOutput::quiet_success())
                    }
                }
            }
            OpcuaControlSubcommand::Security(args) => match args.command {
                OpcuaSecuritySubcommand::Status => {
                    let status = session.security_status().await.map_err(|error| {
                        CliError::ExecutionFailed {
                            message: error.to_string(),
                        }
                    })?;
                    ctx.output().write(&status)?;
                    Ok(CommandOutput::quiet_success())
                }
                OpcuaSecuritySubcommand::TrustReload => {
                    let status = session.trust_reload().await.map_err(|error| {
                        CliError::ExecutionFailed {
                            message: error.to_string(),
                        }
                    })?;
                    ctx.output().write(&status)?;
                    Ok(CommandOutput::quiet_success())
                }
                OpcuaSecuritySubcommand::Rotate(args) => {
                    let status = session
                        .rotate_server_certificate(args.certificate, args.private_key)
                        .await
                        .map_err(|error| CliError::ExecutionFailed {
                            message: error.to_string(),
                        })?;
                    ctx.output().write(&status)?;
                    Ok(CommandOutput::quiet_success())
                }
                OpcuaSecuritySubcommand::AuditSummary => {
                    let summary = session.audit_summary().await.map_err(|error| {
                        CliError::ExecutionFailed {
                            message: error.to_string(),
                        }
                    })?;
                    ctx.output().write(&summary)?;
                    Ok(CommandOutput::quiet_success())
                }
            },
        };

    let stop_result = session
        .stop()
        .await
        .map_err(|error| CliError::ExecutionFailed {
            message: error.to_string(),
        });
    result?;
    stop_result?;
    Ok(CommandOutput::quiet_success())
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
        into_launch_spec, ModbusServeArgs, OpcuaServeArgs, SecurityModeArg, ServeArgs,
        ServeProtocolCommand,
    };
    use crate::CliContext;

    #[tokio::test]
    async fn serve_request_maps_to_runtime_request() {
        let ctx = CliContext::builder().build().unwrap();
        let (request, _) = into_launch_spec(
            &ctx,
            None,
            ServeProtocolCommand::Modbus(ModbusServeArgs {
                serve: ServeArgs::default(),
                session: None,
                port: 1502,
                bind: "127.0.0.1".into(),
                devices: 2,
                points: 32,
            }),
        )
        .await
        .unwrap();

        assert_eq!(request.protocol, "modbus");
        assert_eq!(request.config["transport"]["kind"], "tcp");
        assert_eq!(request.config["transport"]["bind_addr"], "127.0.0.1:1502");
        assert_eq!(request.config["devices"], 2);
        assert_eq!(request.config["points_per_device"], 32);
    }

    #[tokio::test]
    async fn opcua_legacy_serve_request_is_removed() {
        let ctx = CliContext::builder().build().unwrap();
        let result = into_launch_spec(
            &ctx,
            None,
            ServeProtocolCommand::Opcua(OpcuaServeArgs {
                serve: ServeArgs {
                    name: Some("opcua-demo".into()),
                },
                session: None,
                port: 14840,
                bind: "127.0.0.1".into(),
                endpoint: "/sim".into(),
                nodes: 24,
                security: SecurityModeArg::None,
            }),
        )
        .await;

        let error = match result {
            Ok(_) => panic!("expected legacy OPC UA serve flow to be removed"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains(crate::OPCUA_LEGACY_COMPAT_MESSAGE));
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
