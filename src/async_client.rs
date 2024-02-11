use anyhow::{bail, format_err};
use bytes::{Buf, BufMut, BytesMut};
use crate::{
    cast::proxies,
    json_payload::{self, Payload, PayloadDyn, RequestInner, ResponseInner},
    message::{
        CastMessage,
        CastMessagePayload,
    },
    types::{AppId, /* AppIdConst, */
            AppSession,
            EndpointId, EndpointIdConst, ENDPOINT_BROADCAST,
            /* MediaSession, MediaSessionId, */
            MediaSessionId,
            /* MessageType, */ MessageTypeConst,
            /* Namespace, */ NamespaceConst,
            RequestId, /* SessionId */},
    util::named,
};
use futures::{
    future::Either, SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use once_cell::sync::Lazy;
use pin_project_lite::pin_project;
use protobuf::Message;
use std::{
    any::{self, Any},
    collections::{HashMap, HashSet},
    fmt::Debug,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Arc, atomic::{AtomicI32, AtomicUsize, Ordering}},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    pin,
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

    task_cmd_tx: tokio::sync::mpsc::Sender::<TaskCommand>,

    next_request_id: AtomicI32,
    next_command_id: AtomicUsize,

    config: Arc<Config>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub addr: SocketAddr,

    /// `EndpointId` used as the sender, and source of messages we send.
    ///
    /// Set `None` for the default, or `Some(a)` will override it.
    pub sender: Option<EndpointId>,
}

#[derive(Debug)]
pub struct LoadMediaArgs {
    pub media: proxies::media::Media,

    pub current_time: f64,
    pub autoplay: bool,

    /// None to use default.
    pub preload_time: Option<f64>,

    // TODO: Decide whether to expose custom data.
    // custom_data: serde_json::Value,

    // TODO: Add defaults or builder.
}

pin_project! {
    struct Task<S: TokioAsyncStream> {
        #[pin]
        conn_framed_sink: SplitSink<Framed<S, CastMessageCodec>, CastMessage>,

        #[pin]
        conn_framed_stream: SplitStream<Framed<S, CastMessageCodec>>,

        #[pin]
        task_cmd_rx: tokio_stream::wrappers::ReceiverStream::<TaskCommand>,

        #[pin]
        timeout_queue: DelayQueue<RequestId>,

        need_flush: bool,
        requests_map: HashMap<RequestId, RequestState>,

        config: Arc<Config>,
    }
}

#[derive(Debug)]
struct RequestState {
    response_ns: NamespaceConst,
    response_type_names: &'static [MessageTypeConst],
    delay_key: DelayKey,

    #[allow(dead_code)] // Just for debugging for now.
    deadline: tokio::time::Instant,

    result_sender: TaskCommandResultSender,
}

#[derive(Debug)]
struct TaskCommandResultSender {
    command_id: CommandId,
    result_tx: tokio::sync::oneshot::Sender::<TaskCommandResult>,
}

#[derive(Debug)]
struct TaskCommand {
    command: TaskCommandType,
    result_sender: TaskCommandResultSender,
}

#[derive(Debug)]
enum TaskCommandType {
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
    value: Box<dyn Any + Send + Sync>,
}

type TaskCommandResult = Result<TaskResponseBox>;

pub trait TokioAsyncStream: AsyncRead + AsyncWrite + Unpin {}

impl<T> TokioAsyncStream for T
where T: AsyncRead + AsyncWrite + Unpin
{}

type CommandId = usize;

struct CastMessageCodec;

/// Duration for the Task to do something locally. (Probably a bit high).
const LOCAL_TASK_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1_000);

/// Duration for an RPC request and response to the Chromecast.
const RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(5_000);

const DATA_BUFFER_LEN: usize = 64 * 1024;

static JSON_NAMESPACES: Lazy<HashSet<NamespaceConst>> = Lazy::<HashSet<NamespaceConst>>::new(|| {
    HashSet::from([
        json_payload::connection::CHANNEL_NAMESPACE,
        json_payload::heartbeat::CHANNEL_NAMESPACE,
        json_payload::media::CHANNEL_NAMESPACE,
        json_payload::receiver::CHANNEL_NAMESPACE,
    ])
});

pub const DEFAULT_SENDER_ID: EndpointIdConst = "sender-0";
pub const DEFAULT_RECEIVER_ID: EndpointIdConst = "receiver-0";

/// Well known cast receiver app IDs
pub mod app {
    use crate::types::AppIdConst;

    pub const DEFAULT_MEDIA_RECEIVER: AppIdConst = "CC1AD845";
    pub const BACKDROP_ID: AppIdConst = "E8C28D3C";
    pub const YOUTUBE_ID: AppIdConst = "233637DE";
}

impl Config {
    pub async fn connect(self) -> Result<Client> {
        let conn = tls_connect(&self).await?;

        let (task_cmd_tx, task_cmd_rx) = tokio::sync::mpsc::channel(/* buffer: */ 32);

        let config = Arc::new(self);

        let task = Task::new(conn, task_cmd_rx, Arc::clone(&config));

        let task_join_handle = tokio::spawn(task.main());

        let mut client = Client {
            task_join_handle: Some(task_join_handle),
            task_cmd_tx,

            // Some broadcasts have `request_id` 0, so don't re-use that.
            next_request_id: AtomicI32::new(1),

            next_command_id: AtomicUsize::new(1),
            config,
        };

        client.init().await?;

        Ok(client)
    }
}

impl Client {
    pub async fn receiver_status(&mut self) -> Result<proxies::receiver::Status> {
        let payload_req = json_payload::receiver::StatusRequest {};

        let resp: Payload<json_payload::receiver::StatusResponse>
            = self.json_rpc(payload_req, DEFAULT_RECEIVER_ID.to_string()).await?;

        Ok(resp.inner.status)
    }

    #[named]
    pub async fn receiver_launch_app(&mut self, destination_id: EndpointId, app_id: AppId)
    -> Result<(proxies::receiver::Application, proxies::receiver::Status)>
    {
        const METHOD_PATH: &str = method_path!("Client");

        let payload_req = json_payload::receiver::LaunchRequest {
            app_id: app_id.clone(),
        };

        let resp: Payload<json_payload::receiver::LaunchResponse>
            = self.json_rpc(payload_req, destination_id).await?;

        let json_payload::receiver::LaunchResponse::OkStatus { status } = resp.inner else {
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
    -> Result<proxies::receiver::Status>
    {
        use json_payload::receiver::{StopRequest, StopResponse};

        const METHOD_PATH: &str = method_path!("Client");

        let payload_req = StopRequest {
            session_id: app_session.session_id,
        };

        let resp: Payload<StopResponse>
            = self.json_rpc(payload_req, app_session.receiver_destination_id).await?;

        let StopResponse::OkStatus { status } = resp.inner else {
            bail!("{METHOD_PATH}: error response\n\
                   response: {resp:#?}");
        };

        Ok(status)
    }

    #[named]
    pub async fn receiver_set_volume(&mut self,
                                     destination_id: EndpointId,
                                     volume: proxies::receiver::Volume)
    -> Result<proxies::receiver::Status>
    {
        use json_payload::receiver::{SetVolumeRequest, SetVolumeResponse};

        const METHOD_PATH: &str = method_path!("Client");

        let payload_req = SetVolumeRequest {
            volume,
        };

        let resp: Payload<SetVolumeResponse>
            = self.json_rpc(payload_req, destination_id).await?;

        let SetVolumeResponse::OkStatus { status } = resp.inner else {
            bail!("{METHOD_PATH}: error response\n\
                   response: {resp:#?}");
        };

        Ok(status)
    }

    pub async fn media_launch_default(&mut self, destination_id: EndpointId)
    -> Result<(AppSession, proxies::receiver::Status)> {
        let (app, status) = self.receiver_launch_app(destination_id.clone(),
                                                     app::DEFAULT_MEDIA_RECEIVER.into()).await?;
        let session = app.to_app_session(destination_id.clone())?;
        self.connection_connect(session.app_destination_id.clone()).await?;

        Ok((session, status))
    }

    // TODO: Better argument types?
    #[named]
    pub async fn media_status(&mut self,
                              app_session: AppSession,
                              media_session_id: Option<MediaSessionId>)
    -> Result<json_payload::media::MediaStatus> {
        let payload_req = json_payload::media::GetStatusRequest {
            media_session_id,
        };

        let resp: Payload<json_payload::media::GetStatusResponse>
            = self.json_rpc(payload_req, app_session.app_destination_id).await?;

        let json_payload::media::GetStatusResponse::MediaStatus(media_status) = resp.inner else {
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
    pub async fn media_load(&mut self,
                            app_session: AppSession,
                            load_args: LoadMediaArgs)
    -> Result<json_payload::media::MediaStatus> {
        let payload_req = json_payload::media::LoadRequest {
            session_id: app_session.session_id,

            media: load_args.media,

            current_time: load_args.current_time,
            custom_data: serde_json::Value::Null,
            autoplay: load_args.autoplay,
            preload_time: load_args.preload_time.unwrap_or(10_f64),
        };

        let resp: Payload<json_payload::media::LoadResponse>
            = self.json_rpc(payload_req, app_session.app_destination_id.clone()).await?;

        let json_payload::media::LoadResponse::MediaStatus(status) = resp.inner else {
            bail!("{method_path}: Error response\n\
                   _ response = {resp:#?}",
                  method_path = method_path!("Client"));
        };

        Ok(status)
    }

    pub async fn close(mut self) -> Result<()> {
        let _: Box<()> = self.task_cmd::<()>(TaskCommandType::Shutdown).await?;

        let join_fut = self.task_join_handle.take()
                           .expect("task_join_handle is Some(_) until .close().");

        tokio::time::timeout(LOCAL_TASK_COMMAND_TIMEOUT, join_fut).await???;

        Ok(())
    }
}

/// Internals.
impl Client {
    async fn init(&mut self) -> Result<()> {
        self.connection_connect(DEFAULT_RECEIVER_ID.to_string()).await?;

        Ok(())
    }

    fn config(&self) -> &Config {
        &self.config
    }

    pub async fn connection_connect(&mut self, destination: EndpointId) -> Result<()> {
        let payload_req = json_payload::connection::ConnectRequest {
            user_agent: json_payload::USER_AGENT.to_string(),
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

        let cmd_type = TaskCommandType::CastSend(Box::new(CastSend {
            request_message,
            request_id,
        }));

        let _resp: Box<()> = self.task_cmd(cmd_type).await?;

        Ok(())
    }

    #[named]
    async fn json_rpc<Req, Resp>(&mut self, req: Req, destination: EndpointId)
    -> Result<Payload<Resp>>
    where Req: RequestInner,
          Resp: ResponseInner
    {
        let start = tokio::time::Instant::now();

        let (request_message, request_id) = self.cast_request_from_inner(req, destination)?;

        let response_ns = Resp::CHANNEL_NAMESPACE;
        let response_type_names = Resp::TYPE_NAMES;

        let cmd_type = TaskCommandType::CastRpc(Box::new(CastRpc {
            request_message,
            request_id,
            response_ns,
            response_type_names,
        }));

        let resp_dyn: Box<PayloadDyn> = self.task_cmd(cmd_type).await?;
        let resp: Payload<Resp> = self.response_from_dyn(resp_dyn)?;

        let elapsed = start.elapsed();

        tracing::debug!(target: method_path!("Client"),
                        ?elapsed,
                        response_payload = ?resp,
                        response_ns,
                        response_type_name = resp.typ,
                        expected_response_type_names = ?response_type_names,
                        request_id,
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

        let sender = self.config().sender();
        let request_namespace = Req::CHANNEL_NAMESPACE.to_string();

        tracing::debug!(target: method_path!("Client"),
                        ?payload,
                        request_id,
                        request_type = payload.typ,
                        request_namespace,
                        sender, destination,
                        "payload struct");

        let payload_json = serde_json::to_string(&payload)?;

        tracing::trace!(target: method_path!("Client"),
                        payload_json,
                        request_id,
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

    async fn task_cmd<R>(&mut self, cmd_type: TaskCommandType)
    -> Result<Box<R>>
    where R: Any + Send + Sync
    {
        let command_id = self.take_command_id();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel::<TaskCommandResult>();

        let cmd = TaskCommand {
            command: cmd_type,
            result_sender: TaskCommandResultSender {
                command_id,
                result_tx,
            },
        };
        let command_timeout: std::time::Duration = match &cmd.command {
            TaskCommandType::CastRpc(_) => RPC_TIMEOUT,
            TaskCommandType::CastSend(_) => RPC_TIMEOUT,
            TaskCommandType::Shutdown => LOCAL_TASK_COMMAND_TIMEOUT,
        };

        self.task_cmd_tx.send_timeout(
            cmd,
            LOCAL_TASK_COMMAND_TIMEOUT).await?;

        let response: TaskResponseBox =
            tokio::time::timeout(command_timeout, result_rx).await???;

        response.downcast::<R>()
    }

    fn take_request_id(&self) -> RequestId {
        self.next_request_id.fetch_add(1, Ordering::SeqCst)
    }

    fn take_command_id(&self) -> CommandId {
        self.next_command_id.fetch_add(1, Ordering::SeqCst)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if self.task_join_handle.is_some() {
            tracing::error!("Client: task not stopped before drop.\n\
                             Use Client::close to dispose of Client.");
        }
    }
}

#[named]
async fn tls_connect(config: &Config)
-> Result<impl TokioAsyncStream>
{
    const FUNCTION_PATH: &str = function_path!();

    let addr = &config.addr;
    let ip: IpAddr = addr.ip();
    let port: u16 = addr.port();

    let mut config = rustls::ClientConfig::builder()
        .dangerous().with_custom_certificate_verifier(Arc::new(
            crate::util::rustls::danger::NoCertificateVerification::new_ring()))
        .with_no_client_auth();

    config.enable_early_data = true;
    let config = Arc::new(config);

    let connector = tokio_rustls::TlsConnector::from(config);

    let ip_rustls = rustls::pki_types::IpAddr::from(ip);
    let domain = rustls::pki_types::ServerName::IpAddress(ip_rustls);

    let _conn_span = tracing::info_span!(
        target: FUNCTION_PATH,
        "Connecting to cast device",
        %addr, %ip, port,
    ).entered();

    let tcp_stream = tokio::net::TcpStream::connect(addr).await?;

    tracing::debug!(target: FUNCTION_PATH,
                    "TcpStream connected");

    let tls_stream = connector.connect(domain, tcp_stream).await?;

    tracing::debug!(target: FUNCTION_PATH,
                    "TlsStream connected");

    Ok(tls_stream)
}

#[derive(Debug)]
enum TaskEvent {
    Cmd(TaskCommand),
    Flush(Result<()>),
    MessageRead(Result<CastMessage>),
    RpcTimeout(DelayExpired<RequestId>),
}

impl<S: TokioAsyncStream> Task<S> {
    pub fn new(
        conn: S,
        task_cmd_rx: tokio::sync::mpsc::Receiver::<TaskCommand>,
        config: Arc<Config>,
    ) -> Task<S> {
        let task_cmd_rx = tokio_stream::wrappers::ReceiverStream::new(task_cmd_rx);

        let timeout_queue = DelayQueue::<RequestId>::with_capacity(4);

        let cast_message_codec = CastMessageCodec;
        let conn_framed = tokio_util::codec::Framed::with_capacity(
            conn, cast_message_codec, DATA_BUFFER_LEN);

        let (conn_framed_sink, conn_framed_stream) = conn_framed.split();

        Task {
            conn_framed_sink,
            conn_framed_stream,

            task_cmd_rx,
            timeout_queue,

            need_flush: false,
            requests_map: HashMap::new(),
            config,
        }
    }

    #[named]
    async fn main(self) -> Result<()> {
        // TODO: Timeouts for requests, send response and clean up.

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
                    TaskCommandType::CastRpc(rpc) => {
                        this.as_mut().handle_rpc_cmd(rpc, cmd.result_sender).await;
                    },

                    TaskCommandType::CastSend(send) => {
                        this.as_mut().handle_send(send, cmd.result_sender).await;
                    },

                    TaskCommandType::Shutdown => {
                        tracing::info!(target: METHOD_PATH,
                                       "shutdown on command");
                        Self::respond_generic(cmd.result_sender, Ok(()));
                        return Ok(());
                    },
                },

                TaskEvent::MessageRead(read_res) => {
                    this.as_mut().handle_msg_read(read_res).await;
                },

                TaskEvent::RpcTimeout(expired) => {
                    this.as_mut().handle_rpc_timeout(expired);
                }

                TaskEvent::Flush(res) => {
                    if let Err(err) = res {
                        // TODO: Mark connection as dead?
                        // TODO: Add optional auto reconnect.
                        tracing::warn!(target: METHOD_PATH,
                                       ?err,
                                       "flush error");
                    }
                    this.need_flush = false;
                }
            }
        }

        tracing::info!(target: METHOD_PATH,
                       "shutdown on event stream closed");

        // TODO: cleanup? e.g.
        //   * flush outputs
        //   * reset connections,
        //   * return errors to response channels
        //     (once response channel senders are dropped this will happen anyway)

        Ok(())
    }

    async fn take_next_event(self: Pin<&mut Self>) -> Option<TaskEvent> {
        let mut proj = self.project();

        let conn_flush_stream = if *proj.need_flush {
            let fut = proj.conn_framed_sink.flush();
            let stream = futures::stream::once(fut);
            Either::Left(stream)
        } else {
            Either::Right(futures::stream::empty())
        };

        // poll_fn???

        // Streams polled in order with current implementation on first
        // poll of Merge.
        let streams = (
            &mut (conn_flush_stream.map(TaskEvent::Flush)),
            &mut (proj.task_cmd_rx.map(TaskEvent::Cmd)),
            &mut (proj.timeout_queue.map(TaskEvent::RpcTimeout)),
            &mut (proj.conn_framed_stream.map(TaskEvent::MessageRead)),
        );

        let mut merged = futures_concurrency::stream::Merge::merge(streams);

        merged.next().await
    }

    #[named]
    async fn handle_send(mut self: Pin<&mut Self>,
                         send: Box<CastSend>, result_sender: TaskCommandResultSender)
    {
        const METHOD_PATH: &str = method_path!("Task");

        let deadline = tokio::time::Instant::now()
            .checked_add(RPC_TIMEOUT)
            .unwrap_or_else(|| panic!("{METHOD_PATH}: error calculating deadline"));

        let send_debug = format!("{send:#?}");

        let CastSend {
            request_message,
            request_id,
        } = *send;

        let command_id = &result_sender.command_id;

        tracing::debug!(target: METHOD_PATH,
                        ?deadline,
                        request_id,
                        command_id,
                        ?request_message,
                        "msg send");

        let res = self.as_mut().send_raw(request_message, deadline).await;

        if let Err(ref err) = res {
            tracing::warn!(target: METHOD_PATH,
                           ?err,
                           send = send_debug,
                           request_id,
                           command_id,
                           "send_raw error");
        }

        Self::respond_send(result_sender, res);
    }

    #[named]
    async fn handle_rpc_cmd(mut self: Pin<&mut Self>,
                            rpc: Box<CastRpc>, result_sender: TaskCommandResultSender)
    {
        const METHOD_PATH: &str = method_path!("Task");

        let deadline = tokio::time::Instant::now()
            .checked_add(RPC_TIMEOUT)
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
                        request_id,
                        command_id,
                        ?request_message,
                        response_ns,
                        ?response_type_names,
                        "rpc send");

        if let Err(err) = self.as_mut().send_raw(request_message, deadline).await {
            tracing::warn!(target: METHOD_PATH,
                           ?err,
                           rpc = rpc_debug,
                           request_id,
                           command_id,
                           response_ns,
                           ?response_type_names,
                           "send_raw error");

            Self::respond_rpc(result_sender, Err(err));
            return;
        }

        // # Record request state and set timeout.
        let delay_key = self.as_mut().project()
                            .timeout_queue.insert_at(request_id, deadline);

        let state = RequestState {
            deadline,
            delay_key,

            response_ns,
            response_type_names,
            result_sender,
        };

        self.as_mut().requests_map.insert(request_id, state);
    }

    #[named]
    async fn send_logged(mut self: Pin<&mut Self>, msg: CastMessage) {
        const METHOD_PATH: &str = method_path!("Task");

        let deadline = tokio::time::Instant::now()
            .checked_add(RPC_TIMEOUT)
            .unwrap_or_else(|| panic!("{METHOD_PATH}: error calculating deadline"));

        let msg_debug = format!("{msg:#?}");

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

        *proj.need_flush = true;

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
                return;
            }
            Ok(msg) => msg,
        };

        tracing::trace!(target: METHOD_PATH,
                        ?msg,
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
                        "message payload json");

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

        let msg_is_broadcast = msg.destination.as_str() == ENDPOINT_BROADCAST;
        if msg_is_broadcast {
            tracing::debug!(target: METHOD_PATH,
                            ?msg, ?pd, pd_type,
                            "broadcast message");
            // TODO: return to client through channel.
        }

        // TODO: Split off special case handling.

        // # Special message cases

        // Channel close
        if msg_ns == json_payload::connection::CHANNEL_NAMESPACE
            && pd.typ == json_payload::connection::MESSAGE_TYPE_CLOSE
        {
            tracing::error!(target: METHOD_PATH,
                            ?msg, ?pd, pd_type,
                            "Connection closed message from destination.\n\n\
                             This usually means we were never connected to the destination \
                             (try calling method Client::connection_connect()) or \
                             we sent an invalid request.");
            return;
        }

        // Heartbeat ping from remote; reply with a pong.
        if msg_ns == json_payload::heartbeat::CHANNEL_NAMESPACE
            && pd.typ == json_payload::heartbeat::MESSAGE_TYPE_PING
        {
            self.handle_read_ping(msg.source).await;
            return;
        }

        let request_id = match pd.request_id {
            Some(0) | None => {
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

        let Some(request_state) = proj.requests_map.remove(&request_id) else {
            tracing::warn!(target: METHOD_PATH,
                           request_id, ?msg, ?pd, pd_type,
                           "missing request state");
            return;
        };

        if proj.timeout_queue.as_mut().try_remove(&request_state.delay_key).is_none() {
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
        let delay_key = expired.key();
        let request_id = expired.get_ref();

        let proj = self.as_mut().project();

        let Some(request_state) = proj.requests_map.remove(request_id) else {
            panic!("{METHOD_PATH}: missing request_state in requests_map\n\
                    request_id: {request_id}");
        };

        assert_eq!(delay_key, request_state.delay_key);

        tracing::warn!(target: METHOD_PATH,
                       ?expired,
                       ?deadline,
                       request_id,
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
    async fn handle_read_ping(mut self: Pin<&mut Self>, destination: EndpointId) {
        let source = self.as_mut().config().sender();
        let pong_pd = Payload::<json_payload::heartbeat::Pong> {
            request_id: None,
            typ: json_payload::heartbeat::MESSAGE_TYPE_PONG.into(),
            inner: json_payload::heartbeat::Pong {},
        };
        tracing::debug!(target: method_path!("Task"),
                        ?pong_pd,
                        source, destination,
                        "pong payload struct");

        let pong_pd_json = match serde_json::to_string(&pong_pd) {
            Ok(j) => j,
            Err(err) => {
                tracing::error!(target: method_path!("Task"),
                                ?err,
                                ?pong_pd,
                                source, destination,
                                "serde_json serialisation error for pong payload");
                return;
            },
        };

        let pong_msg = CastMessage {
            namespace: json_payload::heartbeat::CHANNEL_NAMESPACE.into(),
            source, destination,
            payload: pong_pd_json.into(),
        };

        self.send_logged(pong_msg).await
    }

    fn respond_rpc(result_sender: TaskCommandResultSender,
                   result: Result<PayloadDyn>)
    {
        Self::respond_generic(result_sender, result);
    }

    fn respond_send(result_sender: TaskCommandResultSender,
                    result: Result<()>)
    {
        Self::respond_generic(result_sender, result);
    }

    fn respond_generic<R>(result_sender: TaskCommandResultSender,
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

    fn config(&self) -> &Config {
        &self.config
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

impl Config {
    fn sender(&self) -> EndpointId {
        self.sender.as_ref()
            .cloned()
            .unwrap_or_else(|| DEFAULT_SENDER_ID.to_string())
    }
}
