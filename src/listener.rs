//! A listener, on either platform.
#![forbid(unsafe_code)]

use alloc::vec::Vec;
use core::time::Duration;

use coqui_peer::NonDefault;

use crate::error::Error;
use crate::name::PipeName;
use crate::options::{ListenerBuilder, ListenerSettings};
use crate::stream::Stream;
use crate::sys;

/// Listens on a name and accepts connections, with the operating system's access control on.
///
/// On Windows the pipe's descriptor gives the current user alone access, the bind fails if the name
/// exists, and clients on other hosts are refused. On Unix the socket is `0600` from the moment its path
/// exists, the directories on its way can be written only by its owner, and a socket left behind is
/// removed only when nothing listens on it. Each can be loosened on the [`ListenerBuilder`], and is then
/// listed and logged.
///
/// Dropping it ends listening. A Unix socket's path goes with it; its `<path>.lock` stays.
#[derive(Debug)]
pub struct Listener {
    inner: sys::Listener,
    name: PipeName,
    non_defaults: Vec<NonDefault>,
    not_applied: Vec<NonDefault>,
}

impl Listener {
    /// Binds `name` with every setting at its default: `Listener::builder(name).build()`.
    ///
    /// # Errors
    ///
    /// [`Error::NameTaken`] when something else holds the name. On Unix [`Error::NotASocket`],
    /// [`Error::SharedDirectory`], [`Error::ForeignDirectory`]. [`Error::InvalidName`] for a name the
    /// system refuses, [`Error::Io`] for the rest.
    pub fn bind(name: &PipeName) -> Result<Self, Error> {
        Self::with_settings(name, ListenerSettings::new())
    }

    /// A builder for a bind of `name`, with every setting at its default.
    pub fn builder(name: &PipeName) -> ListenerBuilder {
        ListenerBuilder::new(name)
    }

    pub(crate) fn with_settings(name: &PipeName, options: ListenerSettings) -> Result<Self, Error> {
        options.check()?;
        options.log_non_defaults();
        let non_defaults = options.non_defaults();
        let not_applied = options.not_applied();
        Ok(Self {
            inner: sys::Listener::bind(name, &options)?,
            name: name.clone(),
            non_defaults,
            not_applied,
        })
    }

    /// Waits for the next connection.
    ///
    /// # Errors
    ///
    /// When the operating system fails. [`Error::Io`], or on Windows [`Error::InvalidOptions`] never: the
    /// wait has no bound.
    pub fn accept(&self) -> Result<Stream, Error> {
        self.accept_timeout(None)
    }

    /// Waits for the next connection for at most `timeout`; `None` waits forever. A wait that ends
    /// loses nothing, and the listener goes on. On Windows callers on one listener wait in turn.
    ///
    /// # Errors
    ///
    /// [`Error::TimedOut`], or [`Error::Io`].
    pub fn accept_timeout(&self, timeout: Option<Duration>) -> Result<Stream, Error> {
        Ok(Stream::from_sys(self.inner.accept(timeout)?))
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
