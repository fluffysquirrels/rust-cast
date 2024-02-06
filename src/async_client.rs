use anyhow::{bail, format_err};
use bytes::{Buf, BufMut, BytesMut};
use crate::{
    cast::proxies,
    json_payload::{self, Payload, PayloadDyn, RequestInner, ResponseInner},
    message::{
        CastMessage,
        CastMessagePayload,
    },
    types::{AppId, MessageType, MessageTypeConst, Namespace, NamespaceConst, RequestId},
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
}

pub struct Config {
    pub addr: SocketAddr,
}

pin_project! {
    struct Task<S: TokioAsyncStream> {
        // #[pin]
        // conn_framed: tokio_util::codec::Framed<S, CastMessageCodec>,

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
    }
}

#[derive(Debug)]
struct RequestState {
    response_ns: NamespaceConst,
    response_type_name: MessageTypeConst,
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
    response_type_name: MessageTypeConst,
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

use crate::{DEFAULT_RECEIVER_ID, DEFAULT_SENDER_ID};

impl Config {
    pub async fn connect(self) -> Result<Client> {
        let conn = tls_connect(&self).await?;

        let (task_cmd_tx, task_cmd_rx) = tokio::sync::mpsc::channel(/* buffer: */ 32);

        let task = Task::new(conn, task_cmd_rx);

        let task_join_handle = tokio::spawn(task.main());

        let mut client = Client {
            task_join_handle: Some(task_join_handle),
            task_cmd_tx,
            next_request_id: AtomicI32::new(0),
            next_command_id: AtomicUsize::new(0),
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

    async fn connection_connect(&mut self, receiver: AppId) -> Result<()> {
        let payload_req = json_payload::connection::ConnectRequest {
            user_agent: json_payload::connection::USER_AGENT.to_string(),
        };
        self.json_send(payload_req, receiver).await?;
        Ok(())
    }

    fn response_from_dyn<Resp>(&self, payload_dyn: Box<PayloadDyn>)
    -> Result<Payload<Resp>>
    where Resp: ResponseInner
    {
        // let expected_namespace = Resp::CHANNEL_NAMESPACE;
        let expected_type = Resp::TYPE_NAME;

        if payload_dyn.typ != expected_type {
            bail!("Unexpected payload type\n\
                   request_id:    {rid:?}\n\
                   expected_type: {expected_type:?}\n\
                   type:          {typ:?}",
                  rid = payload_dyn.request_id,
                  typ = payload_dyn.typ);
        }

        Ok(Payload::<Resp> {
            request_id: payload_dyn.request_id,
            typ: payload_dyn.typ,
            inner: serde_json::from_value(payload_dyn.inner)?,
        })
    }

    async fn json_send<Req>(&mut self, req: Req, destination: AppId)
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

    async fn json_rpc<Req, Resp>(&mut self, req: Req, destination: AppId)
    -> Result<Payload<Resp>>
    where Req: RequestInner,
          Resp: ResponseInner
    {
        let (request_message, request_id) = self.cast_request_from_inner(req, destination)?;

        let cmd_type = TaskCommandType::CastRpc(Box::new(CastRpc {
            request_message,
            request_id,
            response_ns: Resp::CHANNEL_NAMESPACE,
            response_type_name: Resp::TYPE_NAME,
        }));

        let resp_dyn: Box<PayloadDyn> = self.task_cmd(cmd_type).await?;
        let resp: Payload<Resp> = self.response_from_dyn(resp_dyn)?;

        Ok(resp)
    }

    #[named]
    fn cast_request_from_inner<Req>(&self, req: Req, destination: AppId)
    -> Result<(CastMessage, RequestId)>
    where Req: RequestInner
    {
        let request_id = self.take_request_id();
        let payload = Payload::<Req> {
            request_id: Some(request_id),
            typ: Req::TYPE_NAME.to_string(),
            inner: req,
        };

        let payload_json = serde_json::to_string(&payload)?;

        // TODO: Take these as params or config.
        let source = DEFAULT_SENDER_ID.to_string();

        let namespace = Req::CHANNEL_NAMESPACE.to_string();

        tracing::trace!(target: function_path!(),
                        payload_json,
                        request_id,
                        request_type = payload.typ,
                        request_namespace = namespace,
                        source, destination,
                        "json payload");

        let request_message = CastMessage {
            namespace: Req::CHANNEL_NAMESPACE.to_string(),
            source,
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
    ) -> Task<S> {
        let task_cmd_rx = tokio_stream::wrappers::ReceiverStream::new(task_cmd_rx)
//            .map(TaskEvent::Cmd)
            ;

        let timeout_queue = DelayQueue::<RequestId>::with_capacity(4)
//            .map(TaskEvent::RpcTimeout)
            ;

        let cast_message_codec = CastMessageCodec;
        let conn_framed = tokio_util::codec::Framed::with_capacity(
            conn, cast_message_codec, DATA_BUFFER_LEN);

        let (conn_framed_sink, conn_framed_stream) = conn_framed.split();

        Task {
            //conn_framed,

            conn_framed_sink,
            conn_framed_stream,

            task_cmd_rx,
            timeout_queue,

            need_flush: false,
            requests_map: HashMap::new(),
        }
    }

    #[named]
    async fn main(self) -> Result<()> {
        // TODO: Timeouts for requests, send response and clean up.

        const FUNCTION_PATH: &str = function_path!();

        pin! {
            let this = self;
        }

        while let Some(event) = this.as_mut().take_next_event().await {
            tracing::trace!(target: FUNCTION_PATH,
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
                        tracing::info!(target: FUNCTION_PATH,
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
                        // TODO: Mark connection as dead on reconnect.
                        tracing::warn!(target: FUNCTION_PATH,
                                       ?err,
                                       "flush error");
                    }
                    this.need_flush = false;
                }
            }
        }

        tracing::info!(target: FUNCTION_PATH,
                       "shutdown on event stream closed");

        // TODO: cleanup? e.g. flush outputs, reset connections,
        // return errors to response channels?;

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

        // Tried in order.
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
        const FUNCTION_PATH: &str = function_path!();

        let deadline = tokio::time::Instant::now()
            .checked_add(RPC_TIMEOUT)
            .unwrap_or_else(|| panic!("{FUNCTION_PATH}: error calculating deadline"));

        let send_debug = format!("{send:#?}");

        let CastSend {
            request_message,
            request_id,
        } = *send;

        let command_id = &result_sender.command_id;

        tracing::debug!(target: FUNCTION_PATH,
                        ?deadline,
                        request_id,
                        command_id,
                        ?request_message,
                        "msg send");

        if let Err(err) = self.as_mut().send_raw(request_message, deadline).await {
            tracing::warn!(target: FUNCTION_PATH,
                           ?err,
                           send = send_debug,
                           request_id,
                           command_id,
                           "send_raw error");

            Self::respond_send(result_sender, Err(err));
            return;
        }

        Self::respond_send(result_sender, Ok(()));
    }

    #[named]
    async fn handle_rpc_cmd(mut self: Pin<&mut Self>,
                            rpc: Box<CastRpc>, result_sender: TaskCommandResultSender)
    {
        const FUNCTION_PATH: &str = function_path!();

        let deadline = tokio::time::Instant::now()
            .checked_add(RPC_TIMEOUT)
            .unwrap_or_else(|| panic!("{FUNCTION_PATH}: error calculating deadline"));

        let rpc_debug = format!("{rpc:#?}");

        let CastRpc {
            request_message,
            request_id,
            response_ns,
            response_type_name,
        } = *rpc;

        let command_id = &result_sender.command_id;

        tracing::debug!(target: FUNCTION_PATH,
                        ?deadline,
                        request_id,
                        command_id,
                        ?request_message,
                        response_ns,
                        response_type_name,
                        "rpc send");

        if let Err(err) = self.as_mut().send_raw(request_message, deadline).await {
            tracing::warn!(target: FUNCTION_PATH,
                           ?err,
                           rpc = rpc_debug,
                           request_id,
                           command_id,
                           response_ns,
                           response_type_name,
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
            response_type_name,
            result_sender,
        };

        self.as_mut().requests_map.insert(request_id, state);
    }

    async fn send_raw(self: Pin<&mut Self>, msg: CastMessage, deadline: tokio::time::Instant
    ) -> Result<()> {
        let mut proj = self.project();

        *proj.need_flush = true;

        // TODO: Don't block the main task when conn sink buffer is full.
        let fut = proj.conn_framed_sink.feed(msg);
        tokio::time::timeout_at(deadline, fut).await??;

        Ok(())
    }

    #[named]
    async fn handle_msg_read(mut self: Pin<&mut Self>, read_res: Result<CastMessage>) {
        const FUNCTION_PATH: &str = function_path!();

        let msg = match read_res {
            Err(err) => {
                tracing::warn!(target: FUNCTION_PATH,
                               ?err,
                               "Message read error");
                return;
            }
            Ok(msg) => msg,
        };

        tracing::trace!(target: FUNCTION_PATH,
                        ?msg,
                        "message read");

        let msg_ns = msg.namespace.as_str();
        if !JSON_NAMESPACES.contains(msg_ns) {
            tracing::warn!(target: FUNCTION_PATH,
                           msg_ns,
                           ?msg,
                           "message namespace not known");
            return;
        }
        let pd_json_str = match &msg.payload {
            CastMessagePayload::Binary(_b) => {
                tracing::warn!(target: FUNCTION_PATH,
                               msg_ns,
                               ?msg,
                               "binary message not known");
                return;
            },
            CastMessagePayload::String(s) => s.as_str(),
        };

        tracing::trace!(target: FUNCTION_PATH,
                        pd_json_str,
                        "message payload json");

        let pd: PayloadDyn = match serde_json::from_str(pd_json_str) {
            Err(err) => {
                tracing::warn!(target: FUNCTION_PATH,
                               ?err, ?msg,
                               "error deserializing json");
                return;
            },
            Ok(pd) => pd,
        };

        tracing::trace!(target: FUNCTION_PATH,
                        ?pd,
                        "message payload");

        let mut proj = self.as_mut().project();

        let Some(request_id) = pd.request_id else {
            tracing::warn!(target: FUNCTION_PATH,
                           ?msg,
                           "missing request_id in cast message payload");
            return;
        };

        let Some(request_state) = proj.requests_map.remove(&request_id) else {
            tracing::warn!(target: FUNCTION_PATH,
                           request_id, ?msg,
                           "missing request state");
            return;
        };

        if proj.timeout_queue.as_mut().try_remove(&request_state.delay_key).is_none() {
            tracing::warn!(target: FUNCTION_PATH,
                           ?request_state,
                           ?msg,
                           "timeout_queue missing expected delay key");
        }

        let result: Result<PayloadDyn> =
            if request_state.response_ns != msg_ns {
                Err(format_err!(
                    "{FUNCTION_PATH}: received reply message with unexpected namespace:\n\
                     _ request_id    = {request_id}\n\
                     _ expected_ns   = {expected_ns:?}\n\
                     _ msg_ns        = {msg_ns:?}\n\
                     _ request_state = {request_state:#?}",
                    expected_ns = request_state.response_ns))
            } else if request_state.response_type_name != pd.typ {
                Err(format_err!(
                    "{FUNCTION_PATH}: received reply message with unexpected type:\n\
                     _ request_id    = {request_id}\n\
                     _ expected_type = {expected_type:?}\n\
                     _ msg_type      = {pd_type:?}\n\
                     _ request_state = {request_state:#?}",
                    expected_type = request_state.response_type_name,
                    pd_type = pd.typ))
            } else {
                Ok(pd)
            };

        Self::respond_rpc(request_state.result_sender, result);
    }

    #[named]
    fn handle_rpc_timeout(mut self: Pin<&mut Self>, expired: DelayExpired<RequestId>) {
        const FUNCTION_PATH: &str = function_path!();

        let deadline = expired.deadline();
        let delay_key = expired.key();
        let request_id = expired.get_ref();

        let proj = self.as_mut().project();

        let Some(request_state) = proj.requests_map.remove(request_id) else {
            panic!("{FUNCTION_PATH}: missing request_state in requests_map\n\
                    request_id: {request_id}");
        };

        assert_eq!(delay_key, request_state.delay_key);

        tracing::warn!(target: FUNCTION_PATH,
                       ?expired,
                       ?deadline,
                       request_id,
                       ?request_state,
                       "rpc timeout");

        let err = format_err!("{FUNCTION_PATH}: RPC timeout\n\
                               _ request_id:    {request_id}\n\
                               _ deadline:      {deadline:?}\n\
                               _ expired:       {expired:#?}\n\
                               _ request_state: {request_state:#?}");
        Self::respond_rpc(request_state.result_sender,
                          Err(err));
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
