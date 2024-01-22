use anyhow::{bail, format_err};
use crate::{
    cast::proxies,
    ChannelMessage,
    json_payload::{self, Payload, PayloadDyn, RequestInner, ResponseInner},
    message::{
        CastMessage,
        CastMessagePayload,
    },
    types::{Namespace, RequestId},
};
use std::{
    any::{self, Any},
    collections::HashMap,
    sync::{Arc, atomic::{AtomicI32, Ordering}},
};
use tokio::io::{AsyncRead, AsyncWrite};

pub use anyhow::Error;
pub type Result<T, E = Error> = std::result::Result<T, E>;


pub struct Client {
    /// Some(_) until `.close()` is called.
    task_join_handle: Option<tokio::task::JoinHandle<Result<()>>>,

    task_cmd_tx: tokio::sync::mpsc::Sender::<TaskCommand>,

    next_request_id: AtomicI32,
}

pub struct Config {
    // pub _host: String,
    // pub _port: u16,
}

struct Task<S: TokioAsyncStream> {
    stream: S,
    task_cmd_rx: tokio::sync::mpsc::Receiver::<TaskCommand>,

    requests_map: HashMap<RequestId, RequestState>,
}

struct RequestState {}

struct TaskCommand {
    result_tx: tokio::sync::oneshot::Sender::<TaskCommandResult>,
    command: TaskCommandType,
}

enum TaskCommandType {
    CastRpc(Box<CastRpc>),
    Shutdown,
}

struct CastRpc {
    request_message: CastMessage,
    request_id: RequestId,
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

const TASK_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1_000);

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

        tokio::time::timeout(TASK_COMMAND_TIMEOUT, join_fut).await???;

        Ok(())
    }
}

/// Internals.
impl Client {
    fn take_request_id(&self) -> i32 {
        self.next_request_id.fetch_add(1, Ordering::SeqCst)
    }

    async fn init(&mut self) -> Result<()> {
        // TODO?
        Ok(())
    }

    fn response_from_dyn<Resp>(&self, payload_dyn: Box<PayloadDyn>)
    -> Result<Payload<Resp>>
    where Resp: ResponseInner
    {
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
        }));

        let resp_dyn: Box<PayloadDyn> = self.task_cmd(cmd_type).await?;
        let resp: Payload<Resp> = self.response_from_dyn(resp_dyn)?;

        Ok(resp)
    }

    async fn task_cmd<R>(&mut self, cmd_type: TaskCommandType)
    -> Result<Box<R>>
    where R: Any + Send + Sync
    {
        let (result_tx, result_rx) = tokio::sync::oneshot::channel::<TaskCommandResult>();

        let cmd = TaskCommand {
            result_tx,
            command: cmd_type,
        };
        self.task_cmd_tx.send_timeout(cmd, TASK_COMMAND_TIMEOUT).await?;

        let response: TaskResponseBox =
            tokio::time::timeout(TASK_COMMAND_TIMEOUT, result_rx).await???;

        response.downcast::<R>()
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

impl<S: TokioAsyncStream> Task<S> {
    async fn main(mut self) -> Result<()> {
        todo("also read stream messages");

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

        Ok(())
    }

    async fn send_raw(&mut self, msg: &CastMessage) -> Result<()> {
        todo!();
    }

    fn respond<R>(&self, cmd: TaskCommand, result: Result<R>)
    where R: Any + Send + Sync
    {
        let boxed = result.map(|response| TaskResponseBox::new(response));

        if let Err(_) = cmd.result_tx.send(boxed) {
            tracing::warn!("Task::respond: result channel dropped");
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
