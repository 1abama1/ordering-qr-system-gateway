use crate::domain::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_FRAME: usize = 256;

pub fn supported_firmware(fw: &str) -> bool {
    let parts: Vec<_> = fw.split('.').collect();
    parts.len() == 3
        && parts[0] == "1"
        && parts[1] == "0"
        && !parts[2].is_empty()
        && parts[2].bytes().all(|b| b.is_ascii_digit())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", deny_unknown_fields)]
pub enum Frame {
    #[serde(rename = "HELLO")]
    Hello { v: u8, nonce: String },
    #[serde(rename = "HELLO_ACK")]
    HelloAck {
        v: u8,
        nonce: String,
        fw: String,
        cap: Vec<String>,
    },
    #[serde(rename = "BTN")]
    Button {
        v: u8,
        seq: u32,
        b: HardwareButton,
        edge: ButtonEdge,
    },
    #[serde(rename = "STATUS")]
    Status { v: u8 },
    #[serde(rename = "STATE")]
    State {
        v: u8,
        state: ConveyorState,
        station: Option<u8>,
        safe: bool,
        #[serde(rename = "commandId")]
        command_id: Option<Uuid>,
    },
    #[serde(rename = "MOVE_TO")]
    MoveTo {
        v: u8,
        #[serde(rename = "commandId")]
        command_id: Uuid,
        station: u8,
    },
    #[serde(rename = "ACK")]
    Ack {
        v: u8,
        #[serde(rename = "commandId")]
        command_id: Uuid,
    },
    #[serde(rename = "MOVING")]
    Moving {
        v: u8,
        #[serde(rename = "commandId")]
        command_id: Uuid,
    },
    #[serde(rename = "ARRIVED")]
    Arrived {
        v: u8,
        #[serde(rename = "commandId")]
        command_id: Uuid,
        station: u8,
    },
    #[serde(rename = "FAULT")]
    Fault { v: u8, code: String },
    #[serde(rename = "STOP")]
    Stop { v: u8 },
    #[serde(rename = "RESET_FAULT")]
    ResetFault { v: u8 },
    #[serde(rename = "PING")]
    Ping { v: u8 },
    #[serde(rename = "PONG")]
    Pong { v: u8 },
}
impl Frame {
    pub fn validate(&self) -> Result<()> {
        let v = match self {
            Self::Hello { v, .. }
            | Self::HelloAck { v, .. }
            | Self::Button { v, .. }
            | Self::Status { v }
            | Self::State { v, .. }
            | Self::MoveTo { v, .. }
            | Self::Ack { v, .. }
            | Self::Moving { v, .. }
            | Self::Arrived { v, .. }
            | Self::Fault { v, .. }
            | Self::Stop { v }
            | Self::ResetFault { v }
            | Self::Ping { v }
            | Self::Pong { v } => *v,
        };
        if v != 1 {
            return Err(Error::Protocol("unsupported version"));
        }
        match self {
            Self::Hello { nonce, .. } | Self::HelloAck { nonce, .. }
                if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) =>
            {
                return Err(Error::Protocol("invalid nonce"))
            }
            Self::MoveTo { station, .. } | Self::Arrived { station, .. }
                if !(1..=6).contains(station) =>
            {
                return Err(Error::Protocol("station out of range"))
            }
            Self::State {
                station: Some(station),
                ..
            } if !(1..=6).contains(station) => return Err(Error::Protocol("station out of range")),
            Self::MoveTo { command_id, .. }
            | Self::Arrived { command_id, .. }
            | Self::Ack { command_id, .. }
            | Self::Moving { command_id, .. }
                if command_id.is_nil() =>
            {
                return Err(Error::Protocol("nil command id"))
            }
            Self::Fault { code, .. }
                if code.len() > 48
                    || code.is_empty()
                    || !code
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b == b'_' || b.is_ascii_digit()) =>
            {
                return Err(Error::Protocol("invalid fault"))
            }
            Self::HelloAck { fw, cap, .. }
                if fw.len() > 16 || cap.len() > 4 || cap.iter().any(|c| c.len() > 20) =>
            {
                return Err(Error::Protocol("invalid firmware metadata"))
            }
            _ => {}
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut bytes = serde_json::to_vec(self).map_err(|_| Error::Protocol("encode"))?;
        if bytes.len() > MAX_FRAME {
            return Err(Error::Protocol("frame too large"));
        }
        bytes.push(b'\n');
        Ok(bytes)
    }
}

#[derive(Default)]
pub struct SerialProtocolCodec {
    buffer: Vec<u8>,
    discarding: bool,
}
impl SerialProtocolCodec {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Result<Frame>> {
        let mut output = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                if self.discarding {
                    self.discarding = false;
                    self.buffer.clear();
                    continue;
                }
                if self.buffer.last() == Some(&b'\r') {
                    self.buffer.pop();
                }
                if !self.buffer.is_empty() {
                    output.push(parse(&self.buffer));
                }
                self.buffer.clear();
            } else if !self.discarding {
                if self.buffer.len() == MAX_FRAME {
                    self.discarding = true;
                    self.buffer.clear();
                    output.push(Err(Error::Protocol("frame too large")));
                } else {
                    self.buffer.push(byte);
                }
            }
        }
        output
    }
}
pub fn parse(bytes: &[u8]) -> Result<Frame> {
    if bytes.len() > MAX_FRAME {
        return Err(Error::Protocol("frame too large"));
    }
    // UUID serde accepts several representations; wire protocol only accepts canonical UUIDs.
    let raw: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| Error::Protocol("invalid JSON"))?;
    if let Some(id) = raw.get("commandId").and_then(|v| v.as_str()) {
        let parsed = Uuid::parse_str(id).map_err(|_| Error::Protocol("invalid command id"))?;
        if parsed.to_string() != id {
            return Err(Error::Protocol("noncanonical command id"));
        }
    }
    let frame: Frame =
        serde_json::from_slice(bytes).map_err(|_| Error::Protocol("invalid fields"))?;
    frame.validate()?;
    Ok(frame)
}
