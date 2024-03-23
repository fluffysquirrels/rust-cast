// TODO: Move these to `src/payload.rs`

pub type Namespace = String;
pub type NamespaceConst = &'static str;

pub type MessageType = String;
pub type MessageTypeConst = &'static str;

pub type AppId = String;
pub type AppIdConst = &'static str;

pub type EndpointId = String;
pub type EndpointIdConst = &'static str;

pub const ENDPOINT_BROADCAST: EndpointIdConst = "*";

pub type SessionId = String;

/// Unique ID for the playback of an item in this app session.
/// This ID is set by the receiver at LOAD.
pub type MediaSessionId = i32;

#[derive(Clone, Debug)]
pub struct AppSession {
    pub receiver_destination_id: EndpointId,
    pub app_destination_id: EndpointId,
    pub session_id: SessionId,
}

#[derive(Clone, Debug)]
pub struct MediaSession {
    pub app_session: AppSession,
    pub media_session_id: MediaSessionId,
}
