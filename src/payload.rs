use crate::{
    cast::proxies,
    types::{AppId,
            MediaSessionId,
            MessageType, MessageTypeConst,
            /* Namespace, */ NamespaceConst,
            RequestId, SessionId},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payload<T>
{
    pub request_id: Option<RequestId>,

    #[serde(rename = "type")]
    pub typ: MessageType,

    #[serde(flatten)]
    pub inner: T,
}

pub type PayloadDyn = Payload<serde_json::Value>;

pub trait RequestInner: Debug + Serialize
{
    const CHANNEL_NAMESPACE: NamespaceConst;
    const TYPE_NAME: MessageTypeConst;
}

pub trait ResponseInner: Debug + DeserializeOwned
{
    const CHANNEL_NAMESPACE: NamespaceConst;
    const TYPE_NAMES: &'static [MessageTypeConst];
}

pub const USER_AGENT: &str = "RustCast; https://github.com/azasypkin/rust-cast";

pub mod connection {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.tp.connection";

    pub const MESSAGE_TYPE_CONNECT: MessageTypeConst = "CONNECT";
    pub const MESSAGE_TYPE_CLOSE: MessageTypeConst = "CLOSE";

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectRequest {
        pub user_agent: String,
    }

    impl RequestInner for ConnectRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_TYPE_CONNECT;
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectResponse {}

    impl ResponseInner for ConnectResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[MESSAGE_TYPE_CONNECT];
    }
}

pub mod heartbeat {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.tp.heartbeat";

    pub const MESSAGE_TYPE_PING: MessageTypeConst = "PING";
    pub const MESSAGE_TYPE_PONG: MessageTypeConst = "PONG";

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Ping {}

    impl RequestInner for Ping {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_TYPE_PING;
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Pong {}

    impl RequestInner for Pong {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_TYPE_PONG;
    }
}

pub mod media {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.media";

    pub const MESSAGE_REQUEST_TYPE_GET_STATUS: MessageTypeConst = "GET_STATUS";
    pub const MESSAGE_REQUEST_TYPE_LOAD: MessageTypeConst = "LOAD";
    pub const MESSAGE_REQUEST_TYPE_PLAY: MessageTypeConst = "PLAY";
    pub const MESSAGE_REQUEST_TYPE_PAUSE: MessageTypeConst = "PAUSE";
    pub const MESSAGE_REQUEST_TYPE_STOP: MessageTypeConst = "STOP";
    pub const MESSAGE_REQUEST_TYPE_SEEK: MessageTypeConst = "SEEK";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_REMOVE: MessageTypeConst = "QUEUE_REMOVE";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_INSERT: MessageTypeConst = "QUEUE_INSERT";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_LOAD: MessageTypeConst = "QUEUE_LOAD";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEMS: MessageTypeConst = "QUEUE_GET_ITEMS";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_PREV: MessageTypeConst = "QUEUE_PREV";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_NEXT: MessageTypeConst = "QUEUE_NEXT";

    pub const MESSAGE_RESPONSE_TYPE_MEDIA_STATUS: MessageTypeConst = "MEDIA_STATUS";
    pub const MESSAGE_RESPONSE_TYPE_LOAD_CANCELLED: MessageTypeConst = "LOAD_CANCELLED";
    pub const MESSAGE_RESPONSE_TYPE_LOAD_FAILED: MessageTypeConst = "LOAD_FAILED";
    pub const MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE: MessageTypeConst
        = "INVALID_PLAYER_STATE";
    pub const MESSAGE_RESPONSE_TYPE_INVALID_REQUEST: MessageTypeConst = "INVALID_REQUEST";

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Status {
        pub status: Vec<proxies::media::Status>,
    }




    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoadRequest {
        pub session_id: SessionId,

        pub media: proxies::media::Media,
        pub current_time: f64,

        #[serde(default)]
        pub custom_data: serde_json::Value,
        pub autoplay: bool,
        pub preload_time: f64,
    }

    impl RequestInner for LoadRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_LOAD;
    }


    #[derive(Debug, Deserialize, Serialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum LoadResponse {
        #[serde(rename = "MEDIA_STATUS")]
        Ok(Status),

        #[serde(rename = "LOAD_CANCELLED")]
        LoadCancelled,

        #[serde(rename = "LOAD_FAILED")]
        LoadFailed,

        #[serde(rename = "INVALID_PLAYER_STATE")]
        InvalidPlayerState,

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest { reason: String },
    }

    impl ResponseInner for LoadResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_MEDIA_STATUS,
            MESSAGE_RESPONSE_TYPE_LOAD_CANCELLED,
            MESSAGE_RESPONSE_TYPE_LOAD_FAILED,
            MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }



    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetStatusRequest {
        pub media_session_id: Option<MediaSessionId>,
    }

    impl RequestInner for GetStatusRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_GET_STATUS;
    }

    #[derive(Debug, Deserialize, Serialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum GetStatusResponse {
        #[serde(rename = "MEDIA_STATUS")]
        Ok(Status),

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest { reason: String },
    }

    impl ResponseInner for GetStatusResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_MEDIA_STATUS,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }
}

pub mod receiver {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.receiver";

    pub const MESSAGE_REQUEST_TYPE_LAUNCH: &str = "LAUNCH";
    pub const MESSAGE_REQUEST_TYPE_STOP: &str = "STOP";
    pub const MESSAGE_REQUEST_TYPE_GET_STATUS: &str = "GET_STATUS";
    pub const MESSAGE_REQUEST_TYPE_SET_VOLUME: &str = "SET_VOLUME";

    pub const MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS: &str = "RECEIVER_STATUS";
    pub const MESSAGE_RESPONSE_TYPE_LAUNCH_ERROR: &str = "LAUNCH_ERROR";
    pub const MESSAGE_RESPONSE_TYPE_INVALID_REQUEST: &str = "INVALID_REQUEST";

    #[derive(Clone, Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Status {
        pub status: proxies::receiver::Status,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetStatusRequest {}

    impl RequestInner for GetStatusRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_GET_STATUS;
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetStatusResponse(pub Status);

    impl ResponseInner for GetStatusResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS];
    }


    // TODO: Decide: split operations into modules?

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LaunchRequest {
        pub app_id: AppId,
    }

    impl RequestInner for LaunchRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_LAUNCH;
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum LaunchResponse {
        #[serde(rename = "RECEIVER_STATUS")]
        Ok(Status),

        #[serde(rename = "LAUNCH_ERROR")]
        Error {
            reason: String,
        },

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest {
            reason: String,
        },
    }

    impl ResponseInner for LaunchResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
            MESSAGE_RESPONSE_TYPE_LAUNCH_ERROR,
            MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS,
        ];
    }



    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct StopRequest {
        pub session_id: SessionId,
    }

    impl RequestInner for StopRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_STOP;
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum StopResponse {
        #[serde(rename = "RECEIVER_STATUS")]
        Ok(Status),

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest {
            reason: String,
        },
    }

    impl ResponseInner for StopResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }



    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SetVolumeRequest {
        pub volume: proxies::receiver::Volume,
    }

    impl RequestInner for SetVolumeRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_SET_VOLUME;
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum SetVolumeResponse {
        #[serde(rename = "RECEIVER_STATUS")]
        Ok(Status),

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest {
            reason: String,
        },
    }

    impl ResponseInner for SetVolumeResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }
}
