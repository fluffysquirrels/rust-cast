use crate::{
    cast::proxies,
    types::{AppId,
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

pub mod connection {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.tp.connection";

    pub const USER_AGENT: &str = "RustCast";

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectRequest {
        pub user_agent: String,
    }

    impl RequestInner for ConnectRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = "CONNECT";
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectResponse {}

    impl ResponseInner for ConnectResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &["CONNECT"];
    }

    const MESSAGE_TYPE_CLOSE: MessageTypeConst = "CLOSE";
}

pub mod media {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.media";

    const MESSAGE_REQUEST_TYPE_GET_STATUS: MessageTypeConst = "GET_STATUS";
    const MESSAGE_REQUEST_TYPE_LOAD: MessageTypeConst = "LOAD";
    const MESSAGE_REQUEST_TYPE_PLAY: MessageTypeConst = "PLAY";
    const MESSAGE_REQUEST_TYPE_PAUSE: MessageTypeConst = "PAUSE";
    const MESSAGE_REQUEST_TYPE_STOP: MessageTypeConst = "STOP";
    const MESSAGE_REQUEST_TYPE_SEEK: MessageTypeConst = "SEEK";
    const MESSAGE_REQUEST_TYPE_QUEUE_REMOVE: MessageTypeConst = "QUEUE_REMOVE";
    const MESSAGE_REQUEST_TYPE_QUEUE_INSERT: MessageTypeConst = "QUEUE_INSERT";
    const MESSAGE_REQUEST_TYPE_QUEUE_LOAD: MessageTypeConst = "QUEUE_LOAD";
    const MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEMS: MessageTypeConst = "QUEUE_GET_ITEMS";
    const MESSAGE_REQUEST_TYPE_QUEUE_PREV: MessageTypeConst = "QUEUE_PREV";
    const MESSAGE_REQUEST_TYPE_QUEUE_NEXT: MessageTypeConst = "QUEUE_NEXT";

    const MESSAGE_RESPONSE_TYPE_MEDIA_STATUS: MessageTypeConst = "MEDIA_STATUS";
    const MESSAGE_RESPONSE_TYPE_LOAD_CANCELLED: MessageTypeConst = "LOAD_CANCELLED";
    const MESSAGE_RESPONSE_TYPE_LOAD_FAILED: MessageTypeConst = "LOAD_FAILED";
    const MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE: MessageTypeConst = "INVALID_PLAYER_STATE";
    const MESSAGE_RESPONSE_TYPE_INVALID_REQUEST: MessageTypeConst = "INVALID_REQUEST";

    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct MediaStatus {
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
        MediaStatus(MediaStatus),

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
}

pub mod heartbeat {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.tp.heartbeat";

    const MESSAGE_TYPE_PING: MessageTypeConst = "PING";
    const MESSAGE_TYPE_PONG: MessageTypeConst = "PONG";
}

pub mod receiver {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.receiver";

    const MESSAGE_TYPE_RECEIVER_STATUS: &str = "RECEIVER_STATUS";
    const MESSAGE_TYPE_LAUNCH_ERROR: &str = "LAUNCH_ERROR";
    const MESSAGE_TYPE_INVALID_REQUEST: &str = "INVALID_REQUEST";

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct StatusRequest {}

    impl RequestInner for StatusRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = "GET_STATUS";
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct StatusResponse {
        pub status: proxies::receiver::Status,
    }

    impl ResponseInner for StatusResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[MESSAGE_TYPE_RECEIVER_STATUS];
    }


    // TODO: Decide: split operations into modules?

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LaunchRequest {
        pub app_id: AppId,
    }

    impl RequestInner for LaunchRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = "LAUNCH";
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum LaunchResponse {
        #[serde(rename = "RECEIVER_STATUS")]
        OkStatus {
            status: proxies::receiver::Status,
        },

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
        // const TYPE_NAME: MessageTypeConst = "RECEIVER_STATUS";
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_TYPE_INVALID_REQUEST,
            MESSAGE_TYPE_LAUNCH_ERROR,
            MESSAGE_TYPE_RECEIVER_STATUS,
        ];
    }



    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct StopRequest {
        pub session_id: SessionId,
    }

    impl RequestInner for StopRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = "STOP";
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum StopResponse {
        #[serde(rename = "RECEIVER_STATUS")]
        OkStatus {
            status: proxies::receiver::Status,
        },

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest {
            reason: String,
        },
    }

    impl ResponseInner for StopResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_TYPE_RECEIVER_STATUS,
            MESSAGE_TYPE_INVALID_REQUEST,
        ];
    }
}
