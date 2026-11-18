//! Command line definition. Help text: purpose, then arguments and options, then examples.

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

pub const DOCS: &str = "https://hydronode.tech/docs/guide/cli";

#[derive(Parser, Debug)]
#[command(
    name = "hn",
    version,
    about = "HydroNode from your terminal: sensors, values, devices and live data.",
    disable_help_subcommand = false,
    max_term_width = 100
)]
pub struct Cli {
    /// Profile to use (server and login), see 'hn profile list'
    #[arg(
        short = 'p',
        long,
        global = true,
        env = "HYDRONODE_PROFILE",
        hide_env = true,
        value_name = "NAME",
        help_heading = "Global options"
    )]
    pub profile: Option<String>,

    /// Print JSON instead of tables (stable shape, for scripts)
    #[arg(long, global = true, help_heading = "Global options")]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

macro_rules! examples {
    ($($line:literal),+ $(,)?) => {
        concat!("Examples:\n", $("  ", $line, "\n"),+)
    };
}

#[derive(Subcommand, Debug)]
pub enum Command {
    // ---- Start
    /// Sign in (browser, device code or API token)
    #[command(
        long_about = "Sign in to HydroNode. Without a flag you choose how.\n\n\
            --web opens the browser and signs you in like the web app.\n\
            --device shows a code to confirm on another device (SSH, no browser).\n\
            --token / --with-token use an API token: hat_ reads and writes, hrt_ only reads.",
        after_help = examples!(
            "hn login",
            "hn login --device",
            "echo \"$HYDRONODE_TOKEN\" | hn login --with-token",
            "hn -p work login --web"
        )
    )]
    Login(LoginArgs),

    /// Sign out and forget the stored login
    #[command(
        long_about = "Sign out: ends this CLI session on the server and removes the stored login.\n\
            An API token is only forgotten here, revoke it with 'hn tokens revoke'.",
        after_help = examples!("hn logout", "hn -p work logout")
    )]
    Logout,

    /// Show who you are signed in as
    #[command(
        long_about = "Show the account, server and kind of login of the current profile.",
        after_help = examples!("hn whoami", "hn whoami --json")
    )]
    Whoami,

    /// Devices, anomalies and messages at a glance
    #[command(
        long_about = "One screen: projects, device health, open anomalies, unread messages and\n\
            what needs attention.",
        after_help = examples!("hn status", "hn status --json | jq .devices")
    )]
    Status,

    // ---- Data
    /// List, show, create, rename and delete sensors
    #[command(
        long_about = "Your sensors and the ones shared with you.",
        after_help = examples!(
            "hn sensors",
            "hn sensors show <sensor-id>",
            "hn sensors create \"Balcony\" --outdoor",
            "hn sensors rename <sensor-id> \"Balcony east\"",
            "hn sensors credentials <sensor-id>"
        )
    )]
    Sensors(SensorsArgs),

    /// Summaries of a data point over time
    #[command(
        long_about = "Min, max, average, trend and gaps of one data point, in at most 48 steps.\n\
            TARGET is a data point id or SENSOR/TYPE (for example <sensor-id>/temperature).",
        args_conflicts_with_subcommands = true,
        subcommand_negates_reqs = true,
        after_help = examples!(
            "hn values <sensor-id>/temperature",
            "hn values <point-id> --from 7d",
            "hn values <sensor-id>/soil_moisture --below 30 --from 30d",
            "hn values <point-id> --from 2h --raw",
            "hn values compare <point-id> --shift 7d"
        )
    )]
    Values(ValuesArgs),

    /// Live values in the terminal
    #[command(
        long_about = "Live values of your devices as they arrive. Full-screen view, q quits.\n\
            --plain prints one line per value instead (good for logs and pipes).\n\
            Needs a browser or device login.",
        after_help = examples!(
            "hn watch",
            "hn watch <sensor-id> <sensor-id>",
            "hn watch --plain | grep TEMPERATURE"
        )
    )]
    Watch(WatchArgs),

    /// Set a writable data point (switch, setpoint)
    #[command(
        long_about = "Set the value of a writable data point and wait until the device confirms it.\n\
            VALUE: on/off/true/false, a number or text.",
        after_help = examples!(
            "hn set <point-id> on",
            "hn set <sensor-id>/target_temperature 21.5",
            "hn set <point-id> off --wait 0"
        )
    )]
    Set(SetArgs),

    /// Send a command to a device
    #[command(
        long_about = "Queue a command. The device gets it with its next delivery and confirms or\n\
            declines it. Without --type the type is guessed from the value.",
        after_help = examples!(
            "hn command <sensor-id> relay1 true",
            "hn command <sensor-id> relay1 1400 --type uint32",
            "hn command <sensor-id> mode eco --type string"
        )
    )]
    Command(CommandArgs),

    /// Download readings as CSV, JSON or Parquet
    #[command(
        long_about = "Export readings of your own sensors. CSV and JSON go to stdout unless -o is set.",
        after_help = examples!(
            "hn export --from 7d > readings.csv",
            "hn export --sensor <sensor-id> --format json --resolution 1h",
            "hn export --from 30d --format parquet -o readings.parquet"
        )
    )]
    Export(ExportArgs),

    // ---- Devices
    /// Firmware, health and updates over the air
    #[command(
        long_about = "Devices you manage: firmware, health, battery, signal and running updates.\n\
            'update' checks first, shows what would change and asks before it starts.",
        after_help = examples!(
            "hn devices",
            "hn devices show <sensor-id>",
            "hn devices update <sensor-id> --firmware latest",
            "hn devices update <id> <id> --interval 5m --yes"
        )
    )]
    Devices(DevicesArgs),

    /// What devices sent and why something was rejected
    #[command(
        long_about = "Recent requests of your devices (last 48 hours), with the reason when one was\n\
            rejected. Without SENSOR: all devices you manage.",
        after_help = examples!(
            "hn console",
            "hn console <sensor-id> -f",
            "hn console --rejected"
        )
    )]
    Console(ConsoleArgs),

    /// Send a signed value like a device
    #[command(
        long_about = "Send one round of values, signed with the sensor secret, exactly like a device.\n\
            The secret comes from HYDRONODE_SENSOR_SECRET, --secret-stdin, or your login (owner).",
        after_help = examples!(
            "hn push <sensor-id> TEMPERATURE=21.4 HUMIDITY=48",
            "HYDRONODE_SENSOR_SECRET=… hn push <sensor-id> SOIL_MOISTURE=41"
        )
    )]
    Push(PushArgs),

    /// Run a fake device that sends signed values
    #[command(
        long_about = "A fake HTTP device that sends plausible values on a schedule. It writes real\n\
            data into the sensor, so use a test sensor. Ctrl-C stops it.",
        after_help = examples!(
            "hn sensors create \"Test device\"",
            "hn simulate <sensor-id> --kind weather",
            "hn simulate <sensor-id> --kind greenhouse --every 1m --count 10"
        )
    )]
    Simulate(SimulateArgs),

    // ---- More
    /// Open anomalies on your sensors
    #[command(
        long_about = "Anomalies found by the analysis. Open ones by default.",
        after_help = examples!("hn anomalies", "hn anomalies --all --since 30d", "hn anomalies show <id>")
    )]
    Anomalies(AnomaliesArgs),

    /// Your inbox
    #[command(
        long_about = "Messages about devices, your account and news.",
        after_help = examples!(
            "hn messages --unread",
            "hn messages read <id> <id>",
            "hn messages read --all",
            "hn messages archive <id>"
        )
    )]
    Messages(MessagesArgs),

    /// Public weather stations
    #[command(
        long_about = "Search public stations by name or near a place, with their current values.",
        after_help = examples!("hn stations berlin", "hn stations --near 52.52,13.40 --radius 5")
    )]
    Stations(StationsArgs),

    /// List and switch automations
    #[command(
        long_about = "Your automations, what triggers them and when they last ran.",
        after_help = examples!("hn automations", "hn automations show <id>", "hn automations disable <id>")
    )]
    Automations(AutomationsArgs),

    /// API tokens for scripts and integrations
    #[command(
        long_about = "Personal API tokens: hat_ reads and writes, hrt_ only reads. A new token is\n\
            shown once.",
        after_help = examples!("hn tokens", "hn tokens create \"Home Assistant\"", "hn tokens create ci --read", "hn tokens revoke <id>")
    )]
    Tokens(TokensArgs),

    /// Where you are signed in
    #[command(
        long_about = "Signed-in browsers, apps and CLIs of your account. Sign one or all others out.",
        after_help = examples!("hn sessions", "hn sessions revoke <id>", "hn sessions revoke --others")
    )]
    Sessions(SessionsArgs),

    /// Send a raw API request
    #[command(
        long_about = "Call any HydroNode API path with your login and print the answer.\n\
            METHOD defaults to GET. The path must stay on the profile's server.",
        after_help = examples!(
            "hn api /api/agent/v1/me",
            "hn api GET /api/fleet/v2/firmware",
            "hn api PATCH /api/sensors/<id> -d '{\"name\":\"Tank\"}'",
            "hn api GET /api/agent/v1/find -q q=greenhouse"
        )
    )]
    Api(ApiArgs),

    /// Show and change settings
    #[command(
        long_about = "Settings in config.toml (never secrets). Keys: default-profile.",
        after_help = examples!("hn config", "hn config set default-profile work", "hn config path")
    )]
    Config(ConfigArgs),

    /// Servers and accounts (for example a second account)
    #[command(
        long_about = "A profile is a server plus its own login. Use one with -p NAME.",
        after_help = examples!(
            "hn profile add work --server https://hydronode.tech",
            "hn -p work login",
            "hn profile use work",
            "hn profile list"
        )
    )]
    Profile(ProfileArgs),

    /// Shell completion script
    #[command(
        long_about = "Print a completion script for your shell.",
        after_help = examples!(
            "hn completion zsh > \"${fpath[1]}/_hn\"",
            "hn completion bash > ~/.local/share/bash-completion/completions/hn",
            "hn completion fish > ~/.config/fish/completions/hn.fish"
        )
    )]
    Completion(CompletionArgs),

    /// Open HydroNode in the browser
    #[command(
        long_about = "Open the dashboard, a sensor or another page of the profile's server.\n\
            WHAT: a sensor id, or dashboard, devices, messages, stations, docs.",
        after_help = examples!("hn open", "hn open <sensor-id>", "hn open messages --print")
    )]
    Open(OpenArgs),
}

/// Which group a top-level command belongs to in the overview.
pub const GROUPS: &[(&str, &[&str])] = &[
    ("Start", &["login", "logout", "whoami", "status"]),
    (
        "Data",
        &["sensors", "values", "watch", "set", "command", "export"],
    ),
    ("Devices", &["devices", "console", "push", "simulate"]),
    (
        "More",
        &[
            "anomalies",
            "messages",
            "stations",
            "automations",
            "tokens",
            "sessions",
            "api",
            "config",
            "profile",
            "completion",
            "open",
        ],
    ),
];

#[derive(Args, Debug)]
pub struct LoginArgs {
    /// Sign in with the browser (recommended)
    #[arg(long, conflicts_with_all = ["device", "token", "with_token"])]
    pub web: bool,
    /// Show a code to confirm on another device
    #[arg(long, conflicts_with_all = ["token", "with_token"])]
    pub device: bool,
    /// Paste an API token (hidden input)
    #[arg(long, conflicts_with = "with_token")]
    pub token: bool,
    /// Read an API token from stdin
    #[arg(long)]
    pub with_token: bool,
    /// Do not open a browser, only print the link
    #[arg(
        long,
        env = "HYDRONODE_NO_BROWSER",
        hide_env = true,
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub no_browser: bool,
}

#[derive(Args, Debug)]
pub struct SensorsArgs {
    #[command(subcommand)]
    pub cmd: Option<SensorsCmd>,
}

#[derive(Subcommand, Debug)]
pub enum SensorsCmd {
    /// List your sensors and the ones shared with you
    #[command(after_help = examples!("hn sensors list", "hn sensors list --json | jq '.[].id'"))]
    List,
    /// Data points, status, battery and signal of one sensor
    #[command(after_help = examples!("hn sensors show <sensor-id>"))]
    Show {
        /// Sensor id
        sensor: String,
    },
    /// Create a sensor (HTTP device)
    #[command(after_help = examples!("hn sensors create \"Balcony\" --outdoor", "hn sensors create \"Test device\""))]
    Create {
        /// Name of the new sensor
        name: String,
        /// The sensor is outdoors (needed for a station)
        #[arg(long)]
        outdoor: bool,
    },
    /// Rename a sensor
    #[command(after_help = examples!("hn sensors rename <sensor-id> \"Balcony east\""))]
    Rename {
        /// Sensor id
        sensor: String,
        /// New name
        name: String,
    },
    /// Delete a sensor and all its readings
    #[command(
        long_about = "Delete a sensor with all its readings, anomalies, commands and shares.\n\
            This cannot be undone. Asks first unless --yes is given.",
        after_help = examples!("hn sensors delete <sensor-id>", "hn sensors delete <sensor-id> --yes")
    )]
    Delete {
        /// Sensor id
        sensor: String,
        /// Do not ask
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Show the sensor id and secret for a device (owner only)
    #[command(
        long_about = "Print the sensor id and secret a device signs with. Owner only.\n\
            Anyone with the secret can send values as this sensor: keep it private.",
        after_help = examples!(
            "hn sensors credentials <sensor-id>",
            "hn sensors credentials <sensor-id> --json | jq -r .secret"
        )
    )]
    Credentials {
        /// Sensor id
        sensor: String,
    },
}

#[derive(Args, Debug)]
pub struct TimeRange {
    /// Start: 24h, 7d, 90m, 2026-11-01 or 2026-11-01T10:00+01:00
    #[arg(long, default_value = "24h", value_name = "TIME")]
    pub from: String,
    /// End, same forms as --from
    #[arg(long, default_value = "now", value_name = "TIME")]
    pub to: String,
}

#[derive(Args, Debug)]
pub struct ValuesArgs {
    #[command(subcommand)]
    pub cmd: Option<ValuesCmd>,
    /// Data point id or SENSOR/TYPE
    #[arg(required = true)]
    pub target: Option<String>,
    #[command(flatten)]
    pub range: TimeRange,
    /// Single readings instead of a summary (browser, device or hat_ login)
    #[arg(long, conflicts_with_all = ["below", "above"])]
    pub raw: bool,
    /// Periods when the value was below N
    #[arg(
        long,
        value_name = "N",
        conflicts_with = "above",
        allow_negative_numbers = true
    )]
    pub below: Option<f64>,
    /// Periods when the value was above N
    #[arg(long, value_name = "N", allow_negative_numbers = true)]
    pub above: Option<f64>,
}

#[derive(Subcommand, Debug)]
pub enum ValuesCmd {
    /// Up to four data points side by side, or one against an earlier period
    #[command(
        long_about = "Compare up to four data points over the same time, or one point with itself\n\
            shifted back (--shift 7d = the same hours a week earlier).",
        after_help = examples!(
            "hn values compare <point-id> <point-id>",
            "hn values compare <sensor-id>/temperature --shift 7d --from 24h"
        )
    )]
    Compare {
        /// Data point ids or SENSOR/TYPE (1 to 4)
        #[arg(required = true, num_args = 1..=4)]
        targets: Vec<String>,
        #[command(flatten)]
        range: TimeRange,
        /// Compare one point with itself this long ago (for example 7d)
        #[arg(long, value_name = "SPAN")]
        shift: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct WatchArgs {
    /// Only these sensors (ids or names), default all
    pub sensors: Vec<String>,
    /// One line per value instead of the full-screen view
    #[arg(long)]
    pub plain: bool,
}

#[derive(Args, Debug)]
pub struct SetArgs {
    /// Data point id or SENSOR/TYPE
    pub target: String,
    /// New value: on, off, true, false, a number or text
    #[arg(allow_hyphen_values = true)]
    pub value: String,
    /// Seconds to wait for the device to confirm (0 to 20)
    #[arg(long, value_name = "SECONDS", default_value_t = 10, value_parser = clap::value_parser!(u8).range(0..=20))]
    pub wait: u8,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandType {
    Bool,
    Int32,
    Uint32,
    Int64,
    Uint64,
    String,
}

#[derive(Args, Debug)]
pub struct CommandArgs {
    /// Sensor id
    pub sensor: String,
    /// Command name the device listens to (for example relay1)
    pub name: String,
    /// Value to send
    #[arg(allow_hyphen_values = true)]
    pub value: String,
    /// Value type, guessed when left out
    #[arg(long = "type", value_enum, value_name = "TYPE")]
    pub value_type: Option<CommandType>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    Parquet,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Raw,
    #[value(name = "5m")]
    FiveMinutes,
    #[value(name = "1h")]
    Hour,
    #[value(name = "1d")]
    Day,
}

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// File format
    #[arg(long, value_enum, default_value = "csv")]
    pub format: ExportFormat,
    /// Only these sensors (repeat), default all you own
    #[arg(long = "sensor", value_name = "ID")]
    pub sensors: Vec<String>,
    /// Single readings or averages per 5 minutes, hour or day
    #[arg(long, value_enum, default_value = "raw")]
    pub resolution: Resolution,
    /// Start: 7d, 2026-11-01, …
    #[arg(long, default_value = "7d", value_name = "TIME")]
    pub from: String,
    /// End
    #[arg(long, default_value = "now", value_name = "TIME")]
    pub to: String,
    /// Write to this file instead of stdout
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<std::path::PathBuf>,
}

#[derive(Args, Debug)]
pub struct DevicesArgs {
    #[command(subcommand)]
    pub cmd: Option<DevicesCmd>,
}

#[derive(Subcommand, Debug)]
pub enum DevicesCmd {
    /// Devices you manage with firmware, health, battery and signal
    #[command(after_help = examples!("hn devices list", "hn devices list --json"))]
    List,
    /// Details, error rates and recent jobs of one device
    #[command(after_help = examples!("hn devices show <sensor-id>"))]
    Show {
        /// Sensor id
        sensor: String,
    },
    /// Update firmware or settings over the air
    #[command(
        long_about = "Change firmware, send interval, power mode or add the WiFi signal value on one\n\
            or more devices. HydroNode checks every device first; you see what changes and\n\
            confirm. Devices that cannot take the change are skipped.",
        after_help = examples!(
            "hn devices update <sensor-id> --firmware latest",
            "hn devices update <id> <id> --interval 5m",
            "hn devices update <sensor-id> --power deep-sleep --yes"
        )
    )]
    Update(DevicesUpdateArgs),
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerMode {
    AlwaysOn,
    ModemSleep,
    LightSleep,
    DeepSleep,
    Hibernate,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyMode {
    Strict,
    Ingest,
}

#[derive(Args, Debug)]
pub struct DevicesUpdateArgs {
    /// Sensor ids (1 to 200)
    #[arg(required = true, num_args = 1..=200)]
    pub sensors: Vec<String>,
    /// Firmware version or "latest"
    #[arg(long, value_name = "VERSION")]
    pub firmware: Option<String>,
    /// Send interval (10s to 7d)
    #[arg(long, value_name = "SPAN")]
    pub interval: Option<String>,
    /// Power mode
    #[arg(long, value_enum)]
    pub power: Option<PowerMode>,
    /// Also send the WiFi signal (RSSI)
    #[arg(long)]
    pub add_rssi: bool,
    /// How the device proves the update worked
    #[arg(long, value_enum)]
    pub verify: Option<VerifyMode>,
    /// Go ahead for devices below 30 % battery too
    #[arg(long)]
    pub confirm_low_battery: bool,
    /// Do not ask
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct ConsoleArgs {
    /// Sensor id, default all devices you manage
    pub sensor: Option<String>,
    /// Keep printing new lines
    #[arg(short = 'f', long)]
    pub follow: bool,
    /// Only rejected requests
    #[arg(long)]
    pub rejected: bool,
    /// Number of lines (1 to 200)
    #[arg(short = 'n', long, default_value_t = 30, value_parser = clap::value_parser!(u16).range(1..=200))]
    pub limit: u16,
}

#[derive(Args, Debug)]
pub struct PushArgs {
    /// Sensor id
    pub sensor: String,
    /// TYPE=VALUE pairs (1 to 64), for example TEMPERATURE=21.4
    #[arg(required = true, num_args = 1..=64, value_name = "TYPE=VALUE")]
    pub values: Vec<String>,
    /// Read the sensor secret from stdin
    #[arg(long)]
    pub secret_stdin: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimProfile {
    Weather,
    Greenhouse,
    Random,
}

#[derive(Args, Debug)]
pub struct SimulateArgs {
    /// Sensor id (use a test sensor)
    pub sensor: String,
    /// What the fake device measures
    #[arg(long, value_enum, default_value = "weather")]
    pub kind: SimProfile,
    /// Time between rounds, at least 10s
    #[arg(long, default_value = "30s", value_name = "SPAN")]
    pub every: String,
    /// Stop after this many rounds
    #[arg(long, value_name = "N")]
    pub count: Option<u64>,
    /// Read the sensor secret from stdin
    #[arg(long)]
    pub secret_stdin: bool,
}

#[derive(Args, Debug)]
pub struct AnomaliesArgs {
    #[command(subcommand)]
    pub cmd: Option<AnomaliesCmd>,
    /// Also closed ones
    #[arg(long)]
    pub all: bool,
    /// Only this sensor
    #[arg(long, value_name = "ID")]
    pub sensor: Option<String>,
    /// Only since (7d, 2026-11-01, …)
    #[arg(long, value_name = "TIME")]
    pub since: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum AnomaliesCmd {
    /// One anomaly with explanation and values
    #[command(after_help = examples!("hn anomalies show <id>"))]
    Show {
        /// Anomaly id
        id: String,
    },
}

#[derive(Args, Debug)]
pub struct MessagesArgs {
    #[command(subcommand)]
    pub cmd: Option<MessagesCmd>,
    /// Only unread messages
    #[arg(long)]
    pub unread: bool,
}

#[derive(Subcommand, Debug)]
pub enum MessagesCmd {
    /// Mark messages as read
    #[command(after_help = examples!("hn messages read <id> <id>", "hn messages read --all"))]
    Read {
        /// Message ids
        #[arg(required_unless_present = "all")]
        ids: Vec<String>,
        /// Every message
        #[arg(long, conflicts_with = "ids")]
        all: bool,
    },
    /// Mark messages as unread
    #[command(after_help = examples!("hn messages unread <id>"))]
    Unread {
        /// Message ids
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Move messages out of the inbox
    #[command(after_help = examples!("hn messages archive <id>"))]
    Archive {
        /// Message ids
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Bring archived messages back
    #[command(after_help = examples!("hn messages restore <id>"))]
    Restore {
        /// Message ids
        #[arg(required = true)]
        ids: Vec<String>,
    },
}

#[derive(Args, Debug)]
pub struct StationsArgs {
    /// Search by name
    pub query: Option<String>,
    /// Near a place: LAT,LON
    #[arg(long, value_name = "LAT,LON", allow_hyphen_values = true)]
    pub near: Option<String>,
    /// Radius around --near in km (up to 100)
    #[arg(long, value_name = "KM", default_value_t = 10, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub radius: u8,
}

#[derive(Args, Debug)]
pub struct AutomationsArgs {
    #[command(subcommand)]
    pub cmd: Option<AutomationsCmd>,
}

#[derive(Subcommand, Debug)]
pub enum AutomationsCmd {
    /// List automations
    #[command(after_help = examples!("hn automations list"))]
    List,
    /// Trigger, actions and recent runs of one automation
    #[command(after_help = examples!("hn automations show <id>"))]
    Show {
        /// Automation id
        id: String,
    },
    /// Switch an automation on
    #[command(after_help = examples!("hn automations enable <id>"))]
    Enable {
        /// Automation id
        id: String,
    },
    /// Switch an automation off
    #[command(after_help = examples!("hn automations disable <id>"))]
    Disable {
        /// Automation id
        id: String,
    },
}

#[derive(Args, Debug)]
pub struct TokensArgs {
    #[command(subcommand)]
    pub cmd: Option<TokensCmd>,
}

#[derive(Subcommand, Debug)]
pub enum TokensCmd {
    /// List your API tokens (never the token itself)
    #[command(after_help = examples!("hn tokens list"))]
    List,
    /// Create a token and show it once
    #[command(after_help = examples!("hn tokens create \"Home Assistant\"", "hn tokens create ci --read"))]
    Create {
        /// What the token is for
        name: String,
        /// Read-only token (hrt_)
        #[arg(long)]
        read: bool,
    },
    /// Revoke a token
    #[command(after_help = examples!("hn tokens revoke <id>", "hn tokens revoke <id> --yes"))]
    Revoke {
        /// Token id from 'hn tokens list'
        id: String,
        /// Do not ask
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Args, Debug)]
pub struct SessionsArgs {
    #[command(subcommand)]
    pub cmd: Option<SessionsCmd>,
}

#[derive(Subcommand, Debug)]
pub enum SessionsCmd {
    /// List where you are signed in
    #[command(after_help = examples!("hn sessions list"))]
    List,
    /// Sign a session out, or all others
    #[command(after_help = examples!("hn sessions revoke <id>", "hn sessions revoke --others --yes"))]
    Revoke {
        /// Session id from 'hn sessions list'
        #[arg(required_unless_present = "others")]
        id: Option<String>,
        /// Every session except this one
        #[arg(long, conflicts_with = "id")]
        others: bool,
        /// Do not ask
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Args, Debug)]
pub struct ApiArgs {
    /// [METHOD] PATH, for example GET /api/sensors
    #[arg(required = true, num_args = 1..=2, value_name = "METHOD PATH")]
    pub args: Vec<String>,
    /// JSON body (or @file, or - for stdin)
    #[arg(short = 'd', long, value_name = "JSON")]
    pub data: Option<String>,
    /// Query parameter key=value (repeat)
    #[arg(short = 'q', long = "query", value_name = "KEY=VALUE")]
    pub query: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub cmd: Option<ConfigCmd>,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Show all settings
    #[command(after_help = examples!("hn config list"))]
    List,
    /// Show one setting
    #[command(after_help = examples!("hn config get default-profile"))]
    Get {
        /// Setting name
        key: String,
    },
    /// Change a setting
    #[command(after_help = examples!("hn config set default-profile work"))]
    Set {
        /// Setting name
        key: String,
        /// New value
        value: String,
    },
    /// Print where config.toml lives
    #[command(after_help = examples!("hn config path"))]
    Path,
}

#[derive(Args, Debug)]
pub struct ProfileArgs {
    #[command(subcommand)]
    pub cmd: Option<ProfileCmd>,
}

#[derive(Subcommand, Debug)]
pub enum ProfileCmd {
    /// List profiles and their servers
    #[command(after_help = examples!("hn profile list"))]
    List,
    /// Add or change a profile
    #[command(
        long_about = "Add a profile, or change the server of an existing one. HTTPS only, plain http\n\
            only for localhost and 127.0.0.1.",
        after_help = examples!(
            "hn profile add work --server https://hydronode.tech",
            "hn profile add local --server http://localhost:8080"
        )
    )]
    Add {
        /// Profile name
        name: String,
        /// Server URL
        #[arg(long, value_name = "URL")]
        server: String,
    },
    /// Remove a profile and its stored login
    #[command(after_help = examples!("hn profile remove work"))]
    Remove {
        /// Profile name
        name: String,
    },
    /// Use a profile by default
    #[command(after_help = examples!("hn profile use work", "hn profile use default"))]
    Use {
        /// Profile name
        name: String,
    },
}

#[derive(Args, Debug)]
pub struct CompletionArgs {
    /// Shell
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

#[derive(Args, Debug)]
pub struct OpenArgs {
    /// Sensor id or page: dashboard, devices, messages, stations, docs
    pub what: Option<String>,
    /// Only print the link
    #[arg(long)]
    pub print: bool,
}

/// The command with the grouped overview as its help, named after the binary that runs.
pub fn command(bin: &str) -> clap::Command {
    let cmd = Cli::command()
        .name(bin.to_string())
        .bin_name(bin.to_string());
    let mut overview = String::new();
    for (group, names) in GROUPS {
        overview.push_str(&format!("{group}:\n"));
        for name in *names {
            let sub = cmd
                .find_subcommand(name)
                .unwrap_or_else(|| panic!("command {name} missing"));
            let about = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
            overview.push_str(&format!("  {name:<12} {about}\n"));
        }
        overview.push('\n');
    }
    let template = format!(
        "{{about}}\n\n{{usage-heading}} {{usage}}\n\n{overview}Options:\n{{options}}\n\n\
         Run '{bin} help <command>' for details. Docs: {DOCS}\n"
    );
    let cmd = cmd.help_template(template);
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    names
        .iter()
        .fold(cmd, |cmd, name| cmd.mut_subcommand(name, tidy))
}

/// Subcommand pages: purpose (long text) first, then arguments and options, then examples.
/// Only short help exists, so `-h`, `--help` and `hn help <cmd>` print the same compact page.
fn tidy(cmd: clap::Command) -> clap::Command {
    let purpose = cmd
        .get_long_about()
        .or_else(|| cmd.get_about())
        .map(|t| t.to_string())
        .unwrap_or_default();
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    let cmd = cmd
        .long_about(None)
        .disable_help_subcommand(true)
        .before_help(purpose)
        .help_template("{before-help}{usage-heading} {usage}\n\n{all-args}{after-help}");
    names
        .iter()
        .fold(cmd, |cmd, name| cmd.mut_subcommand(name, tidy))
}

#[cfg(test)]
mod tests {
    #[test]
    fn definition_is_consistent() {
        super::command("hn").debug_assert();
    }
}
