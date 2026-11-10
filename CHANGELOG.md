# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Builders and settings.** `Listener::builder()` and `Stream::builder()` set a bind or a connection up by the
  setting's name (`remote_clients`, `security_descriptor`, `first_instance`, `max_instances`, `buffer_size`, `socket_mode`,
  `socket_group`, `shared_directory`, `remove_stale`, `backlog`, `timeout`, `impersonation`, `read_timeout`,
  `write_timeout`), with the pipe name as the required `name` input, then `build()`, or `build_async()` for the tokio
  forms; `Listener::bind` and `Stream::connect` stay as the defaults. `ListenerSettings` and `ConnectSettings` are
  `coqui-builder` settings a file fills (`Partial<ListenerSettings>`), and `fill(..)` lays one over a builder. A read or
  write timeout is an `Option<Duration>`: `None`, written `never`, waits forever. `non_defaults()` lists every setting
  that differs from its default and means something on this platform, named by key; `not_applied()` the ones
  meaningless here. `Connector::settings` replaces `with_options`.
- `SocketMode`, the permission bits of a socket, written `0o660`, and `SocketGroup`, an id or a name; `ListenerParts`
  and `ConnectParts`, what the builders collect, with `parts()` on each builder; `BuildError` and `Missing`, what
  `build` fails with; the `prelude` module with the settings setters traits.
- `PipeName`: a validated Windows pipe name or Unix socket path, written `pipe:NAME` or `unix:PATH`.
- `Listener` and `Stream`: blocking listen, accept and connect, one API over named pipes and Unix
  domain sockets. A stream implements `Read`, `Write`, `coqui_peer::ReadTimeout` and `WriteTimeout`, so
  every `coqui_peer` driver works on it, deadlines included.
- Access control on by default. Windows: a descriptor for the current user only, labelled at the
  listener's integrity level when it is elevated, so a non-elevated or sandboxed process of the same
  user can neither connect nor add instances; `FILE_FLAG_FIRST_PIPE_INSTANCE`,
  `PIPE_REJECT_REMOTE_CLIENTS`, and clients at `SECURITY_IDENTIFICATION`. Unix: a `0600` socket from
  the moment its path exists, directories others can write refused along the path as written and as
  resolved, links on the way of another user refused, a socket left behind removed only when nothing
  listens on it. Every setting looser than its default is listed by `non_defaults()` and logged at `debug`
  through `tracing`.
- `PipeName` text names this host only: `\\HOST\pipe\NAME` is made with `PipeName::at` or `on_host`.
  Windows names Win32 would rewrite (a trailing dot or space, `\\?\`) are refused.
- A duration too long for the clock (`Duration::MAX`) waits as `None` does, everywhere.
- On Unix a write to a peer that left is `BrokenPipe`, never SIGPIPE, on every supported Rust.
- `OsIdentity`: what the operating system says about the other end, as information.
- `Messages`: a `MessageTransport` of length-framed messages with a bound on their size.
- `asynch`, behind `tokio`: the listener, the stream (`AsyncRead + AsyncWrite`) and `Messages` over
  tokio.
- `channel`, behind `channel`: a `coqui_web_channel::Channel` over a stream, so `coqui-agentic-peer` links run
  on a pipe.
- `ListenerOptions::socket_group` and `socket_group_name` (Unix): the socket's group, set before the path exists, so
  `socket_mode(0o660)` shares it with that group alone. A group the process cannot set is `Error::GroupDenied`, a name
  nobody has is `Error::UnknownGroup`, another group than the process's own is in `non_defaults()`, and Windows refuses it.
- `not_applied()` on `ListenerOptions`, `Listener`, `asynch::Listener` and `ConnectOptions`.
- `Connector`, and `asynch::Connector` with `tokio`: a name and `ConnectOptions` as a `coqui_transport::Connector` and
  `AsyncConnector`; `Listener` is a `coqui_transport::Listener`.

### Changed

- **The settings, the builders and the report are `coqui-builder`'s.** `ListenerSettings` and `ConnectSettings` derive
  `Describe` where they derived guatiao's `ToValue`, `FromValue` and `Schema`, so the crate no longer depends on
  guatiao. A text form changes: a duration is a number with an optional unit (`30`, `250ms`), not `[secs, nanos]` or
  `{secs, nanos}`; `read_timeout` and `write_timeout` are `Option<Duration>` and `never` (`None`) replaces
  `Duration::MAX`; `socket_mode` is `0o660`, not the number 432; `socket_group` is a `SocketGroup`. `build` and
  `build_async` fail with `BuildError<Error>`: `Missing` when the name was not given, `Failed(Error)` as before.
  `NonDefault` is `coqui-builder`'s, and its `value` and `default` are typed texts such as `Duration(30s)`,
  `SocketMode(0o600)` and `bool(true)`. A builder's setters take `impl Into` of the setting's type, so a security
  descriptor is given as a `String` and a timeout as a `Duration` or `None::<Duration>`. A builder's `non_defaults()`
  lists every difference, including the ones `not_applied()` names; the settings' and the listener's lists keep to
  what means something on this platform.

- The crate is `coqui-transport-pipe` (it was `coqui-peer-pipe`), in the `coqui-transport` family: the repository is
  `Boriken-Dev/coqui-transport-pipe`, the library `coqui_transport_pipe`.
- The `channel` feature is no longer a module: it makes `Stream` a `coqui_transport::Split` (with `ReadHalf` and
  `WriteHalf`), so `coqui_transport::StreamChannel::start(stream, codec)` is the channel on a pipe, with the codec the
  caller chooses.
- The crate picks no framing: the caller gives `StreamChannel` a codec. A framing is an implementation detail of
  whoever speaks it, so none lives here.
- A setting that differs from its default is a non-default setting: the report is coqui-peer's `NonDefault`
  (`setting`, `value`, `default`) and the methods are `non_defaults()` and `not_applied()`; no alias is kept, since
  the crate is not released.
- A setting looser than its default is reported, never warned about: `non_defaults()` returns it (what, its default, its
  value) and the crate logs it at `debug`. Only the application knows whether a shared socket or a long timeout was
  meant.
- `socket_mode` accepts any mode up to `0o7777`, where it had to hold `0o600` and stay within `0o777`; one other than the
  default is in `non_defaults()` and is never refused or warned about.
- `ListenerOptions::bind` refuses, with `InvalidOptions`, a setting stricter than the default that the platform cannot
  honour (`socket_group` and `remove_stale(false)` on Windows, `security_descriptor` and `max_instances` on Unix). One
  that is meaningless there is no longer dropped silently: `not_applied()` lists it.
- The crate depends on `coqui-transport` by git revision, and no longer on `coqui-web-channel` or, for its tests, on
  `coqui-agentic-peer`.

### Removed

- `Listener::builder(&name)` and `Stream::builder(&name)` taking the name: it is `builder().name(&name)`.
- `ListenerBuilder::settings` and `ConnectBuilder::settings`: `fill(Partial::from(settings))` replaces them.
- `ListenerBuilder::socket_group(u32)` and `socket_group_name(..)`: `socket_group(SocketGroup::Id(gid))` and
  `socket_group(SocketGroup::Name(..))`.
- `Clone` on `ListenerBuilder` and `ConnectBuilder`.
- `Duration::MAX` as the setting for no timeout, and the guatiao value form of the settings (`from_value`, `to_value`,
  `schema`).
- `coqui-peer`'s `NonDefault` as this crate's re-export, and `ListenerSettings::socket_mode` as a `u32`.
- The `channel` module: `Channel`, `ChannelOptions`, `DEFAULT_QUEUED_BYTES` and `DEFAULT_SEND_LIMIT`. Use
  `coqui_transport::StreamChannel` (its builder has the same options and more); `Channel::peer` is `Stream::peer`,
  asked before the stream is given to the channel. The channel read inside `receive`; `StreamChannel` reads ahead on a
  thread (`set_pending` bounds it).
- The channel's own frame code, and `Messages` (blocking and async) with `DEFAULT_MAX_MESSAGE`: they were the length
  framing. Run `coqui-peer`'s `drive_stream` over a `Stream`, or `StreamChannel` with a codec.
