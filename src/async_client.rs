use anyhow::{bail, format_err};
use bytes::BytesMut;
use crate::{
    cast::proxies,
    json_payload::{self, Payload, PayloadDyn, RequestInner, ResponseInner},
    message::{
        CastMessage,
        CastMessagePayload,
    },
    types::{MessageType, MessageTypeConst, Namespace, NamespaceConst, RequestId},
    util::named,
};
use futures::StreamExt;
use futures_concurrency::stream::Merge;
use once_cell::sync::Lazy;
use std::{
    any::{self, Any},
    collections::{HashMap, HashSet},
    sync::{Arc, atomic::{AtomicI32, AtomicUsize, Ordering}},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    pin,
};
use tokio_util::{
    codec,
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
    // pub _host: String,
    // pub _port: u16,
}

struct Task<S: TokioAsyncStream> {
    stream: S,
    task_cmd_rx: tokio::sync::mpsc::Receiver::<TaskCommand>,

    requests_map: HashMap<RequestId, Box<RequestState>>,
}

struct RequestState {
    response_ns: NamespaceConst,
    response_type_name: MessageTypeConst,
    delay_key: DelayKey,
    deadline: tokio::time::Instant,
    result_sender: TaskCommandResultSender,
}

struct TaskCommandResultSender {
    command_id: CommandId,
    result_tx: tokio::sync::oneshot::Sender::<TaskCommandResult>,
}

struct TaskCommand {
    command: TaskCommandType,
    result_sender: TaskCommandResultSender,
}

enum TaskCommandType {
    CastRpc(Box<CastRpc>),
    Shutdown,
}

struct CastRpc {
    request_message: CastMessage,
    request_id: RequestId,
    response_ns: NamespaceConst,
    response_type_name: MessageTypeConst,
}

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

impl Config {
    pub async fn connect(self) -> Result<Client> {
        let stream = tls_connect(&self).await?;

        let (task_cmd_tx, task_cmd_rx) = tokio::sync::mpsc::channel(/* buffer: */ 32);

        let task = Task {
            stream,
            task_cmd_rx,
            requests_map: HashMap::new(),
        };

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
/*
        let request_id = self.take_request_id();
        let payload = serde_json::to_string(&crate::cast::proxies::receiver::GetStatusRequest {
            typ: crate::channels::receiver::MESSAGE_TYPE_GET_STATUS.to_string(),
            request_id,
        })?;

        let source = crate::DEFAULT_SENDER_ID.to_string();
        let destination = crate::DEFAULT_RECEIVER_ID.to_string();

        let msg = RawMessage {
            namespace: crate::channels::receiver::CHANNEL_NAMESPACE.to_string(),
            source, destination,
            payload: RawMessagePayload::String(payload),
        };

        let raw_response = self.rpc(request_id, msg).await?;
        let response = crate::channels::receiver::ReceiverChannel::parse_static(&raw_response)?;
*/

        let payload_req = json_payload::receiver::StatusRequest {};

        let resp: Payload<json_payload::receiver::StatusResponse>
            = self.json_rpc(payload_req).await?;

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
        // TODO?
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
                   request_id: {rid}\n\
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

    async fn json_rpc<Req, Resp>(&mut self, req: Req)
    -> Result<Payload<Resp>>
    where Req: RequestInner,
          Resp: ResponseInner
    {
        let request_id = self.take_request_id();
        let payload = Payload::<Req> {
            request_id,
            typ: Req::TYPE_NAME.to_string(),
            inner: req,
        };

        let payload_json = serde_json::to_string(&payload)?;

        let source = crate::DEFAULT_SENDER_ID.to_string();
        let destination = crate::DEFAULT_RECEIVER_ID.to_string();

        let request_message = CastMessage {
            namespace: Req::CHANNEL_NAMESPACE.to_string(),
            source,
            destination,
            payload: payload_json.into(),
        };

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
            TaskCommandType::Shutdown => LOCAL_TASK_COMMAND_TIMEOUT,
            TaskCommandType::CastRpc(_) => RPC_TIMEOUT,
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

async fn tls_connect(config: &Config)
-> Result<impl TokioAsyncStream>
{
    // TODO: mdns service discovery
    let ip = std::net::IpAddr::from([192, 168, 0, 144]);
    let port = 8009_u16;

    let mut config = rustls::ClientConfig::builder()
        .dangerous().with_custom_certificate_verifier(Arc::new(
            crate::util::rustls::danger::NoCertificateVerification::new_ring()))
        .with_no_client_auth();

    config.enable_early_data = true;
    let config = Arc::new(config);

    let connector = tokio_rustls::TlsConnector::from(config);

    let ip_rustls = rustls::pki_types::IpAddr::from(ip);
    let domain = rustls::pki_types::ServerName::IpAddress(ip_rustls);

    tracing::debug!(
        %ip, port,
        "Connecting to cast device...",
    );

    let tcp_stream = tokio::net::TcpStream::connect((ip, port)).await?;

    tracing::debug!(
        %ip, port,
        "TcpStream connected",
    );

    let tls_stream = connector.connect(domain, tcp_stream).await?;

    tracing::debug!(
        %ip, port,
        "TlsStream connected to cast device",
    );

    Ok(tls_stream)
}

enum TaskEvent {
    Cmd(TaskCommand),
    MessageRead(Result<Box<CastMessage>>),
    RpcTimeout(DelayExpired<RequestId>),
}

impl<S: TokioAsyncStream> Task<S> {
    pub fn new(
        stream: S,
        task_cmd_rx: tokio::sync::mpsc::Receiver::<TaskCommand>,
    ) -> Task<S> {
        Task {
            stream,
            task_cmd_rx,
            requests_map: HashMap::new(),
        }
    }

    async fn main(mut self) -> Result<()> {
        // TODO: Timeouts for requests, send response and clean up.

        // TODO: move these streams into struct Task?
        let cmd_stream = tokio_stream::wrappers::ReceiverStream::new(self.task_cmd_rx)
            .map(TaskEvent::Cmd);

        let rpc_timeout_stream = DelayQueue::<RequestId>::with_capacity(4)
            .map(TaskEvent::RpcTimeout);

        let cast_message_codec = CastMessageCodec;
        let data_framed = tokio_util::codec::Framed::with_capacity(
            self.stream, cast_message_codec, DATA_BUFFER_LEN);

        let (data_sink, data_stream) = data_framed.split();

        let data_stream = data_stream.map(TaskEvent::MessageRead);

        pin! {
            let event_stream /* : impl Stream<Item = TaskEvent> */
                = (cmd_stream, data_stream, rpc_timeout_stream).merge();
        }

        while let Some(event) = event_stream.next().await {
            match event {
                TaskEvent::Cmd(cmd) => match cmd.command {
                    TaskCommandType::CastRpc(rpc) => {
                        self.handle_rpc_cmd(rpc, cmd.result_sender).await;
                    },
                    TaskCommandType::Shutdown => {
                        todo!();
                    },
                },

                TaskEvent::MessageRead(read_res) => {
                    self.handle_msg_read(read_res).await;
                },

                TaskEvent::RpcTimeout(expired) => {
                    let deadline = expired.deadline();
                    let delay_key = expired.key();
                    let request_id = expired.into_inner();
                    todo("get state by request_id, assert delay_key\n\
                          log all\n\
                          reply to Client");
                    todo!();
                }
            }
        }

        /*
        while let Some(cmd) = self.task_cmd_rx.recv().await {
            let cmd: TaskCommand = cmd;

            match cmd.command {
                TaskCommandType::Shutdown => {
                    self.respond(cmd, Ok(()));
                    break;
                },

                TaskCommandType::CastRpc(rpc) => {
                    let state = RequestState {};
                    self.requests_map.insert(rpc.request_id, state);
                    if let Err(err) = self.send_raw(&rpc.request_message).await {
                        todo();
                    }
                }
            }
        }

        todo("when new message", || {
            todo("parse, dispatch to request's response oneshot channel");
        });
         */

        Ok(())
    }

    #[named]
    async fn handle_rpc_cmd(&mut self, rpc: Box<CastRpc>, result_sender: TaskCommandResultSender)
    {
        let deadline = tokio::time::Instant::now()
            .checked_add(RPC_TIMEOUT)
            .unwrap_or_else(|| panic!("{function_path}: error calculating deadline",
                                      function_path = function_path!()));

        let delay_key = delay_queue.insert_at(rpc.request_id, deadline);

        let state = Box::new(RequestState {
            deadline,
            delay_key,

            response_ns: rpc.response_ns,
            response_type_name: rpc.response_type_name,
            result_sender,
        });
        self.requests_map.insert(rpc.request_id, state);
        if let Err(err) = self.send_raw(&rpc.request_message).await {
            todo!("log");
        }
    }

    async fn send_raw(&mut self, msg: &CastMessage) -> Result<()> {
        todo!();
    }

    async fn handle_msg_read(&mut self, read_res: Result<Box<CastMessage>>) {
        let msg = match read_res {
            Err(err) => {
                todo!("log error");
                return;
            }
            Ok(msg) => msg,
        };

        let msg_ns = msg.namespace.as_str();
        if !JSON_NAMESPACES.contains(msg_ns) {
            todo!("log");
            return;
        }
        let pd_json_str = match &msg.payload {
            CastMessagePayload::Binary(b) => {
                todo!("error");
                return;
            },
            CastMessagePayload::String(s) => s.as_str(),
        };

        let pd: Box::<PayloadDyn> = match serde_json::from_str(pd_json_str) {
            Err(err) => {
                todo!("error");
                return;
            }
            Ok(pd) => pd,
        };

        let request_id = pd.request_id;
        let Some(request_state) = self.requests_map.get(&request_id) else {
            todo!("error");
            return;
        };

        todo!("from here, return response to Client via channel");

        if delay_queue.try_remove(request_state.delay_key).is_none() {
            tracing::warn!(todo);
        }

        let result =
            if request_state.response_ns != msg_ns {
                todo!("error");
                Err(todo)
            } else if request_state.response_type_name != pd.typ {
                todo!("error");
                Err(todo)
            } else {
                Ok(pd)
            };

        self.respond(request_state.result_sender, result);
    }

    fn respond<R>(&self, result_sender: TaskCommandResultSender,
                  result: Result<R>)
    where R: Any + Send + Sync
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
            Err(err) =>
                tracing::warn!(
                    command_id,
                    result_variant,
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
             .map_err(|err| format_err!("Command response type didn't match expected\n\
                                         expected type: {expected:?}\n\
                                         type:          {ty:?}",
                                        expected = any::type_name::<R>(),
                                        ty       = type_name))
    }
}

impl codec::Encoder<Box<CastMessage>> for CastMessageCodec {
    type Error = Error;

    fn encode(
        &mut self,
        msg: Box<CastMessage>,
        dst: &mut BytesMut
    ) -> Result<()>
    {
        todo!();

        Ok(())
    }
}

impl codec::Decoder for CastMessageCodec {
    type Item = Box<CastMessage>;
    type Error = Error;

    fn decode(
        &mut self,
        src: &mut BytesMut
    ) -> Result<Option<Box<CastMessage>>>
    {
        todo!();
    }
}
