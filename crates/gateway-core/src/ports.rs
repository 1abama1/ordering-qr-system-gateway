use crate::domain::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait SerialTransport: Send {
    /// Cancellation safe: read bytes, framing is owned by the supervisor.
    async fn read(&mut self, bytes: &mut [u8]) -> Result<usize>;
    async fn write(&mut self, bytes: &[u8]) -> Result<()>;
}
#[async_trait]
pub trait SerialConnector: Send + Sync {
    async fn open(&self, port: &str, baud: u32) -> Result<Box<dyn SerialTransport>>;
}
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub trait Persistence: Send {
    fn receive(&mut self, id: CommandId, simulation: bool) -> Result<bool>;
    fn record(&mut self, event: &CommandExecutionResult, simulation: bool) -> Result<()>;
    fn recover(&mut self) -> Result<Vec<CommandExecutionResult>>;
}
