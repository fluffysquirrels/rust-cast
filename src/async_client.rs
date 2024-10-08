use anyhow::{bail, format_err};
use bytes::{Buf, BufMut, BytesMut};
use chrono::{DateTime, Utc};
use crate::{
    message::{
        CastMessage,
        CastMessagePayload,
    },
    payload::{self, Payload, PayloadDyn, RequestId, RequestIdGen, RequestInner, ResponseInner,
              media::{CustomData, MediaRequestCommon}},
    types::{AppId, /* AppIdConst, */
            AppSession, /* AppSessionId, */
            EndpointId, EndpointIdConst, ENDPOINT_BROADCAST,
            MediaSession, /* MediaSessionId, */
            MediaSessionId,
            /* MessageType, */ MessageTypeConst,
            /* Namespace, */ NamespaceConst },
    util::{named},
};
use futures::{
    future::Either,
    SinkExt, Stream, StreamExt,
    stream::{SplitSink, SplitStream},
};
use once_cell::sync::Lazy;
use pin_project_lite::pin_project;
use protobuf::Message;
use serde::Serialize;
use std::{
    any::{self, Any},
    collections::{HashMap, HashSet},
    fmt::{self, Debug},
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Arc, atomic::{AtomicUsize, Ordering}},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    pin,
    sync::{broadcast, mpsc, watch},
};
use tokio_util::{
    codec::{self, Framed},
    time::delay_queue::{DelayQueue, Expired as DelayExpired, Key as DelayKey},
};

pub use anyhow::Error;
pub type Result<T, E = Error> = std::result::Result<T, E>;


pub struct Client {
    /// Some(_) until `.close()` is called.
    task_join_handle: Option<tokio::task::JoinHandle<Result<()>>>,

    task_cmd_tx: mpsc::Sender::<TaskCmd>,

    request_id_gen: RequestIdGen,
    next_command_id: AtomicUsize,

    shared: Arc<Shared>,

    conn_state_rx: watch::Receiver<ConnectionState>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub addr: SocketAddr,

    /// `EndpointId` used as the sender, and source of messages we send.
    pub sender: EndpointId,

    /// Duration for the Task to do something locally.
    pub local_task_command_timeout: Duration,

    /// Duration for an RPC request to and response from the Chromecast.
    pub rpc_timeout: Duration,

    /// Duration for a media loading RPC request to and response from the Chromecast.
    pub load_timeout: Duration,

    /// After this duration without receiving a message,
    /// disconnect assuming the connection has failed.
    pub heartbeat_disconnect_timeout: Duration,

    /// After this duration without receiving a message,
    /// send a ping message to the receiver.
    pub heartbeat_ping_send_timeout: Duration,
}

/// Data shared between `Client` and its `Task`.
struct Shared {
    config: Config,
    status_tx: broadcast::Sender<StatusUpdate>,
}

// struct Task
pin_project! {
    struct Task<S: TokioAsyncStream> {
        #[pin]
        conn_framed_sink: SplitSink<Framed<S, CastMessageCodec>, CastMessage>,

        #[pin]
        conn_framed_stream: SplitStream<Framed<S, CastMessageCodec>>,

        #[pin]
        task_cmd_rx: tokio_stream::wrappers::ReceiverStream<TaskCmd>,

        #[pin]
        timeout_queue: DelayQueue<RequestId>,

        #[pin]
        heartbeat_disconnect_timeout: tokio::time::Sleep,

        #[pin]
        heartbeat_ping_send_timeout: tokio::time::Sleep,

        flush: Option<FlushState>,

        closed: bool,

        requests_map: HashMap<RequestId, RequestState>,

        conn_state_tx: watch::Sender<ConnectionState>,

        shared: Arc<Shared>,
    }
}

#[derive(Debug)]
struct RequestState {
    response_ns: NamespaceConst,
    response_type_names: &'static [MessageTypeConst],
    timeout_key: DelayKey,

    #[allow(dead_code)] // Just for debugging for now.
    deadline: tokio::time::Instant,

    result_sender: TaskCmdResultSender,
}

#[derive(Debug)]
struct FlushState {
    deadline: tokio::time::Instant,
}

#[derive(Debug)]
struct TaskCmdResultSender {
    command_id: CommandId,
    result_tx: tokio::sync::oneshot::Sender::<TaskCmdResult>,
}

#[derive(Debug)]
struct TaskCmd {
    command: TaskCmdType,
    result_sender: TaskCmdResultSender,
}

#[derive(Debug)]
enum TaskCmdType {
    CastRpc(Box<CastRpc>),
    CastSend(Box<CastSend>),
    Shutdown,
}

#[derive(Debug)]
struct CastRpc {
    request_message: CastMessage,
    request_id: RequestId,
    response_ns: NamespaceConst,
    response_type_names: &'static [MessageTypeConst],
}

#[derive(Debug)]
struct CastSend {
    request_message: CastMessage,
    request_id: RequestId,
}

#[derive(Debug)]
struct TaskResponseBox {
    type_name: &'static str,
    value: Box<dyn Any + Send>,
}

type TaskCmdResult = Result<TaskResponseBox>;

pub trait TokioAsyncStream: AsyncRead + AsyncWrite + Unpin {}

impl<T> TokioAsyncStream for T
where T: AsyncRead + AsyncWrite + Unpin
{}

type CommandId = usize;

struct CastMessageCodec;

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct StatusUpdate {
    pub time: DateTime<Utc>,
    pub msg: StatusMessage,
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum StatusMessage {
    /// Connection closed by request.
    ClosedOk,

    /// Connection closed due to an error.
    ClosedErr,

    // TODO: Implement these?
    // HeartbeatPingSent,
    // HeartbeatPongSent,

    Media(MediaStatusMessage),
    Receiver(ReceiverStatusMessage),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Connected,

    /// Connection closed by request.
    ClosedOk,

    /// Connection closed due to an error.
    ClosedErr,
}

#[cfg(any())] // Disabled for now, may come back to it.
#[derive(Clone, Debug)]
pub struct ErrorStatus {
    // TODO: Implement this.

    // pub io_error_kind: Option<std::io::ErrorKind>,

    // pub connected: bool,
}

#[derive(Clone, Debug)]
pub struct ReceiverStatusMessage {
    pub receiver_id: AppId,
    pub receiver_status: payload::receiver::Status,
}

#[derive(Clone, Debug)]
pub struct MediaStatusMessage {
    pub app_destination_id: AppId,
    pub media_status: payload::media::Status,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReceiverStatuses {
    pub receiver_id: EndpointId,
    pub receiver_status: payload::receiver::Status,
    pub media_statuses: Vec<(MediaSession, payload::media::StatusEntry)>,
}

const DATA_BUFFER_LEN: usize = 64 * 1024;

const TASK_CMD_CHANNEL_CAPACITY: usize = 16;

const STATUS_BROADCAST_CHANNEL_CAPACITY: usize = 16;

const TASK_DELAY_QUEUE_CAPACITY: usize = 4;

static JSON_NAMESPACES: Lazy<HashSet<NamespaceConst>> = Lazy::<HashSet<NamespaceConst>>::new(|| {
    HashSet::from([
        payload::connection::CHANNEL_NAMESPACE,
        payload::heartbeat::CHANNEL_NAMESPACE,
        payload::media::CHANNEL_NAMESPACE,
        payload::receiver::CHANNEL_NAMESPACE,
    ])
});

pub const DEFAULT_SENDER_ID: EndpointIdConst = "sender-0";
pub const DEFAULT_RECEIVER_ID: EndpointIdConst = "receiver-0";

/// Well known cast receiver app IDs
// TODO: Move to payload::receiver.
pub mod app {
    use crate::types::AppIdConst;

    pub const DEFAULT_MEDIA_RECEIVER: AppIdConst = "CC1AD845";
    pub const BACKDROP: AppIdConst = "E8C28D3C";
    pub const NETFLIX: AppIdConst = "CA5E8412";
    pub const SPOTIFY: AppIdConst = "CC32E753";
    pub const YOUTUBE: AppIdConst = "233637DE";
}

pub const DEFAULT_PORT: u16 = 8009;

impl Config {
    pub fn from_addr(addr: SocketAddr) -> Config {
        Config {
            addr,

            sender: DEFAULT_SENDER_ID.to_string(),
            local_task_command_timeout: Duration::from_secs(1),
            rpc_timeout: Duration::from_secs(5),
            load_timeout: Duration::from_secs(10),
            heartbeat_disconnect_timeout: Duration::from_secs(10),
            heartbeat_ping_send_timeout: Duration::from_secs(4),
        }
    }

    #[tracing::instrument(level = "info", skip(self), err)]
    pub async fn connect(self) -> Result<Client> {
        let conn = tls_connect(&self).await?;

        let (task_cmd_tx, task_cmd_rx) = mpsc::channel(TASK_CMD_CHANNEL_CAPACITY);
        let (conn_state_tx, conn_state_rx) = watch::channel(ConnectionState::Connected);

        let shared = Arc::new(Shared {
            config: self,
            status_tx: broadcast::Sender::new(STATUS_BROADCAST_CHANNEL_CAPACITY),
        });

        let task = Task::new(conn, task_cmd_rx, conn_state_tx, Arc::clone(&shared));

        let task_join_handle = Some(tokio::spawn(task.main()));

        let mut client = Client {
            task_join_handle,
            task_cmd_tx,
            conn_state_rx,

            request_id_gen: RequestIdGen::new(),
            next_command_id: AtomicUsize::new(1),

            shared,
        };

        client.init().await?;

        Ok(client)
    }
}

// Public methods
impl Client {
    #[cfg(any())] // Not implemented yet.
    pub async fn reconnect(&mut self) -> Result<()> {
        todo!(
            "Use a new private variant of Config::connect that re-uses existing status_tx\n\
             then mem::replace(self, new)");
    }

    /// NB: If a media app is running, this will connect to it and hence the
    /// client will receive media status updates.
    pub async fn get_statuses(&mut self, receiver_id: EndpointId)
    -> Result<ReceiverStatuses>
    {
        let receiver_status = self.receiver_status(receiver_id.clone()).await?;

        let mut media_statuses = Vec::<(MediaSession, payload::media::StatusEntry)>
                                    ::with_capacity(1);

        for media_app in receiver_status.applications.iter()
                             .filter(|app| app.has_namespace(payload::media::CHANNEL_NAMESPACE))
        {
            let app_session = media_app.to_app_session(receiver_id.clone())?;

            self.connection_connect(app_session.app_destination_id.clone()).await?;
            let media_status = self.media_status(app_session.clone(),
                                                 /* media_session_id: */ None).await?;

            for media_status_entry in media_status.entries.into_iter() {
                let media_session = MediaSession {
                    app_session: app_session.clone(),
                    media_session_id: media_status_entry.media_session_id,
                };

                media_statuses.push((media_session, media_status_entry));
            }
        }

        Ok(ReceiverStatuses {
            receiver_id,
            receiver_status,
            media_statuses,
        })
    }

    pub async fn receiver_status(&mut self, receiver_id: EndpointId)
    -> Result<payload::receiver::Status>
    {
        let payload_req = payload::receiver::GetStatusRequest {};

        let resp: Payload<payload::receiver::GetStatusResponse>
            = self.json_rpc(payload_req, receiver_id,
                            self.config().rpc_timeout).await?;

        Ok(resp.inner.0.status)
    }

    #[named]
    pub async fn receiver_launch_app(&mut self, destination_id: EndpointId, app_id: AppId)
    -> Result<(payload::receiver::Application, payload::receiver::Status)>
    {
        const METHOD_PATH: &str = method_path!("Client");

        let payload_req = payload::receiver::LaunchRequest {
            app_id: app_id.clone(),
        };

        let resp: Payload<payload::receiver::LaunchResponse>
            = self.json_rpc(payload_req, destination_id,
                            self.config().load_timeout).await?;

        let payload::receiver::LaunchResponse::Ok(payload::receiver::StatusWrapper { status })
            = resp.inner else
        {
            bail!("{METHOD_PATH}: error response:\n\
                   Launch response: {resp:#?}");
        };

        let Some(app) = status.applications.iter().find(|app| &app.app_id == &app_id) else {
            bail!("{METHOD_PATH}: missing expected application\n\
                   Receiver status: {status:#?}");
        };

        tracing::debug!(target: METHOD_PATH,
                        ?app,
                        ?status,
                        "Launched app");

        Ok((app.clone(), status))
    }

    #[named]
    pub async fn receiver_stop_app(&mut self, app_session: AppSession)
    -> Result<payload::receiver::Status>
    {
        use payload::receiver::{StatusWrapper, StopRequest, StopResponse};

        const METHOD_PATH: &str = method_path!("Client");

        let payload_req = StopRequest {
            app_session_id: app_session.app_session_id,
        };

        let resp: Payload<StopResponse>
            = self.json_rpc(payload_req, app_session.receiver_destination_id,
                            self.config().load_timeout).await?;

        let StopResponse::Ok(StatusWrapper { status }) = resp.inner else {
            bail!("{METHOD_PATH}: error response\n\
                   response: {resp:#?}");
        };

        Ok(status)
    }

    #[named]
    pub async fn receiver_set_volume(&mut self,
                                     destination_id: EndpointId,
                                     volume: payload::receiver::Volume)
    -> Result<payload::receiver::Status>
    {
        use payload::receiver::{SetVolumeRequest, SetVolumeResponse, StatusWrapper};

        const METHOD_PATH: &str = method_path!("Client");

        let payload_req = SetVolumeRequest {
            volume,
        };

        let resp: Payload<SetVolumeResponse>
            = self.json_rpc(payload_req, destination_id,
                            self.config().rpc_timeout).await?;

        let SetVolumeResponse::Ok(StatusWrapper { status }) = resp.inner else {
            bail!("{METHOD_PATH}: error response\n\
                   response: {resp:#?}");
        };

        Ok(status)
    }

    #[named]
    pub async fn media_get_media_session(&mut self, destination_id: EndpointId)
    -> Result<MediaSession>
    {
        const METHOD_PATH: &str = method_path!("Client");

        let receiver_status = self.receiver_status(destination_id.clone()).await?;

        let Some(media_app)
            = receiver_status.applications.iter()
                  .find(|app| app.has_namespace(payload::media::CHANNEL_NAMESPACE)) else
        {
            bail!("{METHOD_PATH}: No app with media namespace found.\n\
                   _ receiver_status = {receiver_status:#?}");
        };

        let app_session = media_app.to_app_session(destination_id.to_string())?;
        self.connection_connect(app_session.app_destination_id.clone()).await?;

        let media_status = self.media_status(app_session.clone(),
                                             /* media_session_id: */ None).await?;

        Ok(MediaSession {
            app_session,
            media_session_id: media_status.try_find_media_session_id()?,
        })
    }

    #[named]
    pub async fn media_get_default_media_session(&mut self, destination_id: EndpointId)
    -> Result<MediaSession>
    {
        const METHOD_PATH: &str = method_path!("Client");

        let receiver_status = self.receiver_status(destination_id.clone()).await?;

        let Some(media_app)
            = receiver_status.applications.iter()
                  .find(|app| app.app_id == app::DEFAULT_MEDIA_RECEIVER) else
        {
            bail!("{METHOD_PATH}: No default media app found.\n\
                   _ receiver_status = {receiver_status:#?}");
        };

        let app_session = media_app.to_app_session(destination_id.to_string())?;
        self.connection_connect(app_session.app_destination_id.clone()).await?;

        let media_status = self.media_status(app_session.clone(),
                                             /* media_session_id: */ None).await?;

        Ok(MediaSession {
            app_session,
            media_session_id: media_status.try_find_media_session_id()?,
        })
    }

    pub async fn media_get_or_launch_default_app_session(&mut self, destination_id: EndpointId)
    -> Result<AppSession>
    {
        let receiver_status = self.receiver_status(destination_id.clone()).await?;

        let app_session = match receiver_status.applications.iter()
                                  .find(|app| app.app_id == app::DEFAULT_MEDIA_RECEIVER)
        {
            Some(app) => {
                let app_session = app.to_app_session(destination_id)?;
                self.connection_connect(app_session.app_destination_id.clone()).await?;
                app_session
            },
            None => {
                let (app_session, _receiver_status) =
                    self.media_launch_default(destination_id.clone()).await?;
                app_session
            },
        };

        Ok(app_session)
    }

    pub async fn media_get_or_launch_default_media_session(&mut self, destination_id: EndpointId)
    -> Result<MediaSession>
    {
        let app_session = self.media_get_or_launch_default_app_session(destination_id).await?;

        let media_status = self.media_status(app_session.clone(),
                                             /* media_session_id: */ None).await?;

        Ok(MediaSession {
            app_session,
            media_session_id: media_status.try_find_media_session_id()?,
        })
    }

    pub async fn media_launch_default(&mut self, destination_id: EndpointId)
    -> Result<(AppSession, payload::receiver::Status)> {
        let (app, status) = self.receiver_launch_app(destination_id.clone(),
                                                     app::DEFAULT_MEDIA_RECEIVER.into()).await?;
        let app_session = app.to_app_session(destination_id)?;
        self.connection_connect(app_session.app_destination_id.clone()).await?;

        Ok((app_session, status))
    }

    #[named]
    pub async fn media_status(&mut self,
                              app_session: AppSession,
                              media_session_id: Option<MediaSessionId>)
    -> Result<payload::media::Status> {
        let payload_req = payload::media::GetStatusRequest {
            media_session_id,
        };

        let resp: Payload<payload::media::GetStatusResponse>
            = self.json_rpc(payload_req, app_session.app_destination_id,
                            self.config().rpc_timeout).await?;

        let payload::media::GetStatusResponse::Ok(media_status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(media_status)
    }

    // TODO: Decide whether to do this.
    //       Would run connection_connect, include listening to media status.
    // pub async fn media_connect(&mut self, destination: EndpointId) -> Result<()> {
    //     todo!()
    // }


    #[named]
    pub async fn media_edit_tracks_info(&mut self,
                                        media_session: MediaSession,
                                        args: payload::media::EditTracksInfoRequestArgs)
    -> Result<payload::media::Status>
    {
        let payload_req = payload::media::EditTracksInfoRequest {
            args,
            media_session_id: media_session.media_session_id,
        };
        let resp: Payload<payload::media::GetStatusResponse>
            = self.json_rpc(payload_req, media_session.app_destination_id().clone(),
                            self.config().rpc_timeout).await?;

        let payload::media::GetStatusResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    #[named]
    pub async fn media_load(&mut self,
                            app_session: AppSession,
                            load_args: payload::media::LoadRequestArgs)
    -> Result<payload::media::Status> {
        let payload_req = payload::media::LoadRequest {
            app_session_id: app_session.app_session_id,

            args: load_args,
        };

        let resp: Payload<payload::media::LoadResponse>
            = self.json_rpc(payload_req, app_session.app_destination_id.clone(),
                            self.config().load_timeout).await?;

        let payload::media::LoadResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    pub async fn media_play(&mut self,
                            media_session: MediaSession)
    -> Result<payload::media::Status> {
        self.simple_media_request(media_session,
                                  payload::media::PlayRequest,
                                  self.config().rpc_timeout
        ).await
    }

    pub async fn media_pause(&mut self,
                             media_session: MediaSession)
    -> Result<payload::media::Status> {
        self.simple_media_request(media_session,
                                  payload::media::PauseRequest,
                                  self.config().rpc_timeout
        ).await
    }

    #[named]
    pub async fn media_queue_get_item_ids(&mut self,
                                          media_session: MediaSession)
    -> Result<Vec<payload::media::ItemId>> {
        let payload_req = payload::media::QueueGetItemIdsRequest(
            MediaRequestCommon {
                custom_data: CustomData::default(),
                media_session_id: media_session.media_session_id,
            });

        let resp: Payload<payload::media::QueueGetItemIdsResponse>
            = self.json_rpc(payload_req, media_session.app_destination_id().clone(),
                            self.config().rpc_timeout).await?;

        let payload::media::QueueGetItemIdsResponse::Ok { item_ids } = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(item_ids)
    }

    pub async fn media_queue_get_items(&mut self,
                                       media_session: MediaSession,
                                       args: payload::media::QueueGetItemsRequestArgs)
    -> Result<Vec<payload::media::QueueItem>> {
        let payload_req = payload::media::QueueGetItemsRequest {
            args,
            media_session_id: media_session.media_session_id,
        };

        let resp: Payload<payload::media::QueueGetItemsResponse>
            = self.json_rpc(payload_req, media_session.app_destination_id().clone(),
                            self.config().rpc_timeout).await?;

        Ok(resp.inner.items)
    }

    #[named]
    pub async fn media_queue_insert(&mut self,
                                  media_session: MediaSession,
                                  args: payload::media::QueueInsertRequestArgs)
    -> Result<payload::media::Status> {
        let payload_req = payload::media::QueueInsertRequest {
            media_session_id: media_session.media_session_id,
            app_session_id: media_session.app_session_id().clone(),
            args,
        };

        let resp: Payload<payload::media::LoadResponse>
            = self.json_rpc(payload_req, media_session.app_destination_id().clone(),
                            self.config().load_timeout).await?;

        let payload::media::LoadResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    #[named]
    pub async fn media_queue_load(&mut self,
                                  app_session: AppSession,
                                  args: payload::media::QueueLoadRequestArgs)
    -> Result<payload::media::Status> {
        let payload_req = payload::media::QueueLoadRequest {
            app_session_id: app_session.app_session_id.clone(),
            args,
        };

        let resp: Payload<payload::media::LoadResponse>
            = self.json_rpc(payload_req, app_session.app_destination_id.clone(),
                            self.config().load_timeout).await?;

        let payload::media::LoadResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    #[named]
    pub async fn media_queue_remove(&mut self,
                                  media_session: MediaSession,
                                  args: payload::media::QueueRemoveRequestArgs)
    -> Result<payload::media::Status> {
        let payload_req = payload::media::QueueRemoveRequest {
            media_session_id: media_session.media_session_id,
            args,
        };

        let resp: Payload<payload::media::GetStatusResponse>
            = self.json_rpc(payload_req, media_session.app_destination_id().clone(),
                            self.config().load_timeout).await?;

        let payload::media::GetStatusResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    #[named]
    pub async fn media_queue_reorder(&mut self,
                                  media_session: MediaSession,
                                  args: payload::media::QueueReorderRequestArgs)
    -> Result<payload::media::Status> {
        let payload_req = payload::media::QueueReorderRequest {
            media_session_id: media_session.media_session_id,
            args,
        };

        let resp: Payload<payload::media::GetStatusResponse>
            = self.json_rpc(payload_req, media_session.app_destination_id().clone(),
                            self.config().load_timeout).await?;

        let payload::media::GetStatusResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    #[named]
    pub async fn media_queue_update(&mut self,
                                  media_session: MediaSession,
                                  args: payload::media::QueueUpdateRequestArgs)
    -> Result<payload::media::Status> {
        let payload_req = payload::media::QueueUpdateRequest {
            media_session_id: media_session.media_session_id,
            args,
        };

        let resp: Payload<payload::media::LoadResponse>
            = self.json_rpc(payload_req, media_session.app_destination_id().clone(),
                            self.config().load_timeout).await?;

        let payload::media::LoadResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    pub async fn media_stop(&mut self,
                            media_session: MediaSession)
    -> Result<payload::media::Status> {
        self.simple_media_request(media_session,
                                  payload::media::StopRequest,
                                  self.config().rpc_timeout
        ).await
    }

    #[named]
    pub async fn media_seek(&mut self,
                            media_session: MediaSession,
                            seek_args: payload::media::SeekRequestArgs)
    -> Result<payload::media::Status>
    {
        let payload_req = payload::media::SeekRequest {
            media_session_id: media_session.media_session_id,
            args: seek_args,
        };

        let resp: Payload<payload::media::GetStatusResponse>
            = self.json_rpc(payload_req,
                            media_session.app_destination_id().clone(),
                            self.config().load_timeout).await?;

        let payload::media::GetStatusResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response from seek request\n\
                   _ response         = {resp:#?}\n\
                   _ media_session    = {media_session:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    #[named]
    pub async fn media_set_playback_rate(&mut self,
                                         media_session: MediaSession,
                                         args: payload::media::SetPlaybackRateRequestArgs)
    -> Result<payload::media::Status>
    {
        let payload_req = payload::media::SetPlaybackRateRequest {
            media_session_id: media_session.media_session_id,
            args,
        };

        let resp: Payload<payload::media::GetStatusResponse>
            = self.json_rpc(payload_req,
                            media_session.app_destination_id().clone(),
                            self.config().rpc_timeout).await?;

        let payload::media::GetStatusResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response from seek request\n\
                   _ response         = {resp:#?}\n\
                   _ media_session    = {media_session:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    #[named]
    async fn simple_media_request<Req>(
        &mut self,
        media_session: MediaSession,
        msg_type_fn: fn(MediaRequestCommon) -> Req,
        duration: Duration)
    -> Result<payload::media::Status>
    where Req: payload::RequestInner
    {
        let payload_req = msg_type_fn(MediaRequestCommon {
            custom_data: CustomData::default(),
            media_session_id: media_session.media_session_id,
        });

        let resp: Payload<payload::media::GetStatusResponse>
            = self.json_rpc(payload_req,
                            media_session.app_destination_id().clone(),
                            duration).await?;

        let payload::media::GetStatusResponse::Ok(status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response         = {resp:#?}\n\
                   _ request_msg_type = {req_msg_type}\n\
                   _ media_session    = {media_session:#?}",
                  method_path = method_path!("Client"),
                  req_msg_type = Req::TYPE_NAME);
        };

        Ok(status)
    }

    #[named]
    pub fn listen_status(&self) -> impl Stream<Item = StatusUpdate> + Send {
        // TODO: Set up auto connect for the media app?
        tokio_stream::wrappers::BroadcastStream::new(self.shared.status_tx.subscribe())
            .filter_map(|res| futures::future::ready(match res {
                Ok(it) => Some(it),
                Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                    tracing::warn!(target: method_path!("Client"),
                                   n,
                                   "lagged");
                    None
                },
            }))
    }

    pub fn listen_connection_state(&self) -> impl Stream<Item = ConnectionState> + Send {
        tokio_stream::wrappers::WatchStream::new(self.conn_state_rx.clone())
    }

    pub fn connection_state(&mut self) -> ConnectionState {
        self.conn_state_rx.borrow_and_update().clone()
    }

    /// Close in an orderly way. The returned Future must be polled to completion.
    #[named]
    pub async fn close(mut self) -> Result<()> {
        let connection_state = self.connection_state();
        tracing::trace!(target: method_path!("Client"),
                        ?connection_state,
                        "connection_state");

        // Ensure task has stopped.
        match connection_state {
            ConnectionState::Connected => {
                let _: Box<()> = self.task_cmd::<()>(TaskCmdType::Shutdown,
                                                     self.config().local_task_command_timeout
                                 ).await?;
            },

            // Task should already have stopped itself.
            //
            // Don't send a shutdown command: it's not required, and sending the command
            // would fail because the receiving end has probably closed.
            ConnectionState::ClosedOk
            | ConnectionState::ClosedErr => (),
        };

        // Should be no need to handle when task_join_handle is None.
        // If so, `close()` has already been called and Client should have been consumed.
        let join_fut = self.task_join_handle.take()
                           .expect("task_join_handle is Some(_) until .close().");

        tokio::time::timeout(self.config().local_task_command_timeout, join_fut).await???;

        Ok(())
    }

    /// Spawn a task to close the Client in an orderly way.
    ///
    /// Errors will be logged with `tracing`.
    #[named]
    pub fn spawn_close(self) {
        let _handle = tokio::spawn(async move {
            if let Err(err) = self.close().await {
                tracing::error!(target: method_path!("Client"),
                                err = format!("{err:#?}"),
                                "Err closing Client");
            }
        });
    }
}

/// Internals.
impl Client {
    async fn init(&mut self) -> Result<()> {
        self.connection_connect(DEFAULT_RECEIVER_ID.to_string()).await?;

        Ok(())
    }

    pub fn config(&self) -> &Config {
        &self.shared.config
    }

    // TODO: Make this private? Used by current CLI to listen to status updates.
    //       Could provide safer, higher level API for this use case.
    pub async fn connection_connect(&mut self, destination: EndpointId) -> Result<()> {
        let payload_req = payload::connection::ConnectRequest {
            user_agent: payload::USER_AGENT.to_string(),
        };
        self.json_send(payload_req, destination).await?;
        Ok(())
    }

    fn response_from_dyn<Resp>(&self, payload_dyn: Box<PayloadDyn>)
    -> Result<Payload<Resp>>
    where Resp: ResponseInner
    {
        let namespace = Resp::CHANNEL_NAMESPACE;
        let expected_types = Resp::TYPE_NAMES;

        // TODO: Why did I disable this?
        // Was it to try JSON deserialisation anyway to get better / different diagnostics?
        if false && !expected_types.contains(&payload_dyn.typ.as_str()) {
            bail!("Unexpected type in response payload\n\
                   request_id:     {rid:?}\n\
                   namespace:      {namespace:?}\n\
                   expected_types: {expected_types:?}\n\
                   type:           {typ:?}",
                  rid = payload_dyn.request_id,
                  typ = payload_dyn.typ);
        }

        Ok(Payload::<Resp> {
            request_id: payload_dyn.request_id,
            typ: payload_dyn.typ,
            inner: serde_json::from_value(payload_dyn.inner)?,
        })
    }

    async fn json_send<Req>(&mut self, req: Req, destination: EndpointId)
    -> Result<()>
    where Req: RequestInner
    {
        let (request_message, request_id) = self.cast_request_from_inner(req, destination)?;

        let cmd_type = TaskCmdType::CastSend(Box::new(CastSend {
            request_message,
            request_id,
        }));

        let _resp: Box<()> = self.task_cmd(cmd_type, self.config().rpc_timeout).await?;

        Ok(())
    }

    #[named]
    async fn json_rpc<Req, Resp>(&mut self,
                                 req: Req, destination: EndpointId, timeout: Duration)
    -> Result<Payload<Resp>>
    where Req: RequestInner,
          Resp: ResponseInner
    {
        let start = tokio::time::Instant::now();

        let (request_message, request_id) = self.cast_request_from_inner(req, destination)?;

        let response_ns = Resp::CHANNEL_NAMESPACE;
        let response_type_names = Resp::TYPE_NAMES;

        let cmd_type = TaskCmdType::CastRpc(Box::new(CastRpc {
            request_message,
            request_id,
            response_ns,
            response_type_names,
        }));

        let resp_dyn: Box<PayloadDyn> = self.task_cmd(cmd_type, timeout).await?;
        let resp: Payload<Resp> = self.response_from_dyn(resp_dyn)?;

        let elapsed = start.elapsed();

        tracing::debug!(target: method_path!("Client"),
                        ?start,
                        ?elapsed,
                        response_payload = ?resp,
                        response_ns,
                        response_type_name = resp.typ,
                        expected_response_type_names = ?response_type_names,
                        request_id = request_id.inner(),
                        "json_rpc response");

        Ok(resp)
    }

    #[named]
    fn cast_request_from_inner<Req>(&self, req: Req, destination: EndpointId)
    -> Result<(CastMessage, RequestId)>
    where Req: RequestInner
    {
        const METHOD_PATH: &str = method_path!("Client");

        let request_id = self.take_request_id();
        let payload = Payload::<Req> {
            request_id: Some(request_id),
            typ: Req::TYPE_NAME.to_string(),
            inner: req,
        };

        let sender = self.config().sender.clone();
        let request_namespace = Req::CHANNEL_NAMESPACE.to_string();

        tracing::debug!(target: METHOD_PATH,
                        ?payload,
                        request_id = request_id.inner(),
                        request_type = payload.typ,
                        request_namespace,
                        sender, destination,
                        "payload struct");

        let payload_json = serde_json::to_string(&payload)?;

        tracing::trace!(target: METHOD_PATH,
                        payload_json,
                        request_id = request_id.inner(),
                        request_type = payload.typ,
                        request_namespace,
                        sender, destination,
                        "payload json");

        let request_message = CastMessage {
            namespace: request_namespace,
            source: sender,
            destination,
            payload: payload_json.into(),
        };

        Ok((request_message, request_id))
    }

    async fn task_cmd<R>(&mut self, cmd_type: TaskCmdType, timeout: Duration)
    -> Result<Box<R>>
    where R: Any + Send + Sync
    {
        let command_id = self.take_command_id();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel::<TaskCmdResult>();

        let cmd = TaskCmd {
            command: cmd_type,
            result_sender: TaskCmdResultSender {
                command_id,
                result_tx,
            },
        };

        self.task_cmd_tx.send_timeout(
            cmd,
            self.config().local_task_command_timeout
        ).await?;

        let response: TaskResponseBox =
            tokio::time::timeout(timeout, result_rx).await???;

        response.downcast::<R>()
    }

    fn take_request_id(&self) -> RequestId {
        self.request_id_gen.take_next()
    }

    fn take_command_id(&self) -> CommandId {
        self.next_command_id.fetch_add(1, Ordering::SeqCst)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if self.task_join_handle.is_some() {
            // TODO: Call `self.spawn_close()` instead.

            tracing::error!("Client: task not stopped before drop.\n\
                             Use Client::close to dispose of Client.");
        }
    }
}

impl Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Client")
         .field("config", &self.shared.config)
         .field("task", if self.task_join_handle.is_some() { &"Some" } else { &"None" })
         .finish_non_exhaustive()
    }
}

#[tracing::instrument(level = "debug",
                      fields(ip = ?config.addr.ip(),
                             port = config.addr.port()))]
#[named]
async fn tls_connect(config: &Config)
-> Result<impl TokioAsyncStream>
{
    const FUNCTION_PATH: &str = function_path!();

    let deadline = tokio::time::Instant::now() + config.rpc_timeout;

    let addr = &config.addr;
    let ip: IpAddr = addr.ip();

    let mut tls_config = rustls::ClientConfig::builder()
        .dangerous().with_custom_certificate_verifier(Arc::new(
            crate::util::rustls::danger::NoCertificateVerification::new_ring()))
        .with_no_client_auth();

    tls_config.enable_early_data = true;
    let tls_config = Arc::new(tls_config);

    let connector = tokio_rustls::TlsConnector::from(tls_config);

    let ip_rustls = rustls::pki_types::IpAddr::from(ip);
    let domain = rustls::pki_types::ServerName::IpAddress(ip_rustls);

    let tcp_stream = tokio::time::timeout_at(
        deadline,
        tokio::net::TcpStream::connect(addr)).await??;

    tracing::debug!(target: FUNCTION_PATH,
                    "TcpStream connected");

    let tls_stream = tokio::time::timeout_at(
        deadline,
        connector.connect(domain, tcp_stream)).await??;

    tracing::debug!(target: FUNCTION_PATH,
                    "TlsStream connected");

    Ok(tls_stream)
}

#[derive(Debug)]
enum TaskEvent {
    Cmd(TaskCmd),
    Flush(Result<Result<()>, tokio::time::error::Elapsed>),
    HeartbeatDisconnectTimeout,
    HeartbeatPingSendTimeout,
    MessageRead(Result<CastMessage>),
    RpcTimeout(DelayExpired<RequestId>),
}

impl<S: TokioAsyncStream> Task<S> {
    pub fn new(
        conn: S,
        task_cmd_rx: mpsc::Receiver::<TaskCmd>,
        conn_state_tx: watch::Sender::<ConnectionState>,
        shared: Arc<Shared>,
    ) -> Task<S> {
        let task_cmd_rx = tokio_stream::wrappers::ReceiverStream::new(task_cmd_rx);

        let timeout_queue = DelayQueue::<RequestId>::with_capacity(TASK_DELAY_QUEUE_CAPACITY);

        let cast_message_codec = CastMessageCodec;
        let conn_framed = tokio_util::codec::Framed::with_capacity(
            conn, cast_message_codec, DATA_BUFFER_LEN);

        let (conn_framed_sink, conn_framed_stream) = conn_framed.split();

        Task {
            conn_framed_sink,
            conn_framed_stream,

            task_cmd_rx,
            timeout_queue,
            heartbeat_disconnect_timeout: tokio::time::sleep(
                shared.config.heartbeat_disconnect_timeout),
            heartbeat_ping_send_timeout: tokio::time::sleep(
                shared.config.heartbeat_ping_send_timeout),
            flush: None,
            closed: false,
            requests_map: HashMap::new(),

            conn_state_tx,

            shared,
        }
    }

    #[named]
    async fn main(self) -> Result<()> {
        const METHOD_PATH: &str = method_path!("Task");

        pin! {
            let this = self;
        }

        while let Some(event) = this.as_mut().take_next_event().await {
            tracing::trace!(target: METHOD_PATH,
                            ?event,
                            "event");

            match event {
                TaskEvent::Cmd(cmd) => match cmd.command {
                    TaskCmdType::CastRpc(rpc) => {
                        this.as_mut().handle_rpc_cmd(rpc, cmd.result_sender).await;
                    },

                    TaskCmdType::CastSend(send) => {
                        this.as_mut().handle_send(send, cmd.result_sender).await;
                    },

                    TaskCmdType::Shutdown => {
                        tracing::info!(target: METHOD_PATH,
                                       "shutdown on command");
                        Self::respond_generic(cmd.result_sender, Ok(()));
                        this.publish_closed(ConnectionState::ClosedOk);
                        return Ok(());
                    },
                },

                TaskEvent::MessageRead(read_res) => {
                    this.as_mut().handle_msg_read(read_res).await;
                },

                TaskEvent::Flush(res) => {
                    match res {
                        Ok(Ok(())) => { *this.as_mut().project().flush = None; },
                        err => {
                            // Error sending a message. Shut down the connection and return.

                            // TODO: Just warn on errors that leave us possibly connected?
                            // TODO: Possibly add optional auto reconnect?

                            tracing::warn!(target: METHOD_PATH,
                                           ?err,
                                           "flush error");

                            this.publish_closed(ConnectionState::ClosedErr);
                            return Ok(());
                        }
                    }
                },

                TaskEvent::RpcTimeout(expired) => {
                    this.as_mut().handle_rpc_timeout(expired);
                },

                TaskEvent::HeartbeatDisconnectTimeout => {
                    let disconnect_timeout = this.config().heartbeat_disconnect_timeout;

                    tracing::warn!(
                        target: METHOD_PATH,
                        ?disconnect_timeout,
                        "heartbeat message not received for too long: disconnecting");

                    this.publish_closed(ConnectionState::ClosedErr);
                    return Ok(());
                },

                TaskEvent::HeartbeatPingSendTimeout => {
                    this.as_mut().handle_heartbeat_ping_send_timeout().await;
                },
            }
        }

        tracing::info!(target: METHOD_PATH,
                       "shutdown on event stream closed");
        this.publish_closed(ConnectionState::ClosedOk);

        // TODO: cleanup? e.g.
        //   * flush outputs
        //   * reset connections (they will be dropped, do we need this?)
        //   * return errors to response channels
        //     (once response channel senders are dropped this will happen anyway)

        Ok(())
    }

    #[named]
    async fn take_next_event(self: Pin<&mut Self>) -> Option<TaskEvent> {
        let mut proj = self.project();

        if *proj.closed {
            tracing::info!(target: method_path!("Task"),
                           closed = true,
                           "closed = true from previous loop");
            return None;
        }

        let conn_flush_stream /* : impl Stream<Item = Result<Result<()>, Elapsed>>*/
            = if let Some(flush) = proj.flush {
                // Looking at the source code, the returned `Flush` future holds no state,
                // it just calls Sink::poll_flush().
                // So if the flush doesn't complete in this method call, Future is dropped,
                // but no data should be lost.
                let fut = proj.conn_framed_sink.flush();

                let with_timeout = tokio::time::timeout_at(flush.deadline, fut);
                let stream = futures::stream::once(with_timeout);
                Either::Left(stream)
            } else {
                Either::Right(futures::stream::empty())
            };

        pin!(conn_flush_stream);

        pin! {
            let heartbeat_disconnect_timeout =
                futures::stream::once(&mut proj.heartbeat_disconnect_timeout);
            let heartbeat_ping_send_timeout =
                futures::stream::once(&mut proj.heartbeat_ping_send_timeout);
        };

        // Streams polled in order with current implementation on first
        // poll of Merge.
        //
        // This method cannot consume the `Stream`s in fields (most of which are pinned),
        // so we construct a merged stream from mut references to the underlying streams.
        //
        // By assigning to a variable, these temporaries have their
        // lifetime extended so `merge()` can use them.
        let streams = (
            &mut (conn_flush_stream.map(TaskEvent::Flush)),
            &mut (proj.task_cmd_rx.map(TaskEvent::Cmd)),
            &mut (proj.conn_framed_stream.map(TaskEvent::MessageRead)),
            &mut (proj.timeout_queue.map(TaskEvent::RpcTimeout)),
            &mut (heartbeat_disconnect_timeout.map(
                |_| TaskEvent::HeartbeatDisconnectTimeout)),
            &mut (heartbeat_ping_send_timeout.map(
                |_| TaskEvent::HeartbeatPingSendTimeout)),
        );

        let mut merged = futures_concurrency::stream::Merge::merge(streams);

        merged.next().await
    }

    #[named]
    async fn handle_send(mut self: Pin<&mut Self>,
                         send: Box<CastSend>, result_sender: TaskCmdResultSender)
    {
        const METHOD_PATH: &str = method_path!("Task");

        let deadline = tokio::time::Instant::now()
            .checked_add(self.config().rpc_timeout)
            .unwrap_or_else(|| panic!("{METHOD_PATH}: error calculating deadline"));

        let send_debug = format!("{send:#?}");

        let CastSend {
            request_message,
            request_id,
        } = *send;

        let command_id = &result_sender.command_id;

        tracing::debug!(target: METHOD_PATH,
                        ?deadline,
                        request_id = request_id.inner(),
                        command_id,
                        ?request_message,
                        "msg send");

        let res = self.as_mut().send_raw(request_message, deadline).await;

        if let Err(ref err) = res {
            tracing::warn!(target: METHOD_PATH,
                           ?err,
                           send = send_debug,
                           request_id = request_id.inner(),
                           command_id,
                           "send_raw error");
        }

        Self::respond_send(result_sender, res);
    }

    #[named]
    async fn handle_rpc_cmd(mut self: Pin<&mut Self>,
                            rpc: Box<CastRpc>, result_sender: TaskCmdResultSender)
    {
        const METHOD_PATH: &str = method_path!("Task");

        let deadline = tokio::time::Instant::now()
            .checked_add(self.config().rpc_timeout)
            .unwrap_or_else(|| panic!("{METHOD_PATH}: error calculating deadline"));

        let rpc_debug = format!("{rpc:#?}");

        let CastRpc {
            request_message,
            request_id,
            response_ns,
            response_type_names,
        } = *rpc;

        let command_id = &result_sender.command_id;

        tracing::trace!(target: METHOD_PATH,
                        ?deadline,
                        request_id = request_id.inner(),
                        command_id,
                        ?request_message,
                        response_ns,
                        ?response_type_names,
                        "rpc send");

        if let Err(err) = self.as_mut().send_raw(request_message, deadline).await {
            tracing::warn!(target: METHOD_PATH,
                           ?err,
                           rpc = rpc_debug,
                           request_id = request_id.inner(),
                           command_id,
                           response_ns,
                           ?response_type_names,
                           "send_raw error");

            Self::respond_rpc(result_sender, Err(err));
            return;
        }

        // # Record request state and set timeout.
        let timeout_key = self.as_mut().project()
                              .timeout_queue.insert_at(request_id, deadline);

        let state = RequestState {
            deadline,
            timeout_key,

            response_ns,
            response_type_names,
            result_sender,
        };

        self.as_mut().project().requests_map.insert(request_id, state);
    }

    #[named]
    async fn send_logged(mut self: Pin<&mut Self>, msg: CastMessage) {
        const METHOD_PATH: &str = method_path!("Task");

        let deadline = tokio::time::Instant::now()
            .checked_add(self.config().rpc_timeout)
            .unwrap_or_else(|| panic!("{METHOD_PATH}: error calculating deadline"));

        let msg_debug = format!("{msg:?}");

        tracing::debug!(target: METHOD_PATH,
                        ?deadline,
                        ?msg,
                        "msg send");

        let res = self.as_mut().send_raw(msg, deadline).await;

        if let Err(ref err) = res {
            tracing::warn!(target: METHOD_PATH,
                           ?err,
                           msg = msg_debug,
                           "send_raw error");
        }
    }

    async fn send_raw(self: Pin<&mut Self>, msg: CastMessage, deadline: tokio::time::Instant
    ) -> Result<()> {
        let mut proj = self.project();

        *proj.flush = Some(FlushState {
            deadline,
        });

        // TODO: Don't block the main task when conn sink buffer is full.
        //       Probably just return a backpressure error immediately,
        //       no point accumulating another send buffer on top of the bytes
        //       in the Framed sink.
        let fut = proj.conn_framed_sink.feed(msg);
        tokio::time::timeout_at(deadline, fut).await??;

        Ok(())
    }

    #[named]
    async fn handle_msg_read(mut self: Pin<&mut Self>, read_res: Result<CastMessage>) {
        const METHOD_PATH: &str = method_path!("Task");

        let msg: CastMessage = match read_res {
            Err(err) => {
                tracing::warn!(target: METHOD_PATH,
                               ?err,
                               "Message read error");
                self.publish_closed(ConnectionState::ClosedErr);
                return;
            },
            Ok(msg) => msg,
        };

        let msg_time = Utc::now();

        // # Reset the heartbeat timeouts.
        let instant_now = tokio::time::Instant::now();
        let heartbeat_disconnect_deadline =
            instant_now + self.config().heartbeat_disconnect_timeout;
        let heartbeat_ping_send_deadline =
            instant_now + self.config().heartbeat_ping_send_timeout;

        self.as_mut().project().heartbeat_disconnect_timeout
            .reset(heartbeat_disconnect_deadline);
        self.as_mut().project().heartbeat_ping_send_timeout
            .reset(heartbeat_ping_send_deadline);

        tracing::trace!(target: METHOD_PATH,
                        ?msg, ?msg_time,
                        "message read");

        let msg_ns = msg.namespace.as_str();
        if !JSON_NAMESPACES.contains(msg_ns) {
            tracing::warn!(target: METHOD_PATH,
                           msg_ns,
                           ?msg,
                           "message namespace not known");
            return;
        }

        let pd_json_str = match &msg.payload {
            CastMessagePayload::Binary(_b) => {
                tracing::warn!(target: METHOD_PATH,
                               msg_ns,
                               ?msg,
                               "binary message not known");
                return;
            },
            CastMessagePayload::String(s) => s.as_str(),
        };

        tracing::trace!(target: METHOD_PATH,
                        pd_json_str,
                        "message payload json string");

        let pd_all_dyn: serde_json::Value = match serde_json::from_str(pd_json_str) {
            Err(err) => {
                tracing::warn!(target: METHOD_PATH,
                               ?err, ?msg,
                               "error deserializing json as Value");
                return;
            },
            Ok(pd) => pd,
        };
        let pd: PayloadDyn = match serde_json::from_str::<Payload<()>>(pd_json_str) {
            Err(err) => {
                tracing::warn!(target: METHOD_PATH,
                               ?err, ?msg,
                               "error deserializing json");
                return;
            },
            Ok(pd_wrapper) => PayloadDyn {
                request_id: pd_wrapper.request_id,
                typ: pd_wrapper.typ,
                inner: pd_all_dyn,
            },
        };

        let pd_type = &pd.typ;

        tracing::trace!(target: METHOD_PATH,
                        ?pd,
                        "pd.request_id" = pd.request_id.map(|id| id.inner()),
                        "pd.typ" = ?pd_type,
                        "message payload as PayloadDyn");

        let msg_is_broadcast = msg.destination.as_str() == ENDPOINT_BROADCAST;
        if msg_is_broadcast {
            tracing::debug!(target: METHOD_PATH,
                            ?msg, ?pd, pd_type,
                            "broadcast message");
            // TODO: return to client through channel?
        }

        // TODO: Split off special case handling.

        // # Special message cases

        // Channel close
        if msg_ns == payload::connection::CHANNEL_NAMESPACE
            && pd.typ == payload::connection::MESSAGE_TYPE_CLOSE
        {
            tracing::debug!(target: METHOD_PATH,
                            ?msg, ?pd, pd_type,
                            "Connection closed message from destination.\n\n\
                             This may mean we were never connected to the destination \
                             (try calling method Client::connection_connect()) or \
                             we sent an invalid request.");
            return;
        }

        // Heartbeat ping from remote; reply with a pong.
        if msg_ns == payload::heartbeat::CHANNEL_NAMESPACE
            && pd.typ == payload::heartbeat::MESSAGE_TYPE_PING
        {
            self.handle_read_ping(msg.source).await;
            return;
        }

        // Heartbeat pong reply from remote; no further action.
        if msg_ns == payload::heartbeat::CHANNEL_NAMESPACE
            && pd.typ == payload::heartbeat::MESSAGE_TYPE_PONG
        {
            tracing::debug!(target: METHOD_PATH,
                            ?msg,
                            "Heartbeat pong response from remote.");
            return;
        }

        // Receiver status from remote; try to publish update to listeners.
        if msg_ns == payload::receiver::CHANNEL_NAMESPACE
            && pd.typ == payload::receiver::MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS
        {
            self.publish_receiver_status(&msg, &pd, msg_time);
        }

        // Media namespace status from remote; try to publish update to listeners.
        if msg_ns == payload::media::CHANNEL_NAMESPACE
            && pd.typ == payload::media::MESSAGE_RESPONSE_TYPE_MEDIA_STATUS
        {
            self.publish_media_status(&msg, &pd, msg_time);
        }

        let request_id = match pd.request_id {
            Some(RequestId::BROADCAST) | None => {
                if msg_is_broadcast {
                    return;
                }

                tracing::warn!(target: METHOD_PATH,
                               ?msg, ?pd, pd_type,
                               "missing request_id in unicast message payload");
                return;
            },
            Some(id) => id,
        };

        let mut proj = self.as_mut().project();

        // TODO: Do broadcast messages ever have request_id?

        let Some(request_state) = proj.requests_map.remove(&request_id) else {
            if msg_is_broadcast {
                return;
            }

            tracing::warn!(target: METHOD_PATH,
                           request_id = request_id.inner(),
                           ?msg, ?pd, pd_type,
                           "missing request state");
            return;
        };

        if proj.timeout_queue.as_mut().try_remove(&request_state.timeout_key).is_none() {
            tracing::warn!(target: METHOD_PATH,
                           ?request_state,
                           ?msg, ?pd, pd_type,
                           "timeout_queue missing expected delay key");
        }

        let result: Result<PayloadDyn> =
            if request_state.response_ns != msg_ns {
                Err(format_err!(
                    "{METHOD_PATH}: received reply message with unexpected namespace:\n\
                     _ request_id    = {request_id}\n\
                     _ expected_ns   = {expected_ns:?}\n\
                     _ msg_ns        = {msg_ns:?}\n\
                     _ pd_type       = {pd_type:?}\n\
                     _ request_state = {request_state:#?}",
                    expected_ns = request_state.response_ns))

                // TODO: Is this still useful? Why did I disable this?
                //       This will fail to deserialise in `Client` I think?
            } else if false && !request_state.response_type_names.contains(&pd.typ.as_str()) {
                Err(format_err!(
                    "{METHOD_PATH}: received reply message with unexpected type:\n\
                     _ request_id     = {request_id}\n\
                     _ expected_types = {expected_types:?}\n\
                     _ msg_ns         = {msg_ns:?}\n\
                     _ pd_type        = {pd_type:?}\n\
                     _ request_state  = {request_state:#?}",
                    expected_types = request_state.response_type_names,
                    pd_type = pd.typ))
            } else {
                Ok(pd)
            };

        Self::respond_rpc(request_state.result_sender, result);
    }

    #[named]
    fn handle_rpc_timeout(mut self: Pin<&mut Self>, expired: DelayExpired<RequestId>) {
        const METHOD_PATH: &str = method_path!("Task");

        let deadline = expired.deadline();
        let timeout_key = expired.key();
        let request_id = expired.get_ref();

        let proj = self.as_mut().project();

        let Some(request_state) = proj.requests_map.remove(request_id) else {
            panic!("{METHOD_PATH}: missing request_state in requests_map\n\
                    request_id: {request_id}");
        };

        assert_eq!(timeout_key, request_state.timeout_key);

        tracing::warn!(target: METHOD_PATH,
                       ?expired,
                       ?deadline,
                       request_id = request_id.inner(),
                       ?request_state,
                       "rpc timeout");

        let err = format_err!("{METHOD_PATH}: RPC timeout\n\
                               _ request_id:    {request_id}\n\
                               _ deadline:      {deadline:?}\n\
                               _ expired:       {expired:#?}\n\
                               _ request_state: {request_state:#?}");
        Self::respond_rpc(request_state.result_sender,
                          Err(err));
    }

    #[named]
    async fn handle_read_ping(self: Pin<&mut Self>, from: EndpointId) {
        const METHOD_PATH: &str = method_path!("Task");

        tracing::debug!(target: METHOD_PATH,
                        from,
                        "received ping");

        // # Send pong.
        let source = self.config().sender.clone();
        let pong_pd = Payload::<payload::heartbeat::Pong> {
            request_id: None,
            typ: payload::heartbeat::MESSAGE_TYPE_PONG.into(),
            inner: payload::heartbeat::Pong {},
        };
        let destination = from;
        tracing::trace!(target: METHOD_PATH,
                        ?pong_pd,
                        source, destination,
                        "pong payload struct");

        let pong_pd_json = match serde_json::to_string(&pong_pd) {
            Ok(j) => j,
            Err(err) => {
                tracing::error!(target: METHOD_PATH,
                                ?err,
                                ?pong_pd,
                                source, destination,
                                "serde_json serialisation error for pong payload");
                return;
            },
        };

        let pong_msg = CastMessage {
            namespace: payload::heartbeat::CHANNEL_NAMESPACE.into(),
            source, destination,
            payload: pong_pd_json.into(),
        };

        self.send_logged(pong_msg).await
    }

    #[named]
    async fn handle_heartbeat_ping_send_timeout(mut self: Pin<&mut Self>) {
        const METHOD_PATH: &str = method_path!("Task");

        let ping_send_timeout = self.config().heartbeat_ping_send_timeout;

        tracing::debug!(target: METHOD_PATH,
                        ?ping_send_timeout,
                        "Not received a message for timeout duration, sending a ping message");

        let instant_now = tokio::time::Instant::now();
        let heartbeat_ping_send_deadline =
            instant_now + ping_send_timeout;

        self.as_mut().project().heartbeat_ping_send_timeout
            .reset(heartbeat_ping_send_deadline);

        // # Send ping.
        let source = self.config().sender.clone();
        let ping_pd = Payload::<payload::heartbeat::Ping> {
            request_id: None,
            typ: payload::heartbeat::MESSAGE_TYPE_PING.into(),
            inner: payload::heartbeat::Ping {},
        };
        let destination = DEFAULT_RECEIVER_ID.to_string();
        tracing::trace!(target: METHOD_PATH,
                        ?ping_pd,
                        source, destination,
                        "ping payload struct");

        let ping_pd_json = match serde_json::to_string(&ping_pd) {
            Ok(j) => j,
            Err(err) => {
                tracing::error!(target: METHOD_PATH,
                                ?err,
                                ?ping_pd,
                                source, destination,
                                "serde_json serialisation error for ping payload");
                return;
            },
        };

        let ping_msg = CastMessage {
            namespace: payload::heartbeat::CHANNEL_NAMESPACE.into(),
            source, destination,
            payload: ping_pd_json.into(),
        };

        self.send_logged(ping_msg).await;
    }

    fn respond_rpc(result_sender: TaskCmdResultSender,
                   result: Result<PayloadDyn>)
    {
        Self::respond_generic(result_sender, result);
    }

    fn respond_send(result_sender: TaskCmdResultSender,
                    result: Result<()>)
    {
        Self::respond_generic(result_sender, result);
    }

    /// Used to respond with an error when the Ok result type is not known.
    ///
    /// `task_cmd` only downcasts after it checks for an Err, so this should not
    /// fail at runtime.
    fn respond_err(result_sender: TaskCmdResultSender,
                   err: Error)
    {
        Self::respond_generic::<()>(result_sender, Err(err))
    }

    // TODO: Move to a TaskCmdResultSender method.
    fn respond_generic<R>(result_sender: TaskCmdResultSender,
                          result: Result<R>)
    where R: Any + Debug + Send + Sync
    {
        let command_id = result_sender.command_id;
        let result_ok = result.is_ok();
        let result_variant = if result_ok { "Ok"  }
                             else         { "Err" };

        let boxed = result.map(|response| TaskResponseBox::new(response));

        match result_sender.result_tx.send(boxed) {
            Ok(()) =>
                tracing::trace!(
                    command_id,
                    result_variant,
                    "Task::respond: sent result ok"),
            Err(unsent) =>
                tracing::warn!(
                    command_id,
                    result_variant,
                    ?unsent,
                    "Task::respond: result channel dropped"),
        }
    }

    #[named]
    fn publish_receiver_status(&self,
                               msg: &CastMessage, pd: &PayloadDyn, msg_time: DateTime<Utc>)
    {
        let pd_typed: Payload::<payload::receiver::GetStatusResponse> =
            match serde_json::from_value(pd.inner.clone()) {
                Ok(v) => v,
                Err(err) => {
                    tracing::error!(target: method_path!("Task"),
                                    ?pd, ?msg, ?err,
                                    "error deserialising typed receiver status payload");
                    return;
                }
            };

        let payload::receiver::StatusWrapper { status: receiver_status } = pd_typed.inner.0;

        let update = StatusUpdate {
            time: msg_time,
            msg: StatusMessage::Receiver(ReceiverStatusMessage {
                receiver_status,
                receiver_id: msg.source.clone(),
            }),
        };
        self.publish_status_update(update);
    }

    #[named]
    fn publish_media_status(&self,
                            msg: &CastMessage, pd: &PayloadDyn, msg_time: DateTime<Utc>)
    {
        let pd_typed: Payload::<payload::media::Status> =
            match serde_json::from_value(pd.inner.clone()) {
                Ok(v) => v,
                Err(err) => {
                    tracing::error!(target: method_path!("Task"),
                                    ?pd, ?msg, ?err,
                                    "error deserialising typed media status payload");
                    return;
                }
            };

        let update = StatusUpdate {
            time: msg_time,
            msg: StatusMessage::Media(MediaStatusMessage {
                app_destination_id: msg.source.clone(),
                media_status: pd_typed.inner,
            }),
        };
        self.publish_status_update(update);
    }

    #[named]
    fn publish_status_update(&self, update: StatusUpdate) {
        const METHOD_PATH: &str = method_path!("Task");
        tracing::debug!(target: METHOD_PATH,
                        ?update,
                        "status update");

        // Ignore an error result, which just means no receivers are currently listening.
        if let Err(_err) = self.shared.status_tx.send(update) {
            tracing::trace!(target: METHOD_PATH,
                            "status send err: no receivers listening");
        }
    }

    fn publish_closed(mut self: Pin<&mut Self>, state: ConnectionState) {
        let msg: StatusMessage = match state {
            ConnectionState::Connected => panic!("Task::publish_closed: bad state '{state:?}'"),

            ConnectionState::ClosedOk => StatusMessage::ClosedOk,
            ConnectionState::ClosedErr => StatusMessage::ClosedErr,
        };

        *self.as_mut().project().closed = true;

        let _prev = self.conn_state_tx.send_replace(state);

        self.publish_status_update(msg.now());

        // Return errors to all outstanding response channels.
        for (_req_id, req_state) in self.project().requests_map.drain() {
            Self::respond_err(req_state.result_sender,
                              format_err!("Task stopping, state: {state:?}"));
        }
    }

    fn config(&self) -> &Config {
        &self.shared.config
    }
}

impl TaskResponseBox {
    pub fn new<R>(response: R) -> TaskResponseBox
    where R: Any + Send + Sync
    {
        TaskResponseBox {
            type_name: any::type_name::<R>(),
            value: Box::new(response) as Box<dyn Any + Send + Sync>,
        }
    }

    pub fn downcast<R>(self) -> Result<Box<R>>
    where R: Any + Send + Sync
    {
        let TaskResponseBox { type_name, value } = self;

        value.downcast::<R>()
             .map_err(|_as_any| format_err!("Command response type didn't match expected\n\
                                            expected type: {expected:?}\n\
                                            type:          {ty:?}",
                                            expected = any::type_name::<R>(),
                                            ty       = type_name))
    }
}

const SIZE_OF_U32: usize = 4;

// TODO: Box message?
impl codec::Encoder<CastMessage> for CastMessageCodec {
    type Error = Error;

    fn encode(
        &mut self,
        msg: CastMessage,
        dst: &mut BytesMut
    ) -> Result<()>
    {
        use crate::cast::cast_channel::cast_message::{PayloadType, ProtocolVersion};

        let mut proto_msg = crate::cast::cast_channel::CastMessage::new();

        proto_msg.set_protocol_version(ProtocolVersion::CASTV2_1_0);

        proto_msg.set_namespace(msg.namespace);
        proto_msg.set_source_id(msg.source);
        proto_msg.set_destination_id(msg.destination);

        match msg.payload {
            CastMessagePayload::String(s) => {
                proto_msg.set_payload_type(PayloadType::STRING);
                proto_msg.set_payload_utf8(s);
            },

            CastMessagePayload::Binary(b) => {
                proto_msg.set_payload_type(PayloadType::BINARY);
                proto_msg.set_payload_binary(b);
            },
        };

        let proto_len: usize = proto_msg.compute_size().try_into()?;
        let proto_len_u32: u32 = proto_len.try_into()?;

        let total_len: usize = proto_len + SIZE_OF_U32;

        dst.clear();
        dst.reserve(total_len);

        // Uses big endian
        dst.put_u32(proto_len_u32);

        // Braces to limit the scope of writer.
        {
            let mut writer = dst.limit(proto_len as usize)
                                .writer();
            proto_msg.write_to_writer(&mut writer)?;
        }

        assert_eq!(dst.len(), total_len);

        Ok(())
    }
}

impl codec::Decoder for CastMessageCodec {
    type Item = CastMessage;
    type Error = Error;

    fn decode(
        &mut self,
        src: &mut BytesMut
    ) -> Result<Option<CastMessage>>
    {

        if src.len() < SIZE_OF_U32 {
            return Ok(None);
        }

        let proto_len_bytes = <[u8; SIZE_OF_U32]>::try_from(&src[0..SIZE_OF_U32]).unwrap();
        let proto_len_u32: u32 = u32::from_be_bytes(proto_len_bytes);
        let proto_len = usize::try_from(proto_len_u32)?;

        let total_len: usize = proto_len + SIZE_OF_U32;

        let src_len = src.len();

        if src_len < total_len {
            src.reserve(total_len - src_len);
            return Ok(None);
        }

        let mut proto_msg: crate::cast::cast_channel::CastMessage = {
            // Braces to scope proto_bytes' borrow.
            let proto_bytes = &src[SIZE_OF_U32..total_len];
            assert_eq!(proto_bytes.len(), proto_len);

            crate::cast::cast_channel::CastMessage::parse_from_bytes(proto_bytes)?
        };

        src.advance(total_len);

        use crate::cast::cast_channel::cast_message::PayloadType;

        let msg = CastMessage {
            namespace: proto_msg.take_namespace(),
            source: proto_msg.take_source_id(),
            destination: proto_msg.take_destination_id(),
            payload: match proto_msg.payload_type() {
                PayloadType::STRING =>
                    CastMessagePayload::String(proto_msg.take_payload_utf8()),
                PayloadType::BINARY =>
                    CastMessagePayload::Binary(proto_msg.take_payload_binary()),
            },
        };

        Ok(Some(msg))
    }
}

impl StatusMessage {
    fn now(self) -> StatusUpdate {
        StatusUpdate {
            time: Utc::now(),
            msg: self,
        }
    }
}

pub mod small_debug {
    use super::*;

    pub struct StatusUpdate<'a>(pub &'a super::StatusUpdate);
    pub struct StatusMessage<'a>(pub &'a super::StatusMessage);
    pub struct ReceiverStatusMessage<'a>(pub &'a super::ReceiverStatusMessage);
    pub struct MediaStatusMessage<'a>(pub &'a super::MediaStatusMessage);


    impl<'a> Debug for self::StatusUpdate<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("StatusUpdate")
                .field("time", &self.0.time)
                .field("msg", &StatusMessage(&self.0.msg))
                .finish()
        }
    }

    impl<'a> Debug for self::StatusMessage<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self.0 {
                super::StatusMessage::Media(ms) =>
                    Debug::fmt(&MediaStatusMessage(&ms), f),

                super::StatusMessage::Receiver(rs) =>
                    Debug::fmt(&ReceiverStatusMessage(&rs), f),

                _ => Debug::fmt(self, f),
            }
        }
    }

    impl<'a> Debug for self::MediaStatusMessage<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("MediaStatusMessage")
             .field("app_destination_id", &self.0.app_destination_id)
             .field("media_status", &payload::media::small_debug::MediaStatus(
                                        &self.0.media_status))
             .finish()
        }
    }

    impl<'a> Debug for self::ReceiverStatusMessage<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("MediaStatusMessage")
             .field("receiver_id", &self.0.receiver_id)
             .field("receiver_status", &payload::receiver::small_debug::ReceiverStatus(
                                           &self.0.receiver_status))
             .finish()
        }
    }
}
