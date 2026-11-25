use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type DeviceId = Uuid;
pub type CommandId = Uuid;
pub type SerialSessionId = Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Protocol: {0}")]
    Protocol(&'static str),
    #[error("Safety interlock: {0}")]
    Safety(&'static str),
    #[error("Storage: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("Backend HTTP {0}")]
    Http(u16),
    #[error("Backend unavailable")]
    Network,
    #[error("Secure credential storage unavailable")]
    Credentials,
    #[error("Invalid configuration: {0}")]
    Config(&'static str),
    #[error("Service unavailable")]
    Unavailable,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HardwareButton {
    Up,
    Down,
    Left,
    Right,
    Yes,
    No,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ButtonEdge {
    Press,
    Release,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConveyorState {
    Disconnected,
    Initializing,
    Ready,
    Moving,
    Arrived,
    Fault,
    EStop,
    NeedsInspection,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteCommand {
    pub contract: String,
    pub command_id: CommandId,
    pub device_id: DeviceId,
    pub action: String,
    pub target_station: u8,
    pub order_id: u64,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub lease_token: String,
    pub attempt: u32,
    pub version: u32,
}
impl RemoteCommand {
    pub fn validate(&self, device: DeviceId, now: DateTime<Utc>) -> Result<()> {
        if self.contract != "hardware.v1"
            || self.version != 1
            || self.action != "MOVE_TO"
            || self.device_id != device
            || self.command_id.is_nil()
            || self.order_id == 0
            || !(1..=6).contains(&self.target_station)
            || self.attempt == 0
            || self.lease_token.is_empty()
            || self.lease_token.len() > 512
            || self.issued_at > now
            || self.expires_at <= now
            || self.expires_at <= self.issued_at
        {
            return Err(Error::Safety("invalid, fenced or expired command"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareEvent {
    pub contract: String,
    pub event_id: Uuid,
    pub device_id: DeviceId,
    pub session_id: SerialSessionId,
    pub seq: u32,
    pub button: HardwareButton,
    pub edge: ButtonEdge,
    pub occurred_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub browser_session_id: Option<Uuid>,
    pub simulation: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecutionResult {
    pub contract: String,
    pub event_id: Uuid,
    pub command_id: CommandId,
    pub version: u32,
    pub kind: String,
    pub occurred_at: DateTime<Utc>,
}
impl CommandExecutionResult {
    pub fn new(id: CommandId, kind: &str) -> Self {
        Self {
            contract: "hardware.v1".into(),
            event_id: Uuid::new_v4(),
            command_id: id,
            version: 1,
            kind: kind.into(),
            occurred_at: Utc::now(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub backend_url: String,
    pub allow_loopback_http: bool,
    pub port: Option<String>,
    pub baud: u32,
    pub auto_connect: bool,
    pub minimize_to_tray: bool,
    pub theme: String,
    pub language: String,
    pub station_mapping: [u8; 6],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            backend_url: "https://localhost:8443".into(),
            allow_loopback_http: false,
            port: None,
            baud: 115200,
            auto_connect: false,
            minimize_to_tray: true,
            theme: "dark".into(),
            language: "ru".into(),
            station_mapping: [1, 2, 3, 4, 5, 6],
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        if ![9600, 19200, 38400, 57600, 115200, 230400].contains(&self.baud) {
            return Err(Error::Config("baud"));
        }
        if !["dark", "light"].contains(&self.theme.as_str()) || self.language != "ru" {
            return Err(Error::Config("theme/language"));
        }
        let mut mapping = self.station_mapping;
        mapping.sort();
        if mapping != [1, 2, 3, 4, 5, 6] {
            return Err(Error::Config("station mapping must be permutation 1..6"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolStatistics {
    pub frames: u64,
    pub invalid: u64,
    pub duplicates: u64,
    pub dropped: u64,
    pub button_events: u64,
    pub reconnects: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub at: DateTime<Utc>,
    pub level: String,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    pub state: ConveyorState,
    pub usb_connected: bool,
    pub handshake_complete: bool,
    pub safe: bool,
    pub firmware: Option<String>,
    pub capabilities: Vec<String>,
    pub session_id: Uuid,
    pub position: Option<u8>,
    pub active_command: Option<Uuid>,
    pub fault: Option<String>,
    pub backend_connected: bool,
    pub enrolled: bool,
    pub device_id: Option<Uuid>,
    pub browser_session_id: Option<Uuid>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub paused: bool,
    pub simulation: bool,
    pub statistics: ProtocolStatistics,
    pub buttons: Vec<HardwareEvent>,
    pub logs: Vec<LogEntry>,
    pub history: Vec<CommandExecutionResult>,
    pub settings: Settings,
}
impl Default for GatewayStatus {
    fn default() -> Self {
        Self {
            state: ConveyorState::Disconnected,
            usb_connected: false,
            handshake_complete: false,
            safe: false,
            firmware: None,
            capabilities: vec![],
            session_id: Uuid::new_v4(),
            position: None,
            active_command: None,
            fault: None,
            backend_connected: false,
            enrolled: false,
            device_id: None,
            browser_session_id: None,
            last_heartbeat: None,
            paused: true,
            simulation: false,
            statistics: ProtocolStatistics::default(),
            buttons: vec![],
            logs: vec![],
            history: vec![],
            settings: Settings::default(),
        }
    }
}
