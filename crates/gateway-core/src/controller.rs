use crate::{domain::*, protocol::Frame};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Execution {
    pub id: Uuid,
    pub target: u8,
    pub ack_deadline: u64,
    pub move_deadline: u64,
    pub acked: bool,
}
pub struct ConveyorController {
    pub state: ConveyorState,
    pub position: Option<u8>,
    pub safe: bool,
    pub active: Option<Execution>,
    pub inspection: bool,
    pub fault: Option<String>,
    pub stationary_verified: bool,
}
impl Default for ConveyorController {
    fn default() -> Self {
        Self {
            state: ConveyorState::Disconnected,
            position: None,
            safe: false,
            active: None,
            inspection: false,
            fault: None,
            stationary_verified: false,
        }
    }
}
impl ConveyorController {
    pub fn connected(&mut self) {
        self.state = ConveyorState::Initializing;
        self.safe = false;
        self.position = None;
        self.stationary_verified = false;
    }
    pub fn ready(&self) -> bool {
        matches!(self.state, ConveyorState::Ready | ConveyorState::Arrived)
            && self.safe
            && !self.inspection
            && self.active.is_none()
            && self.position.is_some()
    }
    pub fn start(&mut self, id: Uuid, target: u8, now: u64) -> Result<Frame> {
        if !self.ready() || !(1..=6).contains(&target) {
            return Err(Error::Safety("controller is not ready"));
        }
        self.active = Some(Execution {
            id,
            target,
            ack_deadline: now + 2000,
            move_deadline: now + 15000,
            acked: false,
        });
        self.state = ConveyorState::Moving;
        self.safe = false;
        self.position = None;
        self.stationary_verified = false;
        Ok(Frame::MoveTo {
            v: 1,
            command_id: id,
            station: target,
        })
    }
    pub fn uncertain(&mut self, reason: &str) -> Option<CommandExecutionResult> {
        self.inspection = true;
        self.safe = false;
        self.position = None;
        self.stationary_verified = false;
        self.state = ConveyorState::NeedsInspection;
        self.fault = Some(reason.into());
        self.active
            .take()
            .map(|a| CommandExecutionResult::new(a.id, "NEEDS_INSPECTION"))
    }
    pub fn disconnected(&mut self) -> Option<CommandExecutionResult> {
        if self.active.is_some() {
            return self.uncertain("USB_LOST_DURING_MOTION");
        }
        self.safe = false;
        self.position = None;
        self.state = if self.inspection {
            ConveyorState::NeedsInspection
        } else {
            ConveyorState::Disconnected
        };
        None
    }
    pub fn frame(&mut self, frame: &Frame) -> Option<CommandExecutionResult> {
        match frame {
            Frame::Ack { command_id, .. } => {
                if let Some(a) = &mut self.active {
                    if a.id == *command_id && !a.acked {
                        a.acked = true;
                        return Some(CommandExecutionResult::new(a.id, "ACKNOWLEDGED"));
                    }
                }
            }
            Frame::Arrived {
                command_id,
                station,
                ..
            } => {
                if let Some(a) = &self.active {
                    if a.id == *command_id && a.target == *station {
                        self.active = None;
                        self.position = Some(*station);
                        self.safe = true;
                        self.state = ConveyorState::Arrived;
                        return Some(CommandExecutionResult::new(*command_id, "ARRIVED"));
                    }
                    return self.uncertain("ARRIVAL_MISMATCH");
                }
            }
            Frame::Fault { code, .. } => {
                let result = self.uncertain(code);
                self.state = if code == "E_STOP" {
                    ConveyorState::EStop
                } else {
                    ConveyorState::Fault
                };
                return result;
            }
            Frame::State {
                state,
                station,
                safe,
                command_id,
                ..
            } => {
                if let Some(a) = &self.active {
                    if *state == ConveyorState::Moving && *command_id == Some(a.id) {
                        return None;
                    }
                    // STATUS alone cannot complete a movement: only sensor ARRIVED can.
                    return self.uncertain("STATUS_DURING_UNRESOLVED_MOTION");
                }
                self.position = *station;
                self.safe = *safe;
                if matches!(state, ConveyorState::Ready | ConveyorState::Arrived)
                    && (!safe || station.is_none())
                {
                    return self.uncertain("POSITION_OR_SAFETY_UNVERIFIED");
                }
                if matches!(state, ConveyorState::Fault | ConveyorState::EStop) {
                    self.inspection = true;
                }
                self.stationary_verified = *safe
                    && station.is_some()
                    && matches!(state, ConveyorState::Ready | ConveyorState::Arrived);
                self.state = if matches!(state, ConveyorState::EStop | ConveyorState::Fault) {
                    *state
                } else if self.inspection {
                    ConveyorState::NeedsInspection
                } else {
                    *state
                };
                if *state == ConveyorState::Moving {
                    return self.uncertain("UNOWNED_MOTION");
                }
            }
            _ => {}
        }
        None
    }
    pub fn tick(&mut self, now: u64) -> Option<CommandExecutionResult> {
        if let Some(a) = &self.active {
            if !a.acked && now >= a.ack_deadline {
                return self.uncertain("ACK_TIMEOUT");
            }
            if now >= a.move_deadline {
                return self.uncertain("POSITION_TIMEOUT");
            }
        }
        None
    }
    pub fn clear_inspection(&mut self) -> Result<()> {
        if self.active.is_some()
            || !self.safe
            || !self.stationary_verified
            || self.position.is_none()
            || matches!(self.state, ConveyorState::EStop | ConveyorState::Moving)
        {
            return Err(Error::Safety(
                "fresh stationary safe sensor STATUS required",
            ));
        }
        self.inspection = false;
        self.fault = None;
        self.state = ConveyorState::Ready;
        Ok(())
    }
}
