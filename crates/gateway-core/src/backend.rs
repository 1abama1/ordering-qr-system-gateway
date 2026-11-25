use crate::domain::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, Method, Url};
use secrecy::{ExposeSecret, SecretString};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;

pub struct DeviceCredentials {
    pub device_id: Uuid,
    pub secret: SecretString,
    pub origin: String,
}
#[derive(Serialize, Deserialize)]
struct CredentialBlob {
    device_id: Uuid,
    secret: String,
    origin: String,
}
pub trait CredentialStore: Send + Sync {
    fn load(&self, origin: &str) -> Result<Option<DeviceCredentials>>;
    fn save(&self, credentials: &DeviceCredentials) -> Result<()>;
}
pub struct OsCredentialStore;
impl CredentialStore for OsCredentialStore {
    fn load(&self, origin: &str) -> Result<Option<DeviceCredentials>> {
        let entry = keyring::Entry::new("com.dastor.hardware-gateway", "device")
            .map_err(|_| Error::Credentials)?;
        match entry.get_password() {
            Ok(text) => {
                let blob: CredentialBlob =
                    serde_json::from_str(&text).map_err(|_| Error::Credentials)?;
                if blob.origin
                    != Url::parse(origin)
                        .map_err(|_| Error::Config("backend origin"))?
                        .origin()
                        .ascii_serialization()
                {
                    return Err(Error::Config("credentials belong to another backend"));
                }
                Ok(Some(DeviceCredentials {
                    device_id: blob.device_id,
                    secret: blob.secret.into(),
                    origin: blob.origin,
                }))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(Error::Credentials),
        }
    }
    fn save(&self, c: &DeviceCredentials) -> Result<()> {
        let blob = CredentialBlob {
            device_id: c.device_id,
            secret: c.secret.expose_secret().into(),
            origin: c.origin.clone(),
        };
        let text = serde_json::to_string(&blob).map_err(|_| Error::Credentials)?;
        keyring::Entry::new("com.dastor.hardware-gateway", "device")
            .map_err(|_| Error::Credentials)?
            .set_password(&text)
            .map_err(|_| Error::Credentials)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Enrollment {
    contract: String,
    device_id: Uuid,
    device_secret: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessToken {
    pub contract: String,
    pub access_token: SecretString,
    pub expires_at: DateTime<Utc>,
    pub scopes: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceConfig {
    pub contract: String,
    pub device_id: Uuid,
    pub station_mapping: [u8; 6],
    pub browser_session_id: Option<Uuid>,
    pub browser_session_expires_at: Option<DateTime<Utc>>,
}
impl DeviceConfig {
    pub fn validate(&self, device: Uuid) -> Result<()> {
        let mut mapping = self.station_mapping;
        mapping.sort();
        if self.contract != "hardware.v1"
            || self.device_id != device
            || mapping != [1, 2, 3, 4, 5, 6]
        {
            return Err(Error::Protocol("invalid device config"));
        }
        Ok(())
    }
}

#[async_trait]
pub trait BackendTransport: Send + Sync {
    async fn next_command(&self, token: &SecretString) -> Result<Option<RemoteCommand>>;
    async fn ack(&self, token: &SecretString, command: &RemoteCommand) -> Result<()>;
    async fn publish(&self, token: &SecretString, event: &CommandExecutionResult) -> Result<()>;
    async fn button(&self, token: &SecretString, event: &HardwareEvent) -> Result<()>;
}
#[derive(Clone)]
pub struct BackendApiClient {
    client: Client,
    base: Url,
}
impl BackendApiClient {
    pub fn new(origin: &str, allow_loopback_http: bool) -> Result<Self> {
        let mut base = Url::parse(origin).map_err(|_| Error::Config("backend URL"))?;
        let loopback = matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if (base.scheme() != "https"
            && !(allow_loopback_http && base.scheme() == "http" && loopback))
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(Error::Config(
                "HTTPS origin required (no path, credentials, query or fragment)",
            ));
        }
        base.set_path("/");
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(25))
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Self { client, base })
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        token: Option<&SecretString>,
        body: Option<serde_json::Value>,
    ) -> Result<Option<Vec<u8>>> {
        let url = self
            .base
            .join(path)
            .map_err(|_| Error::Config("API path"))?;
        let mut request = self
            .client
            .request(method, url)
            .header("X-Hardware-Contract", "hardware.v1");
        if let Some(token) = token {
            request = request.bearer_auth(token.expose_secret());
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request.send().await.map_err(|_| Error::Network)?;
        if !response.status().is_success() {
            return Err(Error::Http(response.status().as_u16()));
        }
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        let mut bytes = vec![];
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Network)? {
            if bytes.len() + chunk.len() > 65536 {
                return Err(Error::Protocol("backend response too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Some(bytes))
    }
    fn decode<T: DeserializeOwned>(bytes: Option<Vec<u8>>) -> Result<T> {
        serde_json::from_slice(&bytes.ok_or(Error::Protocol("empty backend response"))?)
            .map_err(|_| Error::Protocol("invalid backend contract"))
    }
    pub async fn enroll(&self, code: &str) -> Result<DeviceCredentials> {
        if code.len() < 6
            || code.len() > 64
            || !code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(Error::Config("pairing code format"));
        }
        let e: Enrollment = Self::decode(
            self.request(
                Method::POST,
                "api/device/enroll",
                None,
                Some(serde_json::json!({"contract":"hardware.v1","pairingCode":code})),
            )
            .await?,
        )?;
        if e.contract != "hardware.v1"
            || e.device_id.is_nil()
            || e.device_secret.len() < 16
            || e.device_secret.len() > 4096
        {
            return Err(Error::Protocol("enrollment contract"));
        }
        Ok(DeviceCredentials {
            device_id: e.device_id,
            secret: e.device_secret.into(),
            origin: self.base.origin().ascii_serialization(),
        })
    }
    pub async fn authenticate(&self, c: &DeviceCredentials) -> Result<AccessToken> {
        let t: AccessToken = Self::decode(self.request(Method::POST,"api/device/auth/token",None,
            Some(serde_json::json!({"contract":"hardware.v1","deviceId":c.device_id,"deviceSecret":c.secret.expose_secret()}))).await?)?;
        if t.contract != "hardware.v1"
            || !t.scopes.iter().any(|s| s == "hardware.v1")
            || t.expires_at <= Utc::now() + chrono::Duration::seconds(5)
            || t.access_token.expose_secret().is_empty()
        {
            return Err(Error::Protocol("token scope or expiry"));
        }
        Ok(t)
    }
    pub async fn config(&self, t: &SecretString, device: Uuid) -> Result<DeviceConfig> {
        let c: DeviceConfig = Self::decode(
            self.request(Method::GET, "api/device/config", Some(t), None)
                .await?,
        )?;
        c.validate(device)?;
        Ok(c)
    }
    pub async fn heartbeat(&self, t: &SecretString, s: &GatewayStatus) -> Result<()> {
        if s.simulation {
            return Err(Error::Safety(
                "simulator heartbeat cannot be sent to the device API",
            ));
        }
        let identified = s.usb_connected && s.handshake_complete;
        let firmware = s.firmware.as_deref().filter(|_| identified);
        let capabilities: &[String] = if identified { &s.capabilities } else { &[] };
        let status = serde_json::json!({
            "contract": "hardware.v1",
            "deviceId": s.device_id,
            "sessionId": s.session_id,
            "state": s.state,
            "paused": s.paused,
            "simulation": false,
            "serialConnected": s.usb_connected,
            "arduinoDetected": identified,
            "firmwareVersion": firmware,
            "protocolVersion": identified.then_some(1),
            "safe": identified && s.safe,
            "currentPosition": s.position.filter(|_| identified),
            "activeCommandId": s.active_command.filter(|_| identified),
            "buttonControllerOnline": capabilities.iter().any(|cap| cap == "BUTTONS"),
            "faultCode": s.fault,
            "capabilities": capabilities,
            "supportedActions": ["MOVE_TO"],
        });
        self.request(Method::POST, "api/device/heartbeat", Some(t), Some(status))
            .await?;
        Ok(())
    }
    pub async fn telemetry(&self, t: &SecretString, s: &GatewayStatus) -> Result<()> {
        self.request(Method::POST,"api/device/telemetry",Some(t),Some(serde_json::json!({"contract":"hardware.v1","deviceId":s.device_id,"state":s.state,"simulation":false}))).await?;
        Ok(())
    }
}
#[async_trait]
impl BackendTransport for BackendApiClient {
    async fn next_command(&self, token: &SecretString) -> Result<Option<RemoteCommand>> {
        match self
            .request(
                Method::GET,
                "api/device/commands/next?waitSeconds=20",
                Some(token),
                None,
            )
            .await?
        {
            None => Ok(None),
            Some(bytes) => Ok(Some(Self::decode(Some(bytes))?)),
        }
    }
    async fn ack(&self, token: &SecretString, command: &RemoteCommand) -> Result<()> {
        self.request(Method::POST,&format!("api/device/commands/{}/ack",command.command_id),Some(token),
            Some(serde_json::json!({"contract":"hardware.v1","leaseToken":command.lease_token,"version":command.version}))).await?;
        Ok(())
    }
    async fn publish(&self, token: &SecretString, event: &CommandExecutionResult) -> Result<()> {
        self.request(
            Method::POST,
            &format!("api/device/commands/{}/events", event.command_id),
            Some(token),
            Some(serde_json::to_value(event).map_err(|_| Error::Protocol("event"))?),
        )
        .await?;
        Ok(())
    }
    async fn button(&self, token: &SecretString, event: &HardwareEvent) -> Result<()> {
        if event.simulation || event.expires_at <= Utc::now() || event.browser_session_id.is_none()
        {
            return Err(Error::Safety("button expired or unbound"));
        }
        self.request(
            Method::POST,
            "api/device/buttons/events",
            Some(token),
            Some(serde_json::to_value(event).map_err(|_| Error::Protocol("button"))?),
        )
        .await?;
        Ok(())
    }
}
