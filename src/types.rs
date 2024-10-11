// TODO: Move these to `src/payload.rs`

use crate::{
    message::EndpointId,
    payload::{
        media::MediaSessionId,
        receiver::AppSessionId,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppSession {
    pub app_destination_id: EndpointId,
    pub receiver_destination_id: EndpointId,

    pub app_session_id: AppSessionId,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MediaSession {
    #[serde(flatten)]
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
