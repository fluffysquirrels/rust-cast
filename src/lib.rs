// #![deny(warnings)]

#[macro_use]
mod util;
pub use util::named;

pub mod async_client;
pub mod cast;
pub mod channels;
pub mod errors;
mod message;
pub mod message_manager;
pub mod payload;
mod types;
mod utils;

// use std::{borrow::Cow, net::TcpStream, sync::Arc};
use std::{net::TcpStream};


use channels::{
    connection::{ConnectionChannel, ConnectionResponse},
    heartbeat::{HeartbeatChannel, HeartbeatResponse},
    media::{MediaChannel, MediaResponse},
    receiver::{ReceiverChannel, ReceiverResponse},
};

// use errors::Error;

use message_manager::{CastMessage, MessageManager};

// use rustls::{ClientConnection, OwnedTrustAnchor, RootCertStore, StreamOwned};
use rustls::{ClientConnection, StreamOwned};


const DEFAULT_SENDER_ID: &str = "sender-0";
const DEFAULT_RECEIVER_ID: &str = "receiver-0";

#[cfg(feature = "thread_safe")]
type Lrc<T> = std::sync::Arc<T>;
#[cfg(not(feature = "thread_safe"))]
type Lrc<T> = std::rc::Rc<T>;


/// Supported channel message types.
#[derive(Clone, Debug)]
pub enum ChannelMessage {
    /// Message to be processed by `ConnectionChannel`.
    Connection(ConnectionResponse),
    /// Message to be processed by `HeartbeatChannel`.
    Heartbeat(HeartbeatResponse),
    /// Message to be processed by `MediaChannel`.
    Media(MediaResponse),
    /// Message to be processed by `ReceiverChannel`.
    Receiver(ReceiverResponse),
    /// Raw message is returned when built-in channels can't process it (e.g. because of unknown
    /// `namespace`).
    Raw(CastMessage),
}

/// Structure that manages connection to a cast device.
pub struct CastDevice<'a> {
    message_manager: Lrc<MessageManager<StreamOwned<ClientConnection, TcpStream>>>,

    /// Channel that manages connection responses/requests.
    pub connection: ConnectionChannel<'a, StreamOwned<ClientConnection, TcpStream>>,

    /// Channel that allows connection to stay alive (via ping-pong requests/responses).
    pub heartbeat: HeartbeatChannel<'a, StreamOwned<ClientConnection, TcpStream>>,

    /// Channel that manages various media stuff.
    pub media: MediaChannel<'a, StreamOwned<ClientConnection, TcpStream>>,

    /// Channel that manages receiving platform (e.g. Chromecast).
    pub receiver: ReceiverChannel<'a, StreamOwned<ClientConnection, TcpStream>>,
}


#[cfg(test)]
pub(crate) mod tests {
    #[test]
    #[cfg(feature = "thread_safe")]
    fn test_thread_safe() {
        use crate::CastDevice;

        fn is_sync<T: Sync>() {}
        fn is_send<T: Send>() {}

        is_sync::<CastDevice>();
        is_send::<CastDevice>();
    }
}


#[cfg(any())] // Disabled
mod boop {
struct NoCertificateVerification {}

impl rustls::client::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

impl<'a> CastDevice<'a> {
    /// Connects to the cast device using host name and port.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rust_cast::CastDevice;
    ///
    /// let device = CastDevice::connect("192.168.1.2", 8009)?;
    /// # Ok::<(), rust_cast::errors::Error>(())
    /// ```
    ///
    /// # Arguments
    ///
    /// * `host` - Cast device host name.
    /// * `port` - Cast device port number.
    ///
    /// # Errors
    ///
    /// This method may fail if connection to Cast device can't be established for some reason
    /// (e.g. wrong host name or port).
    ///
    /// # Return value
    ///
    /// Instance of `CastDevice` that allows you to manage connection.
    pub fn connect<S>(host: S, port: u16) -> Result<CastDevice<'a>, Error>
    where
        S: Into<Cow<'a, str>>,
    {
        let host = host.into();

        log::debug!(
            "Establishing connection with cast device at {}:{}...",
            host,
            port
        );

        let mut root_store = RootCertStore::empty();
        root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));

        let mut config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        // Enable early data.
        config.enable_early_data = true;
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(NoCertificateVerification {}));
        let config = Arc::new(config);

        let server_name = host.as_ref().try_into().expect("invalid DNS name");
        let conn = rustls::ClientConnection::new(Arc::clone(&config), server_name).unwrap();

        let tcp_stream = TcpStream::connect((host.as_ref(), port))?;

        let stream = rustls::StreamOwned::new(conn, tcp_stream);

        CastDevice::connect_to_device(stream)
    }

    /// Connects to the cast device using host name and port _without_ host verification. Use on
    /// your own risk!
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rust_cast::CastDevice;
    ///
    /// let device = CastDevice::connect_without_host_verification("192.168.1.2", 8009)?;
    /// # Ok::<(), rust_cast::errors::Error>(())
    /// ```
    ///
    /// # Arguments
    ///
    /// * `host` - Cast device host name.
    /// * `port` - Cast device port number.
    ///
    /// # Errors
    ///
    /// This method may fail if connection to Cast device can't be established for some reason
    /// (e.g. wrong host name or port).
    ///
    /// # Return value
    ///
    /// Instance of `CastDevice` that allows you to manage connection.
    pub fn connect_without_host_verification<S>(host: S, port: u16) -> Result<CastDevice<'a>, Error>
    where
        S: Into<Cow<'a, str>>,
    {
        let host = host.into();

        log::debug!(
            "Establishing non-verified connection with cast device at {}:{}...",
            host,
            port
        );

        let tcp_stream = TcpStream::connect((host.as_ref(), port))?;

        log::debug!(
            "Connection with {}:{} successfully established.",
            host,
            port
        );

        let mut root_store = RootCertStore::empty();
        root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));

        let mut config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        // Enable early data.
        config.enable_early_data = true;
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(NoCertificateVerification {}));
        let config = Arc::new(config);

        let server_name = host.as_ref().try_into().expect("invalid DNS name");
        let conn = rustls::ClientConnection::new(Arc::clone(&config), server_name).unwrap();

        let tls = rustls::StreamOwned::new(conn, tcp_stream);

        CastDevice::connect_to_device(tls)
    }

    /// Waits for any message returned by cast device (e.g. Chromecast) and returns its parsed
    /// version.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rust_cast::ChannelMessage;
    ///
    /// # use rust_cast::CastDevice;
    /// # let cast_device = CastDevice::connect_without_host_verification("192.168.1.2", 8009)?;
    ///
    /// match cast_device.receive() {
    ///     Ok(ChannelMessage::Connection(res)) => log::debug!("Connection message: {:?}", res),
    ///     Ok(ChannelMessage::Heartbeat(_)) => cast_device.heartbeat.pong()?,
    ///     Ok(_) => {},
    ///     Err(err) => log::error!("Error occurred while receiving message {}", err)
    /// }
    /// # Ok::<(), rust_cast::errors::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Usually fails if message returned by device can't be parsed.
    ///
    /// # Returned values
    ///
    /// Parsed channel message.
    pub fn receive(&self) -> Result<ChannelMessage, Error> {
        let cast_message = self.message_manager.receive()?;

        if self.connection.can_handle(&cast_message) {
            return Ok(ChannelMessage::Connection(
                self.connection.parse(&cast_message)?,
            ));
        }

        if self.heartbeat.can_handle(&cast_message) {
            return Ok(ChannelMessage::Heartbeat(
                self.heartbeat.parse(&cast_message)?,
            ));
        }

        if self.media.can_handle(&cast_message) {
            return Ok(ChannelMessage::Media(self.media.parse(&cast_message)?));
        }

        if self.receiver.can_handle(&cast_message) {
            return Ok(ChannelMessage::Receiver(
                self.receiver.parse(&cast_message)?,
            ));
        }

        Ok(ChannelMessage::Raw(cast_message))
    }

    /// Connects to the cast device using provided ssl stream.
    ///
    /// # Arguments
    ///
    /// * `ssl_stream` - SSL Stream for the TCP connection established with the device.
    ///
    /// # Return value
    ///
    /// Instance of `CastDevice` that allows you to manage connection.
    fn connect_to_device(
        ssl_stream: StreamOwned<ClientConnection, TcpStream>,
    ) -> Result<CastDevice<'a>, Error> {
        let message_manager_rc = Lrc::new(MessageManager::new(ssl_stream));

        let heartbeat = HeartbeatChannel::new(
            DEFAULT_SENDER_ID,
            DEFAULT_RECEIVER_ID,
            Lrc::clone(&message_manager_rc),
        );
        let connection = ConnectionChannel::new(DEFAULT_SENDER_ID, Lrc::clone(&message_manager_rc));
        let receiver = ReceiverChannel::new(
            DEFAULT_SENDER_ID,
            DEFAULT_RECEIVER_ID,
            Lrc::clone(&message_manager_rc),
        );
        let media = MediaChannel::new(DEFAULT_SENDER_ID, Lrc::clone(&message_manager_rc));

        Ok(CastDevice {
            message_manager: message_manager_rc,
            heartbeat,
            connection,
            receiver,
            media,
        })
    }
}

}
