//! What a listener and a connection do by default, and what can be changed.
#![forbid(unsafe_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::time::Duration;

use coqui_peer::NonDefault;

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
pub const DEFAULT_SOCKET_MODE: u32 = 0o600;

/// The most instances Windows lets a pipe have, which it treats as no limit of its own.
pub(crate) const UNLIMITED_INSTANCES: u32 = 255;

/// A group of a Unix socket: an id, or a name looked up when binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Group {
    Id(u32),
    Name(String),
}

impl Group {
    /// The group a settings text names: digits alone are an id, anything else a name.
    fn of(text: &str) -> Self {
        text.parse()
            .map_or_else(|_| Self::Name(text.to_owned()), Self::Id)
    }
}

impl core::fmt::Display for Group {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Id(id) => write!(f, "{id}"),
            Self::Name(name) => f.write_str(name),
        }
    }
}

/// The bound a timeout setting stands for: none for [`Duration::MAX`].
pub(crate) fn bounded(timeout: Duration) -> Option<Duration> {
    (timeout != Duration::MAX).then_some(timeout)
}

/// What a [`ListenerBuilder`] takes whole through `settings`, in the form a file, a form or a peer
/// fills.
///
/// A setting for the other platform has no effect there and is listed by
/// [`not_applied`](Self::not_applied). Every key is optional (an absent one is its default) and an
/// unknown key is refused; a duration is a number of seconds, `[secs, nanos]` or `{secs, nanos}`. The
/// type is a guatiao value (`from_value`, `to_value`, `schema`), and `guatiao_serde::text` reads and
/// writes its text. It is `#[non_exhaustive]`: start from [`Default`] and assign the fields.
#[derive(Clone, Debug, PartialEq, Eq, guatiao::ToValue, guatiao::FromValue, guatiao::Schema)]
#[map(deny_unknown_fields)]
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
    /// (384 as a number in text) by default. Another is reported by
    /// [`non_defaults`](Self::non_defaults), never refused.
    #[map(default = ListenerSettings::new().socket_mode)]
    pub socket_mode: u32,
    /// Unix: the socket's group, from the moment the path exists, so a mode with group access
    /// (`0o660`) shares it with that group and no other: an id, or a name looked up when binding
    /// ([`Error::UnknownGroup`] when there is none). In text, digits alone are an id. The process
    /// must be root or a member of the group, or the bind is [`Error::GroupDenied`]. A group other
    /// than the process's own is reported by [`non_defaults`](Self::non_defaults). On Windows it is
    /// [`Error::InvalidOptions`].
    pub socket_group: Option<String>,
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
    /// The bound on each read of an accepted [`Stream`]; [`Duration::MAX`] waits forever. The async
    /// streams have none: wrap their calls in tokio's timer.
    #[map(default = ListenerSettings::new().read_timeout)]
    pub read_timeout: Duration,
    /// The bound on each write of an accepted [`Stream`]; [`Duration::MAX`] waits forever.
    #[map(default = ListenerSettings::new().write_timeout)]
    pub write_timeout: Duration,
}

impl Default for ListenerSettings {
    fn default() -> Self {
        Self::new()
    }
}

/// The keys of [`ListenerSettings`] that mean something on this platform, and the ones that do
/// not but are never refused.
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
#[cfg(windows)]
const LISTENER_IGNORED: &[&str] = &["socket_mode", "shared_directory", "backlog"];
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
            read_timeout: DEFAULT_IO_TIMEOUT,
            write_timeout: DEFAULT_IO_TIMEOUT,
        }
    }

    /// Every setting that differs from its default and means something on this platform, named by
    /// key. A socket group is listed only when it is not the process's own.
    #[must_use]
    pub fn non_defaults(&self) -> Vec<NonDefault> {
        NonDefault::of_settings(self)
            .into_iter()
            .filter(|setting| LISTENER_APPLIED.contains(&setting.setting.as_str()))
            .filter(|setting| setting.setting != "socket_group" || self.group_differs())
            .collect()
    }

    /// Settings changed from their default that this platform cannot honour, reported rather than
    /// dropped: the ones that are meaningless here (a Windows buffer size on Unix, a Unix backlog on
    /// Windows, a setting that only loosens a check this platform does not make). A setting that
    /// would be stricter than the default is not here: it is [`Error::InvalidOptions`] at bind.
    #[must_use]
    pub fn not_applied(&self) -> Vec<NonDefault> {
        NonDefault::of_settings(self)
            .into_iter()
            .filter(|setting| LISTENER_IGNORED.contains(&setting.setting.as_str()))
            .collect()
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
        self.group()
            .is_some_and(|group| crate::sys::group_differs(&group))
    }

    #[cfg(not(unix))]
    fn group_differs(&self) -> bool {
        self.socket_group.is_some()
    }

    /// The socket's group.
    pub(crate) fn group(&self) -> Option<Group> {
        self.socket_group.as_deref().map(Group::of)
    }

    /// The bound on each read of an accepted stream, if any.
    pub(crate) fn read(&self) -> Option<Duration> {
        bounded(self.read_timeout)
    }

    /// The bound on each write of an accepted stream, if any.
    pub(crate) fn write(&self) -> Option<Duration> {
        bounded(self.write_timeout)
    }

    pub(crate) fn check(&self) -> Result<(), Error> {
        let instances_are_valid = (2..=UNLIMITED_INSTANCES).contains(&self.max_instances);
        let mode_is_valid = self.socket_mode <= 0o7777;
        let backlog_is_valid = self.backlog > 0;
        let group_is_valid = match self.group() {
            None => true,
            Some(Group::Name(name)) => cfg!(unix) && !name.is_empty() && !name.contains('\0'),
            Some(Group::Id(_)) => cfg!(unix),
        };
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

/// Builds a [`Listener`]: [`Listener::builder`], the settings, then [`build`](Self::build).
///
/// Every setting that differs from its default and means something on this platform is listed by
/// [`non_defaults`](Self::non_defaults) and logged at `debug` through `tracing` when binding. The
/// crate reports and does not judge: whether one was meant is the caller's to say.
#[derive(Clone, Debug)]
#[must_use]
pub struct ListenerBuilder {
    name: PipeName,
    settings: ListenerSettings,
}

impl ListenerBuilder {
    pub(crate) fn new(name: &PipeName) -> Self {
        Self {
            name: name.clone(),
            settings: ListenerSettings::new(),
        }
    }

    /// Windows: accepts clients from other hosts; see [`ListenerSettings::remote_clients`].
    pub fn remote_clients(mut self, allow: bool) -> Self {
        self.settings.remote_clients = allow;
        self
    }

    /// Windows: this descriptor, in SDDL; see [`ListenerSettings::security_descriptor`].
    pub fn security_descriptor(mut self, sddl: impl Into<String>) -> Self {
        self.settings.security_descriptor = Some(sddl.into());
        self
    }

    /// Windows: refuses a name that already exists; see [`ListenerSettings::first_instance`].
    pub fn first_instance(mut self, first: bool) -> Self {
        self.settings.first_instance = first;
        self
    }

    /// Windows: the most instances of this pipe at once; see [`ListenerSettings::max_instances`].
    pub fn max_instances(mut self, instances: u32) -> Self {
        self.settings.max_instances = instances;
        self
    }

    /// Windows: the size of each direction's buffer of an instance; see
    /// [`ListenerSettings::buffer_size`].
    pub fn buffer_size(mut self, bytes: u32) -> Self {
        self.settings.buffer_size = bytes;
        self
    }

    /// Unix: the socket's mode; see [`ListenerSettings::socket_mode`].
    pub fn socket_mode(mut self, mode: u32) -> Self {
        self.settings.socket_mode = mode;
        self
    }

    /// Unix: the socket's group, by id; see [`ListenerSettings::socket_group`]. Replaces a group set
    /// before by id or by name.
    pub fn socket_group(mut self, group: u32) -> Self {
        self.settings.socket_group = Some(group.to_string());
        self
    }

    /// Unix: the socket's group, the one named `name`, looked up when binding. Replaces a group set
    /// before by id or by name.
    pub fn socket_group_name(mut self, name: impl Into<String>) -> Self {
        self.settings.socket_group = Some(name.into());
        self
    }

    /// Unix: binds even when others can write a directory on the way to the socket; see
    /// [`ListenerSettings::shared_directory`].
    pub fn shared_directory(mut self, allow: bool) -> Self {
        self.settings.shared_directory = allow;
        self
    }

    /// Unix: removes a socket left at the path once nothing listens on it; see
    /// [`ListenerSettings::remove_stale`].
    pub fn remove_stale(mut self, remove: bool) -> Self {
        self.settings.remove_stale = remove;
        self
    }

    /// Unix: how many connections wait for `accept`; see [`ListenerSettings::backlog`].
    pub fn backlog(mut self, connections: i32) -> Self {
        self.settings.backlog = connections;
        self
    }

    /// The bound on each read of an accepted [`Stream`]; see [`ListenerSettings::read_timeout`].
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.settings.read_timeout = timeout;
        self
    }

    /// The bound on each write of an accepted [`Stream`]; see [`ListenerSettings::write_timeout`].
    pub fn write_timeout(mut self, timeout: Duration) -> Self {
        self.settings.write_timeout = timeout;
        self
    }

    /// Every setting from `settings`. Replaces what the setters set before, and is replaced by what
    /// they set after it. The name is an argument, not a setting, and stays.
    pub fn settings(mut self, settings: ListenerSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Every setting that differs from its default and means something on this platform.
    #[must_use]
    pub fn non_defaults(&self) -> Vec<NonDefault> {
        self.settings.non_defaults()
    }

    /// Settings changed from their default that this platform cannot honour; see
    /// [`ListenerSettings::not_applied`].
    #[must_use]
    pub fn not_applied(&self) -> Vec<NonDefault> {
        self.settings.not_applied()
    }

    /// Binds the name: see [`Listener::bind`].
    ///
    /// # Errors
    ///
    /// As [`Listener::bind`]; [`Error::InvalidOptions`] for a value that means nothing, and for a
    /// setting stricter than the default that this platform cannot honour: a socket group, or a stale
    /// socket kept, on Windows; a security descriptor or an instance limit on Unix.
    pub fn build(self) -> Result<Listener, Error> {
        Listener::with_settings(&self.name, self.settings)
    }

    /// As [`build`](Self::build), as an `asynch::Listener`. Its timeouts belong to blocking streams
    /// and mean nothing there.
    ///
    /// # Errors
    ///
    /// As [`build`](Self::build).
    #[cfg(feature = "tokio")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
    pub fn build_async(self) -> Result<crate::asynch::Listener, Error> {
        crate::asynch::Listener::with_settings(&self.name, self.settings)
    }
}

/// What a [`ConnectBuilder`] takes whole through `settings`, in the form a file, a form or a peer
/// fills.
///
/// Read as [`ListenerSettings`] is. It is `#[non_exhaustive]`: start from [`Default`] and assign the
/// fields.
#[derive(Clone, Debug, PartialEq, Eq, guatiao::ToValue, guatiao::FromValue, guatiao::Schema)]
#[map(deny_unknown_fields)]
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
    /// The bound on each read of the [`Stream`]; [`Duration::MAX`] waits forever. The async streams
    /// have none: wrap their calls in tokio's timer.
    #[map(default = ConnectSettings::new().read_timeout)]
    pub read_timeout: Duration,
    /// The bound on each write of the [`Stream`]; [`Duration::MAX`] waits forever.
    #[map(default = ConnectSettings::new().write_timeout)]
    pub write_timeout: Duration,
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
            read_timeout: DEFAULT_IO_TIMEOUT,
            write_timeout: DEFAULT_IO_TIMEOUT,
        }
    }

    /// Settings changed from their default that this platform cannot honour: `impersonation` on
    /// Unix, where there is no impersonation level to raise. Reported, never dropped.
    #[must_use]
    pub fn not_applied(&self) -> Vec<NonDefault> {
        if cfg!(unix) {
            NonDefault::of_settings(self)
                .into_iter()
                .filter(|setting| setting.setting == "impersonation")
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Every setting that differs from its default and means something on this platform, named by
    /// key.
    #[must_use]
    pub fn non_defaults(&self) -> Vec<NonDefault> {
        NonDefault::of_settings(self)
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

    /// The bound on each read of the stream, if any.
    pub(crate) fn read(&self) -> Option<Duration> {
        bounded(self.read_timeout)
    }

    /// The bound on each write of the stream, if any.
    pub(crate) fn write(&self) -> Option<Duration> {
        bounded(self.write_timeout)
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

/// Builds a connection: [`Stream::builder`], the settings, then [`build`](Self::build).
///
/// Every setting that differs from its default and means something on this platform is listed by
/// [`non_defaults`](Self::non_defaults) and logged at `debug` through `tracing` when connecting.
#[derive(Clone, Debug)]
#[must_use]
pub struct ConnectBuilder {
    name: PipeName,
    settings: ConnectSettings,
}

impl ConnectBuilder {
    pub(crate) fn new(name: &PipeName) -> Self {
        Self {
            name: name.clone(),
            settings: ConnectSettings::new(),
        }
    }

    /// How long to wait for a free instance or a place in the queue; see
    /// [`ConnectSettings::timeout`].
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.settings.timeout = timeout;
        self
    }

    /// Windows: lets the server act as this client; see [`ConnectSettings::impersonation`].
    pub fn impersonation(mut self, allow: bool) -> Self {
        self.settings.impersonation = allow;
        self
    }

    /// The bound on each read of the [`Stream`]; see [`ConnectSettings::read_timeout`].
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.settings.read_timeout = timeout;
        self
    }

    /// The bound on each write of the [`Stream`]; see [`ConnectSettings::write_timeout`].
    pub fn write_timeout(mut self, timeout: Duration) -> Self {
        self.settings.write_timeout = timeout;
        self
    }

    /// Every setting from `settings`. Replaces what the setters set before, and is replaced by what
    /// they set after it. The name is an argument, not a setting, and stays.
    pub fn settings(mut self, settings: ConnectSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Settings changed from their default that this platform cannot honour; see
    /// [`ConnectSettings::not_applied`].
    #[must_use]
    pub fn not_applied(&self) -> Vec<NonDefault> {
        self.settings.not_applied()
    }

    /// Every setting that differs from its default and means something on this platform.
    #[must_use]
    pub fn non_defaults(&self) -> Vec<NonDefault> {
        self.settings.non_defaults()
    }

    /// Connects to the name: see [`Stream::connect`].
    ///
    /// # Errors
    ///
    /// As [`Stream::connect`]; [`Error::InvalidOptions`] for a zero duration.
    pub fn build(self) -> Result<Stream, Error> {
        Stream::with_settings(&self.name, self.settings)
    }

    /// As [`build`](Self::build), as an `asynch::Stream`. Its read and write timeouts belong to
    /// blocking streams and mean nothing there.
    ///
    /// # Errors
    ///
    /// As [`build`](Self::build).
    #[cfg(feature = "tokio")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
    pub async fn build_async(self) -> Result<crate::asynch::Stream, Error> {
        crate::asynch::Stream::with_settings(&self.name, self.settings).await
    }
}

fn timeouts_are_valid(read: Duration, write: Duration) -> bool {
    !read.is_zero() && !write.is_zero()
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
