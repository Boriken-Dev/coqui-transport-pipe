//! A setting looser than its default is reported as data by `non_defaults()` and never warned about: the crate
//! reports and the caller decides.

mod support;

use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use coqui_transport_pipe::{Listener, Stream};
use support::place;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

#[derive(Clone, Default)]
struct Recorder {
    warnings: Arc<Mutex<Vec<String>>>,
}

struct Setting(Option<String>);

impl Visit for Setting {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "setting" {
            self.0 = Some(format!("{value:?}"));
        }
    }
}

impl Subscriber for Recorder {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        if *event.metadata().level() == Level::WARN {
            let mut setting = Setting(None);
            event.record(&mut setting);
            if let Some(name) = setting.0 {
                self.warnings.lock().unwrap().push(name);
            }
        }
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// The settings warned about while `body` runs.
fn warned(body: impl FnOnce()) -> Vec<String> {
    let recorder = Recorder::default();
    let warnings = recorder.warnings.clone();
    tracing::subscriber::with_default(recorder, body);
    warnings.lock().unwrap().clone()
}

#[test]
fn a_non_default_listener_is_reported_when_it_binds_and_not_warned_about() {
    let place = place("reported");
    let mut reported = Vec::new();
    let warnings = warned(|| {
        let options = Listener::builder()
            .name(&place.name)
            .read_timeout(None::<Duration>)
            .write_timeout(None::<Duration>);
        reported = options.non_defaults();
        let listener = options.build().unwrap();
        assert_eq!(listener.non_defaults().len(), 2);
    });
    assert!(warnings.is_empty(), "{warnings:?}");
    let settings: Vec<_> = reported.iter().map(|w| w.setting.to_string()).collect();
    assert_eq!(settings, ["read_timeout", "write_timeout"]);
    assert_eq!(reported[0].value.to_string(), "Duration(never)");
    assert_eq!(reported[0].default.to_string(), "Duration(30s)");
}

#[test]
fn a_non_default_connection_is_reported_and_not_warned_about() {
    let place = place("reported-client");
    let _listener = Listener::bind(&place.name).unwrap();
    let mut reported = Vec::new();
    let warnings = warned(|| {
        let options = Stream::builder()
            .name(&place.name)
            .read_timeout(Duration::from_secs(31));
        reported = options.non_defaults();
        drop(options.build().unwrap());
    });
    assert!(warnings.is_empty(), "{warnings:?}");
    let settings: Vec<_> = reported.iter().map(|w| w.setting.to_string()).collect();
    assert_eq!(settings, ["read_timeout"]);
}

#[test]
fn the_defaults_report_nothing() {
    let place = place("reported-defaults");
    let warnings = warned(|| {
        let listener = Listener::bind(&place.name).unwrap();
        assert!(listener.non_defaults().is_empty());
        let _client = Stream::connect(&place.name).unwrap();
        assert!(
            Stream::builder()
                .name(&place.name)
                .non_defaults()
                .is_empty()
        );
        drop(listener);
    });
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[cfg(windows)]
#[test]
fn allowing_remote_clients_is_reported_and_not_warned_about() {
    let place = place("reported-remote");
    let mut reported = Vec::new();
    let warnings = warned(|| {
        let options = Listener::builder().name(&place.name).remote_clients(true);
        reported = options.non_defaults();
        drop(options.build().unwrap());
    });
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].setting, "remote_clients");
}

#[cfg(windows)]
#[test]
fn a_socket_group_means_nothing_on_windows() {
    use coqui_transport_pipe::SocketGroup;

    let place = place("group-windows");
    for options in [
        Listener::builder()
            .name(&place.name)
            .socket_group(SocketGroup::Id(1)),
        Listener::builder()
            .name(&place.name)
            .socket_group(SocketGroup::Name("users".to_owned())),
    ] {
        assert!(matches!(
            options.build(),
            Err(coqui_transport_pipe::BuildError::Failed(
                coqui_transport_pipe::Error::InvalidOptions
            ))
        ));
    }
}

/// A setting this platform cannot honour is never dropped: one that would be stricter than the default is
/// refused, one that is meaningless here is reported in `not_applied()` and not warned about.
#[cfg(windows)]
#[test]
fn a_unix_setting_is_refused_when_stricter_and_reported_when_meaningless() {
    use coqui_transport_pipe::Error;

    let place = place("not-applied");
    assert!(matches!(
        Listener::builder()
            .name(&place.name)
            .remove_stale(false)
            .build(),
        Err(coqui_transport_pipe::BuildError::Failed(
            Error::InvalidOptions
        ))
    ));
    let mut reported = Vec::new();
    let warnings = warned(|| {
        let options = Listener::builder()
            .name(&place.name)
            .socket_mode(0o660)
            .backlog(8);
        reported = options.not_applied();
        let listener = options.build().unwrap();
        assert_eq!(listener.not_applied().len(), 2);
        assert!(listener.non_defaults().is_empty());
    });
    assert!(warnings.is_empty(), "{warnings:?}");
    let settings: Vec<_> = reported.iter().map(|w| w.setting.to_string()).collect();
    assert_eq!(settings, ["backlog", "socket_mode"]);
    assert_eq!(reported[0].value.to_string(), "i32(8)");
    assert_eq!(reported[0].default.to_string(), "i32(128)");
    assert_eq!(reported[1].value.to_string(), "SocketMode(0o660)");
    assert_eq!(reported[1].default.to_string(), "SocketMode(0o600)");
}

#[cfg(unix)]
#[test]
fn a_windows_setting_is_refused_when_stricter_and_reported_when_meaningless() {
    use coqui_transport_pipe::Error;

    let place = place("not-applied");
    for options in [
        Listener::builder()
            .name(&place.name)
            .security_descriptor("D:P(A;;GA;;;WD)".to_owned()),
        Listener::builder().name(&place.name).max_instances(2),
    ] {
        assert!(matches!(
            options.build(),
            Err(coqui_transport_pipe::BuildError::Failed(
                Error::InvalidOptions
            ))
        ));
    }
    let mut reported = Vec::new();
    let warnings = warned(|| {
        let options = Listener::builder()
            .name(&place.name)
            .buffer_size(4096)
            .remote_clients(true);
        reported = options.not_applied();
        let listener = options.build().unwrap();
        assert_eq!(listener.not_applied().len(), 2);
        assert!(listener.non_defaults().is_empty());
        let connect = Stream::builder().name(&place.name).impersonation(true);
        assert_eq!(connect.not_applied().len(), 1);
        drop(connect.build().unwrap());
    });
    assert!(warnings.is_empty(), "{warnings:?}");
    let settings: Vec<_> = reported.iter().map(|w| w.setting.to_string()).collect();
    assert_eq!(settings, ["buffer_size", "remote_clients"]);
    assert_eq!(reported[0].value.to_string(), "u32(4096)");
}

#[cfg(unix)]
#[test]
fn any_socket_mode_is_accepted_and_reported_when_it_is_not_the_default() {
    use std::os::unix::fs::MetadataExt;

    for mode in [0o400, 0o640, 0o660, 0o666, 0o700] {
        let place = place("mode");
        let mut reported = Vec::new();
        let warnings = warned(|| {
            let options = Listener::builder().name(&place.name).socket_mode(mode);
            reported = options.non_defaults();
            let listener = options.build().unwrap();
            let on_disk = std::fs::metadata(place.name.path()).unwrap().mode() & 0o7777;
            assert_eq!(on_disk, mode, "{mode:o}");
            drop(listener);
        });
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(reported.len(), 1, "{mode:o}");
        assert_eq!(reported[0].setting, "socket_mode");
        assert_eq!(
            reported[0].value.to_string(),
            format!("SocketMode(0o{mode:o})")
        );
        assert_eq!(reported[0].default.to_string(), "SocketMode(0o600)");
    }
    let place = place("mode-default");
    assert!(
        Listener::builder()
            .name(&place.name)
            .socket_mode(0o600)
            .non_defaults()
            .is_empty()
    );
}
