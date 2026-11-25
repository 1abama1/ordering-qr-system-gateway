use crate::{
    domain::*,
    ports::{SerialConnector, SerialTransport},
};
use async_trait::async_trait;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::{SerialPortBuilderExt, SerialPortType};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPort {
    pub name: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}
pub fn discover() -> Result<Vec<DiscoveredPort>> {
    tokio_serial::available_ports()
        .map_err(|_| Error::Unavailable)
        .map(|ports| {
            ports
                .into_iter()
                .map(|p| {
                    let mut d = DiscoveredPort {
                        name: p.port_name,
                        vid: None,
                        pid: None,
                        serial_number: None,
                        manufacturer: None,
                        product: None,
                    };
                    if let SerialPortType::UsbPort(info) = p.port_type {
                        d.vid = Some(info.vid);
                        d.pid = Some(info.pid);
                        d.serial_number = info.serial_number;
                        d.manufacturer = info.manufacturer;
                        d.product = info.product;
                    }
                    d
                })
                .collect()
        })
}
pub struct UsbSerialTransport(pub tokio_serial::SerialStream);
pub struct UsbSerialConnector;
#[async_trait]
impl SerialConnector for UsbSerialConnector {
    async fn open(&self, port: &str, baud: u32) -> Result<Box<dyn SerialTransport>> {
        Ok(Box::new(UsbSerialTransport::open(port, baud).await?))
    }
}
impl UsbSerialTransport {
    pub async fn open(port: &str, baud: u32) -> Result<Self> {
        // Only discovered names: no arbitrary Windows device paths from IPC.
        if !discover()?.iter().any(|p| p.name == port) {
            return Err(Error::Config("port not discovered"));
        }
        let stream = tokio_serial::new(port, baud)
            .open_native_async()
            .map_err(|_| Error::Unavailable)?;
        #[cfg(unix)]
        let mut stream = stream;
        #[cfg(unix)]
        stream.set_exclusive(true).map_err(|_| Error::Unavailable)?;
        // Uno resets on DTR. Motor remains disabled throughout boot; bounded wait before HELLO.
        tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
        Ok(Self(stream))
    }
}
#[async_trait]
impl SerialTransport for UsbSerialTransport {
    async fn read(&mut self, bytes: &mut [u8]) -> Result<usize> {
        Ok(self.0.read(bytes).await?)
    }
    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.write_all(bytes).await?;
        Ok(())
    }
}
