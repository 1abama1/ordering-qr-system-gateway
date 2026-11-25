use crate::domain::*;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Default)]
pub struct HardwareInputService {
    session: Option<Uuid>,
    last_seq: Option<u32>,
    pressed: HashSet<HardwareButton>,
}
impl HardwareInputService {
    pub fn reset(&mut self, session: Uuid) {
        self.session = Some(session);
        self.last_seq = None;
        self.pressed.clear();
    }
    #[allow(clippy::too_many_arguments)]
    pub fn accept(
        &mut self,
        device: Uuid,
        session: Uuid,
        seq: u32,
        button: HardwareButton,
        edge: ButtonEdge,
        now: DateTime<Utc>,
        simulation: bool,
    ) -> Option<HardwareEvent> {
        if self.session != Some(session) || self.last_seq.is_some_and(|last| seq <= last) {
            return None;
        }
        self.last_seq = Some(seq);
        let changed = match edge {
            ButtonEdge::Press => self.pressed.insert(button),
            ButtonEdge::Release => self.pressed.remove(&button),
        };
        if !changed {
            return None;
        }
        Some(HardwareEvent {
            contract: "hardware.v1".into(),
            event_id: Uuid::new_v4(),
            device_id: device,
            session_id: session,
            seq,
            button,
            edge,
            occurred_at: now,
            expires_at: now + Duration::milliseconds(1500),
            browser_session_id: None,
            simulation,
        })
    }
}
