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

pub type AppSessionId = String;

/// Unique ID for the playback of an item in this app session.
/// This ID is set by the receiver at LOAD.
pub type MediaSessionId = i32;

#[derive(Clone, Debug)]
pub struct AppSession {
    pub app_destination_id: EndpointId,
    pub receiver_destination_id: EndpointId,

    pub app_session_id: AppSessionId,
}

#[derive(Clone, Debug)]
pub struct MediaSession {
    pub app_session: AppSession,
    pub media_session_id: MediaSessionId,
}

impl MediaSession {
    pub fn app_destination_id(&self) -> &EndpointId {
        &self.app_session.app_destination_id
    }

    pub fn receiver_destination_id(&self) -> &EndpointId {
        &self.app_session.receiver_destination_id
    }

    pub fn app_session_id(&self) -> &AppSessionId {
        &self.app_session.app_session_id
    }
}
