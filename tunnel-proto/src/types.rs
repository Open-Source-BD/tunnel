use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u8 = 1;

pub const MAX_FRAME_PAYLOAD: u32 = 16 * 1024 * 1024; // 16MB

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Register = 0x01,
    Registered = 0x02,
    HttpRequest = 0x03,
    HttpResponse = 0x04,
    TcpData = 0x05,
    Error = 0x06,
    CloseStream = 0x07,
    Heartbeat = 0x08,
}

impl TryFrom<u8> for MessageType {
    type Error = ProtocolError;

    fn try_from(v: u8) -> Result<Self> {
        match v {
            0x01 => Ok(Self::Register),
            0x02 => Ok(Self::Registered),
            0x03 => Ok(Self::HttpRequest),
            0x04 => Ok(Self::HttpResponse),
            0x05 => Ok(Self::TcpData),
            0x06 => Ok(Self::Error),
            0x07 => Ok(Self::CloseStream),
            0x08 => Ok(Self::Heartbeat),
            _ => Err(ProtocolError::UnknownMessageType(v)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub stream_id: u32,
    pub msg_type: MessageType,
    pub payload: bytes::Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterPayload {
    pub subdomain: String,
    pub local_port: u16,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredPayload {
    pub assigned_url: String,
    pub tunnel_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosePayload {
    pub stream_id: u32,
    pub reason: Option<String>,
}

pub type Result<T> = std::result::Result<T, ProtocolError>;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("unknown message type: {0}")]
    UnknownMessageType(u8),

    #[error("version mismatch: expected {expected}, got {got}")]
    VersionMismatch { expected: u8, got: u8 },

    #[error("payload too large: {0} bytes (max {})", MAX_FRAME_PAYLOAD)]
    PayloadTooLarge(u32),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
