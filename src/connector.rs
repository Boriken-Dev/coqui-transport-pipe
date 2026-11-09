//! A name and settings to connect to, as a `coqui_transport::Connector`, and the listener as a
//! `coqui_transport::Listener`.
#![forbid(unsafe_code)]

use core::time::Duration;

use crate::error::Error;
use crate::listener::Listener;
use crate::name::PipeName;
use crate::options::ConnectSettings;
use crate::stream::Stream;

/// Connects to one [`PipeName`] with [`ConnectSettings`].
///
/// As a [`coqui_transport::Connector`], `connect`'s timeout replaces the settings' own, and nothing
/// proves who listens: see [`Stream::connect`].
#[derive(Clone, Debug)]
#[must_use]
pub struct Connector {
    pub(crate) name: PipeName,
    pub(crate) settings: ConnectSettings,
}

impl Connector {
    /// A connector for `name` with every setting at its default.
    pub fn new(name: PipeName) -> Self {
        Self {
            name,
            settings: ConnectSettings::new(),
        }
    }

    /// The settings to connect with.
    pub fn settings(mut self, settings: ConnectSettings) -> Self {
        self.settings = settings;
        self
    }

    /// The name this connects to.
    #[must_use]
    pub fn name(&self) -> &PipeName {
        &self.name
    }
}

impl coqui_transport::Connector for Connector {
    type Connection = Stream;
    type Error = Error;

    fn connect(&self, timeout: Duration) -> Result<Stream, Error> {
        Stream::with_settings(&self.name, self.settings.within(timeout))
    }
}

impl coqui_transport::Listener for Listener {
    type Connection = Stream;
    type Error = Error;

    fn accept(&self) -> Result<Stream, Error> {
        Self::accept(self)
    }
}
