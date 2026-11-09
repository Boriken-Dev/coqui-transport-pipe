//! The listener, the stream and the message transport over tokio.
//!
//! Binding and connecting run the same checks as the blocking forms. These need a tokio runtime with
//! its I/O driver enabled: tokio panics otherwise. Streams have no timeouts of their own: wrap a call in
//! `tokio::time::timeout`, or use `coqui_peer::asynch`'s `_until` drivers.
#![forbid(unsafe_code)]

use alloc::vec::Vec;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io;
use std::time::Instant;

use coqui_peer::NonDefault;
use coqui_transport::asynch::{AsyncConnector, BoxFuture};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::deadline;
use crate::error::Error;
use crate::identity::OsIdentity;
use crate::name::PipeName;
use crate::options::{ConnectSettings, ListenerSettings};
use crate::sys;

/// How often a wait for a free instance or a place in the queue looks again.
const BUSY_POLL: core::time::Duration = core::time::Duration::from_millis(10);

#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer};

#[cfg(unix)]
#[derive(Debug)]
struct Waiting {
    socket: tokio::net::UnixListener,
    _bound: sys::Bound,
}

#[cfg(windows)]
#[derive(Debug)]
struct Waiting {
    instances: sys::Instances,
    pending: NamedPipeServer,
    spare: Option<NamedPipeServer>,
}

/// [`Listener`](crate::Listener) over tokio, with the same access control. Dropping it ends listening.
#[derive(Debug)]
pub struct Listener {
    waiting: Waiting,
    name: PipeName,
    non_defaults: Vec<NonDefault>,
    not_applied: Vec<NonDefault>,
}

impl Listener {
    /// Binds `name` with every setting at its default: `crate::Listener::builder(name).build_async()`.
    ///
    /// # Errors
    ///
    /// As [`Listener::bind`](crate::Listener::bind).
    pub fn bind(name: &PipeName) -> Result<Self, Error> {
        Self::with_settings(name, ListenerSettings::new())
    }

    pub(crate) fn with_settings(name: &PipeName, options: ListenerSettings) -> Result<Self, Error> {
        options.check()?;
        options.log_non_defaults();
        let non_defaults = options.non_defaults();
        let not_applied = options.not_applied();
        Ok(Self {
            waiting: Waiting::bind(name, &options)?,
            name: name.clone(),
            non_defaults,
            not_applied,
        })
    }

    /// Waits for the next connection. Cancelling this loses nothing.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when the operating system fails.
    pub async fn accept(&mut self) -> Result<Stream, Error> {
        self.waiting.accept().await
    }

    /// The name this listens on.
    #[must_use]
    pub fn name(&self) -> &PipeName {
        &self.name
    }

    /// Every setting this was bound with that differs from its default and means something on this platform.
    #[must_use]
    pub fn non_defaults(&self) -> &[NonDefault] {
        &self.non_defaults
    }

    /// Every setting this was bound with that this platform could not honour, as
    /// [`ListenerSettings::not_applied`].
    #[must_use]
    pub fn not_applied(&self) -> &[NonDefault] {
        &self.not_applied
    }
}

#[cfg(unix)]
impl Waiting {
    fn bind(name: &PipeName, options: &ListenerSettings) -> Result<Self, Error> {
        let (socket, bound) = sys::bind_socket(name, options)?;
        Ok(Self {
            socket: tokio::net::UnixListener::from_std(socket)?,
            _bound: bound,
        })
    }

    async fn accept(&mut self) -> Result<Stream, Error> {
        let (socket, _) = self.socket.accept().await?;
        sys::no_sigpipe(&socket)?;
        Ok(Stream {
            inner: Inner::Socket(socket),
        })
    }
}

#[cfg(windows)]
impl Waiting {
    fn bind(name: &PipeName, options: &ListenerSettings) -> Result<Self, Error> {
        let instances = sys::Instances::new(name, options)?;
        let (pending, spare) = instances.first_two()?;
        Ok(Self {
            instances,
            pending: sys::server_from(pending)?,
            spare: spare.map(sys::server_from).transpose()?,
        })
    }

    async fn accept(&mut self) -> Result<Stream, Error> {
        while self.spare.is_none() {
            match self.instances.create(false)? {
                Some(instance) => self.spare = Some(sys::server_from(instance)?),
                None => tokio::time::sleep(BUSY_POLL).await,
            }
        }
        if let Err(error) = self.pending.connect().await
            && !sys::is_client_gone(&error)
        {
            return Err(Error::Io(error));
        }
        let next = self
            .spare
            .take()
            .expect("the loop above leaves a spare instance");
        let connected = core::mem::replace(&mut self.pending, next);
        Ok(Stream {
            inner: Inner::Server(connected),
        })
    }
}

#[derive(Debug)]
enum Inner {
    #[cfg(unix)]
    Socket(tokio::net::UnixStream),
    #[cfg(windows)]
    Server(NamedPipeServer),
    #[cfg(windows)]
    Client(NamedPipeClient),
}

/// [`Stream`](crate::Stream) over tokio: `AsyncRead + AsyncWrite`, so every driver of
/// `coqui_peer::asynch` works on it.
///
/// Dropping it ends the connection, with what was written still readable by the peer.
/// `poll_shutdown` ends the writing direction on Unix; on Windows a pipe has no half-close and it
/// does nothing.
#[derive(Debug)]
pub struct Stream {
    inner: Inner,
}

impl Stream {
    /// Connects to `name` with every setting at its default: `Stream::builder(name).build_async()`.
    ///
    /// # Errors
    ///
    /// As [`Stream::connect`](crate::Stream::connect).
    pub async fn connect(name: &PipeName) -> Result<Self, Error> {
        Self::with_settings(name, ConnectSettings::new()).await
    }

    pub(crate) async fn with_settings(
        name: &PipeName,
        options: ConnectSettings,
    ) -> Result<Self, Error> {
        options.check()?;
        options.log_non_defaults();
        let deadline = deadline::after(options.timeout);
        loop {
            if let Some(inner) = Inner::try_connect(name, &options, deadline)? {
                return Ok(Self { inner });
            }
            if deadline::passed(deadline) {
                return Err(Error::TimedOut);
            }
            tokio::time::sleep(BUSY_POLL).await;
        }
    }

    /// Who the operating system says is at the other end: see [`OsIdentity`].
    ///
    /// # Errors
    ///
    /// When the operating system does not answer at all.
    pub fn peer(&self) -> io::Result<OsIdentity> {
        match &self.inner {
            #[cfg(unix)]
            Inner::Socket(socket) => sys::peer_of(socket),
            #[cfg(windows)]
            Inner::Server(pipe) => Ok(sys::peer_of_tokio(pipe, true)),
            #[cfg(windows)]
            Inner::Client(pipe) => Ok(sys::peer_of_tokio(pipe, false)),
        }
    }
}

impl Inner {
    #[cfg(unix)]
    fn try_connect(
        name: &PipeName,
        _options: &ConnectSettings,
        deadline: Option<Instant>,
    ) -> Result<Option<Self>, Error> {
        match sys::try_connect(name.path(), deadline)? {
            Some(socket) => {
                socket.set_nonblocking(true)?;
                sys::no_sigpipe(&socket)?;
                Ok(Some(Self::Socket(tokio::net::UnixStream::from_std(
                    socket,
                )?)))
            }
            None => Ok(None),
        }
    }

    #[cfg(windows)]
    fn try_connect(
        name: &PipeName,
        options: &ConnectSettings,
        _deadline: Option<Instant>,
    ) -> Result<Option<Self>, Error> {
        let path = sys::path_of(name)?;
        match sys::open(&path, options.impersonation)? {
            Some(handle) => Ok(Some(Self::Client(sys::client_from(handle)?))),
            None => Ok(None),
        }
    }
}

macro_rules! dispatch {
    ($self:ident, $inner:ident => $call:expr) => {
        match &mut $self.get_mut().inner {
            #[cfg(unix)]
            Inner::Socket($inner) => $call,
            #[cfg(windows)]
            Inner::Server($inner) => $call,
            #[cfg(windows)]
            Inner::Client($inner) => $call,
        }
    };
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        dispatch!(self, inner => Pin::new(inner).poll_read(context, buffer))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().inner {
            // As the blocking stream writes: a peer that left is an error, never SIGPIPE.
            #[cfg(unix)]
            Inner::Socket(socket) => loop {
                core::task::ready!(socket.poll_write_ready(context))?;
                match socket.try_io(tokio::io::Interest::WRITABLE, || {
                    sys::send(&*socket, buffer)
                }) {
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    other => return Poll::Ready(other),
                }
            },
            #[cfg(windows)]
            Inner::Server(pipe) => Pin::new(pipe).poll_write(context, buffer),
            #[cfg(windows)]
            Inner::Client(pipe) => Pin::new(pipe).poll_write(context, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        dispatch!(self, inner => Pin::new(inner).poll_flush(context))
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        dispatch!(self, inner => Pin::new(inner).poll_shutdown(context))
    }
}

/// Connects to one [`PipeName`] with [`ConnectSettings`], as a `coqui_transport::asynch::AsyncConnector`.
///
/// `connect`'s timeout replaces the settings' own. The listener has no such form: `AsyncListener::accept`
/// takes `&self`, and [`Listener::accept`] needs `&mut self`.
#[derive(Clone, Debug)]
#[must_use]
pub struct Connector {
    inner: crate::Connector,
}

impl Connector {
    /// A connector for `name` with every setting at its default.
    pub fn new(name: PipeName) -> Self {
        Self {
            inner: crate::Connector::new(name),
        }
    }

    /// The settings to connect with.
    pub fn settings(mut self, settings: ConnectSettings) -> Self {
        self.inner = self.inner.settings(settings);
        self
    }

    /// The name this connects to.
    #[must_use]
    pub fn name(&self) -> &PipeName {
        self.inner.name()
    }
}

impl AsyncConnector for Connector {
    type Connection = Stream;
    type Error = Error;

    fn connect(&self, timeout: core::time::Duration) -> BoxFuture<'_, Result<Stream, Error>> {
        Box::pin(Stream::with_settings(
            &self.inner.name,
            self.inner.settings.within(timeout),
        ))
    }
}
