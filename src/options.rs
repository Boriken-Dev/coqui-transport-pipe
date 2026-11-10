//! What a listener and a connection do by default, and what can be changed.
#![forbid(unsafe_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::str::FromStr;
use core::time::Duration;

use coqui_builder::{Builder, Configured, Describe, Leaf, NonDefault};

use crate::error::Error;
use crate::listener::Listener;
use crate::name::PipeName;
use crate::stream::Stream;

/// The default bound on one read and one write of a [`Stream`].
pub const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(30);
/// The default bound on [`ConnectBuilder::build`].
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The default size of each direction's buffer of a pipe instance on Windows, in bytes.
pub const DEFAULT_BUFFER_SIZE: u32 = 64 * 1024;
/// The default queue of connections a socket holds for `accept` on Unix.
pub const DEFAULT_BACKLOG: i32 = 128;
/// The default mode of a socket on Unix: its owner only.
pub const DEFAULT_SOCKET_MODE: SocketMode = SocketMode::new(0o600);

/// The most instances Windows lets a pipe have, which it treats as no limit of its own.
pub(crate) const UNLIMITED_INSTANCES: u32 = 255;

/// The permission bits of a Unix socket. Written in octal with a `0o` prefix (`0o660`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Leaf)]
pub struct SocketMode(u32);

impl SocketMode {
    /// These permission bits. A value above `0o7777` is [`Error::InvalidOptions`] when a listener
    /// binds.
    #[must_use]
    pub const fn new(bits: u32) -> Self {
        Self(bits)
    }

    /// The permission bits.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl From<u32> for SocketMode {
    fn from(bits: u32) -> Self {
        Self(bits)
    }
}

impl fmt::Display for SocketMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0o{:o}", self.0)
    }
}

impl FromStr for SocketMode {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        let digits = text.strip_prefix("0o").ok_or(Error::InvalidOptions)?;
        if digits.starts_with('+') {
            return Err(Error::InvalidOptions);
        }
        u32::from_str_radix(digits, 8)
            .map(Self)
            .map_err(|_| Error::InvalidOptions)
    }
}

/// The group of a Unix socket: an id, or a name looked up when binding.
///
/// Written as the id's digits or as the name, so a name made only of digits that fit an id reads as
/// that id: such a name is [`Error::InvalidOptions`] when a listener binds. An empty name, or one
/// holding a NUL, is too.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Leaf)]
pub enum SocketGroup {
    /// The group with this id.
    Id(u32),
    /// The group with this name, looked up when binding ([`Error::UnknownGroup`] when there is none).
    Name(String),
}

impl SocketGroup {
    /// Whether this names a group the text form can hold.
    fn is_writable(&self) -> bool {
        match self {
            Self::Id(_) => true,
            Self::Name(name) => {
                !name.is_empty() && !name.contains('\0') && name.parse::<u32>().is_err()
            }
        }
    }
}

impl fmt::Display for SocketGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(id) => write!(f, "{id}"),
            Self::Name(name) => f.write_str(name),
        }
    }
}

impl FromStr for SocketGroup {
    type Err = Error;

    /// Digits alone are an id, anything else a name.
    fn from_str(text: &str) -> Result<Self, Error> {
        if text.is_empty() || text.contains('\0') {
            return Err(Error::InvalidOptions);
        }
        Ok(text
            .parse()
            .map_or_else(|_| Self::Name(text.to_owned()), Self::Id))
    }
}

/// What a [`ListenerBuilder`] takes whole through `fill`, in the form a file, a form or a peer
/// fills.
///
/// A setting for the other platform has no effect there and is listed by
/// [`not_applied`](Self::not_applied). Every key is optional (an absent one is its default) and an
/// unknown key is refused; a duration is a number of seconds or a number with a unit (`30`, `250ms`),
/// and `never` is no bound on a timeout. The type is a `coqui-builder` setting: read it by filling a
/// `Partial<ListenerSettings>` and building it. It is `#[non_exhaustive]`: start from [`Default`]
/// and assign the fields.
#[derive(Clone, Debug, PartialEq, Eq, Describe)]
#[map(deny_unknown_fields)]
#[builder(setters_on = ListenerBuilder)]
#[non_exhaustive]
pub struct ListenerSettings {
    /// Windows: accepts clients from other hosts. Off by default, by `PIPE_REJECT_REMOTE_CLIENTS`.
    #[map(default = ListenerSettings::new().remote_clients)]
    pub remote_clients: bool,
    /// Windows: this descriptor, in SDDL, in place of the default, which gives the current user alone
    /// full access (`D:P(A;;GA;;;<SID>)`, with that user as owner) and, above medium integrity,
    /// labels the pipe so that nobody below this process's level can connect or add instances.
    ///
    /// This replaces all of that: no label is added, and a text with no DACL gets the token's
    /// default DACL. A descriptor granting `FILE_CREATE_PIPE_INSTANCE` (`GENERIC_WRITE` holds it) to
    /// another user lets that user add instances of this pipe and receive its clients. One holding
    /// a NUL, or not letting this user add instances, is [`Error::InvalidDescriptor`] at bind.
    pub security_descriptor: Option<String>,
    /// Windows: refuses a name that already exists, by `FILE_FLAG_FIRST_PIPE_INSTANCE`. On by
    /// default. Turned off, this listener joins an existing pipe its user may add instances to,
    /// which keeps its own descriptor, and clients may reach whichever process created the other
    /// instances.
    #[map(default = ListenerSettings::new().first_instance)]
    pub first_instance: bool,
    /// Windows: the most instances of this pipe at once, waiting and connected, from 2 to 254; 255,
    /// the default, leaves it to the system. One instance always waits for the next client, so at
    /// `n` an [`accept`](Listener::accept) waits while `n - 1` connections are open.
    #[map(default = ListenerSettings::new().max_instances)]
    pub max_instances: u32,
    /// Windows: the size of each direction's buffer of an instance, in bytes. A write larger than
    /// the room left waits for the peer to read.
    #[map(default = ListenerSettings::new().buffer_size)]
    pub buffer_size: u32,
    /// Unix: the socket's mode, from the moment the path exists, any of `0` to `0o7777`; `0o600`
    /// by default. Another is reported by [`non_defaults`](ListenerSettings::non_defaults), never refused.
    #[map(default = ListenerSettings::new().socket_mode)]
    pub socket_mode: SocketMode,
    /// Unix: the socket's group, from the moment the path exists, so a mode with group access
    /// (`0o660`) shares it with that group and no other: an id, or a name looked up when binding
    /// ([`Error::UnknownGroup`] when there is none). In text, digits alone are an id. The process
    /// must be root or a member of the group, or the bind is [`Error::GroupDenied`]. A group other
    /// than the process's own is reported by [`non_defaults`](ListenerSettings::non_defaults). On Windows it is
    /// [`Error::InvalidOptions`].
    pub socket_group: Option<SocketGroup>,
    /// Unix: binds even when others can write a directory on the way to the socket, or another user
    /// owns one. Off by default: such a user could replace the socket, and clients would reach
    /// theirs.
    #[map(default = ListenerSettings::new().shared_directory)]
    pub shared_directory: bool,
    /// Unix: removes a socket left at the path once nothing listens on it. On by default; off, it is
    /// [`Error::NameTaken`]. A live socket, or anything that is not a socket, is never removed.
    #[map(default = ListenerSettings::new().remove_stale)]
    pub remove_stale: bool,
    /// Unix: how many connections wait for `accept`, up to the system's own limit.
    #[map(default = ListenerSettings::new().backlog)]
    pub backlog: i32,
    /// The bound on each read of an accepted [`Stream`]; `never` (`None`) waits forever. The async
    /// streams have none: wrap their calls in tokio's timer.
    #[map(default = Some(DEFAULT_IO_TIMEOUT), none = "never")]
    pub read_timeout: Option<Duration>,
    /// The bound on each write of an accepted [`Stream`]; `never` (`None`) waits forever.
    #[map(default = Some(DEFAULT_IO_TIMEOUT), none = "never")]
    pub write_timeout: Option<Duration>,
}

impl Default for ListenerSettings {
    fn default() -> Self {
        Self::new()
    }
}

/// The keys of [`ListenerSettings`] that mean something on this platform.
#[cfg(windows)]
const LISTENER_APPLIED: &[&str] = &[
    "remote_clients",
    "security_descriptor",
    "first_instance",
    "max_instances",
    "buffer_size",
    "read_timeout",
    "write_timeout",
];
/// The keys that mean nothing on this platform and are never refused.
#[cfg(windows)]
const LISTENER_IGNORED: &[&str] = &["socket_mode", "shared_directory", "backlog"];
/// The keys that are refused when set stricter than their default, since this platform cannot
/// honour them.
#[cfg(windows)]
#[cfg(test)]
const LISTENER_REFUSED: &[&str] = &["socket_group", "remove_stale"];
#[cfg(not(windows))]
const LISTENER_APPLIED: &[&str] = &[
    "socket_mode",
    "socket_group",
    "shared_directory",
    "remove_stale",
    "backlog",
    "read_timeout",
    "write_timeout",
];
#[cfg(not(windows))]
const LISTENER_IGNORED: &[&str] = &["buffer_size", "remote_clients", "first_instance"];
#[cfg(not(windows))]
#[cfg(test)]
const LISTENER_REFUSED: &[&str] = &["security_descriptor", "max_instances"];

impl ListenerSettings {
    /// Every setting at its default.
    pub const fn new() -> Self {
        Self {
            remote_clients: false,
            security_descriptor: None,
            first_instance: true,
            max_instances: UNLIMITED_INSTANCES,
            buffer_size: DEFAULT_BUFFER_SIZE,
            socket_mode: DEFAULT_SOCKET_MODE,
            socket_group: None,
            shared_directory: false,
            remove_stale: true,
            backlog: DEFAULT_BACKLOG,
            read_timeout: Some(DEFAULT_IO_TIMEOUT),
            write_timeout: Some(DEFAULT_IO_TIMEOUT),
        }
    }

    /// Every setting that differs from its default and means something on this platform, named by
    /// key. A socket group is listed only when it is not the process's own.
    #[must_use]
    pub fn non_defaults(&self) -> Vec<NonDefault> {
        applied(coqui_builder::non_defaults(self), self.group_differs())
    }

    /// Settings changed from their default that this platform cannot honour, reported rather than
    /// dropped: the ones that are meaningless here (a Windows buffer size on Unix, a Unix backlog on
    /// Windows, a setting that only loosens a check this platform does not make). A setting that
    /// would be stricter than the default is not here: it is [`Error::InvalidOptions`] at bind.
    #[must_use]
    pub fn not_applied(&self) -> Vec<NonDefault> {
        ignored(coqui_builder::non_defaults(self))
    }

    /// Whether a setting stricter than the default cannot be honoured here.
    fn unsupported(&self) -> bool {
        if cfg!(windows) {
            self.socket_group.is_some() || !self.remove_stale
        } else {
            self.security_descriptor.is_some() || self.max_instances != UNLIMITED_INSTANCES
        }
    }

    #[cfg(unix)]
    fn group_differs(&self) -> bool {
        self.socket_group
            .as_ref()
            .is_some_and(crate::sys::group_differs)
    }

    #[cfg(not(unix))]
    fn group_differs(&self) -> bool {
        self.socket_group.is_some()
    }

    pub(crate) fn check(&self) -> Result<(), Error> {
        let instances_are_valid = (2..=UNLIMITED_INSTANCES).contains(&self.max_instances);
        let mode_is_valid = self.socket_mode.get() <= 0o7777;
        let backlog_is_valid = self.backlog > 0;
        let group_is_valid = self
            .socket_group
            .as_ref()
            .is_none_or(|group| cfg!(unix) && group.is_writable());
        if instances_are_valid
            && group_is_valid
            && !self.unsupported()
            && mode_is_valid
            && backlog_is_valid
            && timeouts_are_valid(self.read_timeout, self.write_timeout)
        {
            Ok(())
        } else {
            Err(Error::InvalidOptions)
        }
    }

    pub(crate) fn log_non_defaults(&self) {
        log(&self.non_defaults());
        log_not_applied(&self.not_applied());
    }
}

/// The parts a [`ListenerBuilder`] gathers: the settings and the name to bind.
///
/// Made by [`ListenerBuilder::parts`]; [`build`](ListenerBuilder::build) and
/// [`build_async`](ListenerBuilder::build_async) convert it.
///
/// Every setting that differs from its default and means something on this platform is listed by
/// [`Listener::non_defaults`] and logged at `debug` through `tracing` when binding. The crate
/// reports and does not judge: whether one was meant is the caller's to say.
#[derive(Builder, Debug)]
#[builder(build = Listener, fallible)]
pub struct ListenerParts {
    settings: ListenerSettings,
    #[builder(input)]
    name: PipeName,
}

impl TryFrom<ListenerParts> for Listener {
    type Error = Error;

    fn try_from(parts: ListenerParts) -> Result<Self, Error> {
        Self::with_settings(&parts.name, parts.settings)
    }
}

impl ListenerBuilder {
    /// Settings changed from their default that this platform cannot honour; see
    /// [`ListenerSettings::not_applied`].
    #[must_use]
    pub fn not_applied(&self) -> Vec<NonDefault> {
        ignored(Configured::non_defaults(self))
    }

    /// As [`build`](Self::build), as an `asynch::Listener`. Its timeouts belong to blocking streams
    /// and mean nothing there.
    ///
    /// # Errors
    ///
    /// As [`build`](Self::build).
    #[cfg(feature = "tokio")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
    pub fn build_async(self) -> Result<crate::asynch::Listener, crate::BuildError<Error>> {
        let parts = self.parts().map_err(crate::BuildError::Missing)?;
        crate::asynch::Listener::with_settings(&parts.name, parts.settings)
            .map_err(crate::BuildError::Failed)
    }
}

/// What a [`ConnectBuilder`] takes whole through `fill`, in the form a file, a form or a peer
/// fills.
///
/// Read as [`ListenerSettings`] is. It is `#[non_exhaustive]`: start from [`Default`] and assign the
/// fields.
#[derive(Clone, Debug, PartialEq, Eq, Describe)]
#[map(deny_unknown_fields)]
#[builder(setters_on = ConnectBuilder)]
#[non_exhaustive]
pub struct ConnectSettings {
    /// How long to wait for a free instance or a place in the queue. A name nobody listens on fails
    /// at once. On Windows the open of another host's pipe is the system's and waits for the
    /// network, whatever this says.
    #[map(default = ConnectSettings::new().timeout)]
    pub timeout: Duration,
    /// Windows: lets the server act as this client. Off by default: the client connects at
    /// `SECURITY_IDENTIFICATION`, so a server, or a squatter in its place, can learn who the client
    /// is and cannot act as it.
    #[map(default = ConnectSettings::new().impersonation)]
    pub impersonation: bool,
    /// The bound on each read of the [`Stream`]; `never` (`None`) waits forever. The async streams
    /// have none: wrap their calls in tokio's timer.
    #[map(default = Some(DEFAULT_IO_TIMEOUT), none = "never")]
    pub read_timeout: Option<Duration>,
    /// The bound on each write of the [`Stream`]; `never` (`None`) waits forever.
    #[map(default = Some(DEFAULT_IO_TIMEOUT), none = "never")]
    pub write_timeout: Option<Duration>,
}

impl Default for ConnectSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectSettings {
    /// Every setting at its default.
    pub const fn new() -> Self {
        Self {
            timeout: DEFAULT_CONNECT_TIMEOUT,
            impersonation: false,
            read_timeout: Some(DEFAULT_IO_TIMEOUT),
            write_timeout: Some(DEFAULT_IO_TIMEOUT),
        }
    }

    /// Settings changed from their default that this platform cannot honour: `impersonation` on
    /// Unix, where there is no impersonation level to raise. Reported, never dropped.
    #[must_use]
    pub fn not_applied(&self) -> Vec<NonDefault> {
        connect_ignored(coqui_builder::non_defaults(self))
    }

    /// Every setting that differs from its default and means something on this platform, named by
    /// key.
    #[must_use]
    pub fn non_defaults(&self) -> Vec<NonDefault> {
        coqui_builder::non_defaults(self)
            .into_iter()
            .filter(|setting| !(cfg!(unix) && setting.setting == "impersonation"))
            .collect()
    }

    /// These settings with `timeout` as the wait for a place, which a connector's caller chooses.
    pub(crate) fn within(&self, timeout: Duration) -> Self {
        let mut settings = self.clone();
        settings.timeout = timeout;
        settings
    }

    pub(crate) fn check(&self) -> Result<(), Error> {
        if !self.timeout.is_zero() && timeouts_are_valid(self.read_timeout, self.write_timeout) {
            Ok(())
        } else {
            Err(Error::InvalidOptions)
        }
    }

    pub(crate) fn log_non_defaults(&self) {
        log(&self.non_defaults());
        log_not_applied(&self.not_applied());
    }
}

/// The parts a [`ConnectBuilder`] gathers: the settings and the name to connect to.
///
/// Made by [`ConnectBuilder::parts`]; [`build`](ConnectBuilder::build) and
/// [`build_async`](ConnectBuilder::build_async) convert it.
///
/// Every setting that differs from its default and means something on this platform is logged at
/// `debug` through `tracing` when connecting.
#[derive(Builder, Debug)]
#[builder(build = Stream, name = "ConnectBuilder", fallible)]
pub struct ConnectParts {
    settings: ConnectSettings,
    #[builder(input)]
    name: PipeName,
}

impl TryFrom<ConnectParts> for Stream {
    type Error = Error;

    fn try_from(parts: ConnectParts) -> Result<Self, Error> {
        Self::with_settings(&parts.name, parts.settings)
    }
}

impl ConnectBuilder {
    /// Settings changed from their default that this platform cannot honour; see
    /// [`ConnectSettings::not_applied`].
    #[must_use]
    pub fn not_applied(&self) -> Vec<NonDefault> {
        connect_ignored(Configured::non_defaults(self))
    }

    /// As [`build`](Self::build), as an `asynch::Stream`. Its read and write timeouts belong to
    /// blocking streams and mean nothing there.
    ///
    /// # Errors
    ///
    /// As [`build`](Self::build).
    #[cfg(feature = "tokio")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
    pub async fn build_async(self) -> Result<crate::asynch::Stream, crate::BuildError<Error>> {
        let parts = self.parts().map_err(crate::BuildError::Missing)?;
        crate::asynch::Stream::with_settings(&parts.name, parts.settings)
            .await
            .map_err(crate::BuildError::Failed)
    }
}

/// The differences that mean something here, with the socket group only when `group_differs`.
fn applied(found: Vec<NonDefault>, group_differs: bool) -> Vec<NonDefault> {
    found
        .into_iter()
        .filter(|setting| LISTENER_APPLIED.contains(&setting.setting.as_str()))
        .filter(|setting| setting.setting != "socket_group" || group_differs)
        .collect()
}

/// The differences that mean nothing here.
fn ignored(found: Vec<NonDefault>) -> Vec<NonDefault> {
    found
        .into_iter()
        .filter(|setting| LISTENER_IGNORED.contains(&setting.setting.as_str()))
        .collect()
}

fn connect_ignored(found: Vec<NonDefault>) -> Vec<NonDefault> {
    if cfg!(unix) {
        found
            .into_iter()
            .filter(|setting| setting.setting == "impersonation")
            .collect()
    } else {
        Vec::new()
    }
}

fn timeouts_are_valid(read: Option<Duration>, write: Option<Duration>) -> bool {
    !read.is_some_and(|bound| bound.is_zero()) && !write.is_some_and(|bound| bound.is_zero())
}

fn log_not_applied(list: &[NonDefault]) {
    for ignored in list {
        tracing::debug!(
            setting = %ignored.setting,
            value = %ignored.value,
            default = %ignored.default,
            "a pipe setting has no effect on this platform"
        );
    }
}

fn log(list: &[NonDefault]) {
    for different in list {
        tracing::debug!(
            setting = %different.setting,
            value = %different.value,
            default = %different.default,
            "a pipe setting differs from its default"
        );
    }
}

#[cfg(test)]
mod tests {
    use coqui_builder::Describe;

    use super::{
        ConnectSettings, LISTENER_APPLIED, LISTENER_IGNORED, LISTENER_REFUSED, ListenerSettings,
    };

    /// Every key of the listener settings is told apart on this platform: it means something, it
    /// means nothing and is never refused, or it is refused when stricter than its default. A new
    /// key that is in none of the lists would be dropped from every report.
    #[test]
    fn every_listener_key_is_applied_ignored_or_refused_exactly_once() {
        let description = ListenerSettings::description();
        let keys: Vec<&str> = description
            .group_fields()
            .expect("the settings are a group")
            .iter()
            .map(|field| field.key.as_str())
            .collect();
        for key in &keys {
            let lists = [LISTENER_APPLIED, LISTENER_IGNORED, LISTENER_REFUSED]
                .iter()
                .filter(|list| list.contains(key))
                .count();
            assert_eq!(lists, 1, "{key} is in {lists} of the three lists");
        }
        for key in LISTENER_APPLIED
            .iter()
            .chain(LISTENER_IGNORED)
            .chain(LISTENER_REFUSED)
        {
            assert!(keys.contains(key), "{key} is not a key");
        }
    }

    #[test]
    fn the_connect_keys_are_the_ones_the_reports_know() {
        let description = ConnectSettings::description();
        let keys: Vec<&str> = description
            .group_fields()
            .expect("the settings are a group")
            .iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(
            keys,
            ["timeout", "impersonation", "read_timeout", "write_timeout"]
        );
    }
}
