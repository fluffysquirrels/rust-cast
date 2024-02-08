pub type Namespace = String;
pub type NamespaceConst = &'static str;

pub type MessageType = String;
pub type MessageTypeConst = &'static str;

pub type RequestId = i32;

pub type AppId = String;
pub type AppIdConst = &'static str;

pub type EndpointId = String;
pub type EndpointIdConst = &'static str;

pub const ENDPOINT_BROADCAST: EndpointIdConst = "*";

pub type SessionId = String;
pub type MediaSessionId = i32;

// #[derive(Clone, Debug)]
// pub struct MediaSession {
//     pub destination_id: EndpointId,
//     pub session_id: SessionId,
//     pub media_session_id: MediaSessionId,
// }

#[derive(Clone, Debug)]
pub struct AppSession {
    pub receiver_destination_id: EndpointId,
    pub app_destination_id: EndpointId,
    pub session_id: SessionId,
}
