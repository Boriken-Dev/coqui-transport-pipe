//! Each builder setter reaches the settings it builds, `fill` takes keys, the last write wins, and
//! the settings are read from text.

mod support;

use std::time::Duration;

use coqui_builder::Partial;
use coqui_transport_pipe::{
    BuildError, ConnectSettings, DEFAULT_BACKLOG, DEFAULT_CONNECT_TIMEOUT, DEFAULT_IO_TIMEOUT,
    DEFAULT_SOCKET_MODE, Error, Listener, ListenerSettings, SocketGroup, SocketMode, Stream,
};
use support::{line, lines, place};

#[test]
fn the_defaults_report_nothing() {
    assert!(ListenerSettings::default().non_defaults().is_empty());
    assert!(ListenerSettings::default().not_applied().is_empty());
    assert!(ConnectSettings::default().non_defaults().is_empty());
    assert!(ConnectSettings::default().not_applied().is_empty());
}

#[test]
fn the_listener_setters_reach_the_settings_it_reports() {
    let place = place("builder-setters");
    let builder = Listener::builder()
        .name(&place.name)
        .read_timeout(Duration::from_secs(5))
        .write_timeout(None::<Duration>);
    assert_eq!(
        lines(&builder.non_defaults()),
        [
            line("read_timeout", "Duration(5s)", "Duration(30s)"),
            line("write_timeout", "Duration(never)", "Duration(30s)"),
        ]
    );
    let listener = builder.build().unwrap();
    assert_eq!(listener.non_defaults().len(), 2);
}

#[test]
fn a_filled_partial_sets_only_its_keys_and_the_last_write_wins() {
    let place = place("builder-whole");
    let mut keys = Partial::<ListenerSettings>::new();
    keys.set("read_timeout", "7").unwrap();
    let found = Listener::builder()
        .name(&place.name)
        .write_timeout(Duration::from_secs(1))
        .fill(keys)
        .read_timeout(Duration::from_secs(8))
        .non_defaults();
    assert_eq!(
        lines(&found),
        [
            line("read_timeout", "Duration(8s)", "Duration(30s)"),
            line("write_timeout", "Duration(1s)", "Duration(30s)"),
        ]
    );
}

#[test]
fn the_connect_setters_reach_the_settings_it_reports() {
    let place = place("builder-connect");
    let _listener = Listener::bind(&place.name).unwrap();
    let builder = Stream::builder()
        .name(&place.name)
        .timeout(Duration::from_secs(2))
        .read_timeout(Duration::from_secs(4));
    assert_eq!(
        lines(&builder.non_defaults()),
        [
            line("read_timeout", "Duration(4s)", "Duration(30s)"),
            line("timeout", "Duration(2s)", "Duration(5s)"),
        ]
    );
    drop(builder.build().unwrap());
}

#[test]
fn a_meaningless_value_is_refused_at_build() {
    let place = place("builder-refused");
    assert!(matches!(
        Listener::builder().name(&place.name).backlog(0).build(),
        Err(BuildError::Failed(Error::InvalidOptions))
    ));
    assert!(matches!(
        Stream::builder()
            .name(&place.name)
            .timeout(Duration::ZERO)
            .build(),
        Err(BuildError::Failed(Error::InvalidOptions))
    ));
}

#[test]
fn a_build_without_the_name_says_what_is_missing() {
    let Err(BuildError::Missing(missing)) = Listener::builder().build() else {
        panic!("a listener without a name built");
    };
    assert_eq!(missing.to_string(), "missing inputs: name");
    let Err(BuildError::Missing(missing)) = Stream::builder().build() else {
        panic!("a connection without a name built");
    };
    assert_eq!(missing.inputs(), ["name"]);
}

#[test]
fn a_socket_mode_and_a_socket_group_are_texts() {
    let mut keys = Partial::<ListenerSettings>::new();
    keys.set("socket_mode", "0o660").unwrap();
    keys.set("socket_group", "4242").unwrap();
    let settings = keys.build().unwrap();
    assert_eq!(settings.socket_mode, SocketMode::new(0o660));
    assert_eq!(settings.socket_group, Some(SocketGroup::Id(4242)));
    let mut keys = Partial::<ListenerSettings>::new();
    keys.set("socket_group", "staff").unwrap();
    assert_eq!(
        keys.build().unwrap().socket_group,
        Some(SocketGroup::Name("staff".to_owned()))
    );
    for refused in ["660", "384", "0o", "0o8", "0o+7", "0x1ff", ""] {
        let mut keys = Partial::<ListenerSettings>::new();
        assert!(keys.set("socket_mode", refused).is_err(), "{refused:?}");
    }
    let mut keys = Partial::<ListenerSettings>::new();
    assert!(keys.set("socket_group", "").is_err());
    coqui_builder::describe::assert_round_trip(&[
        SocketMode::new(0),
        SocketMode::new(0o660),
        SocketMode::new(0o7777),
    ]);
    coqui_builder::describe::assert_round_trip(&[
        SocketGroup::Id(0),
        SocketGroup::Id(4242),
        SocketGroup::Name("staff".to_owned()),
    ]);
}

#[test]
fn a_group_name_that_would_read_back_as_an_id_is_refused_at_bind() {
    let place = place("group-digits");
    for group in [
        SocketGroup::Name("123".to_owned()),
        SocketGroup::Name(String::new()),
    ] {
        assert!(matches!(
            Listener::builder()
                .name(&place.name)
                .socket_group(group)
                .build(),
            Err(BuildError::Failed(Error::InvalidOptions))
        ));
    }
}

#[cfg(feature = "tokio")]
#[test]
fn the_second_terminal_misses_what_the_first_misses() {
    assert!(matches!(
        Listener::builder().build_async(),
        Err(BuildError::Missing(_))
    ));
}

#[test]
fn settings_read_from_toml_and_json() {
    let settings = toml::from_str::<Partial<ListenerSettings>>("backlog = 4\nread_timeout = 2\n")
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(settings.backlog, 4);
    assert_eq!(settings.read_timeout, Some(Duration::from_secs(2)));
    assert_eq!(settings.write_timeout, Some(DEFAULT_IO_TIMEOUT));
    assert_eq!(settings.socket_mode, DEFAULT_SOCKET_MODE);
    assert_eq!(
        toml::from_str::<Partial<ListenerSettings>>("")
            .unwrap()
            .build()
            .unwrap(),
        ListenerSettings::default()
    );

    let settings = toml::from_str::<Partial<ListenerSettings>>(
        "write_timeout = \"never\"\nsocket_mode = \"0o660\"\nsocket_group = \"staff\"\n\
         security_descriptor = \"D:P(A;;GA;;;WD)\"\nbacklog = 129\n",
    )
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(settings.write_timeout, None);
    assert_eq!(settings.read_timeout, Some(DEFAULT_IO_TIMEOUT));
    assert_eq!(settings.backlog, DEFAULT_BACKLOG + 1);
    assert_eq!(settings.socket_mode, SocketMode::new(0o660));
    assert_eq!(
        settings.security_descriptor.as_deref(),
        Some("D:P(A;;GA;;;WD)")
    );
    let json = serde_json::from_str::<Partial<ListenerSettings>>(
        r#"{"read_timeout": "never", "socket_group": "7"}"#,
    )
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(json.read_timeout, None);
    assert_eq!(json.socket_group, Some(SocketGroup::Id(7)));

    let connect =
        toml::from_str::<Partial<ConnectSettings>>("timeout = \"1500ms\"\nimpersonation = true\n")
            .unwrap()
            .build()
            .unwrap();
    assert_eq!(connect.timeout, Duration::from_millis(1500));
    assert!(connect.impersonation);
    assert_eq!(
        toml::from_str::<Partial<ConnectSettings>>("")
            .unwrap()
            .build()
            .unwrap()
            .timeout,
        DEFAULT_CONNECT_TIMEOUT
    );
}

#[test]
fn settings_refuse_unknown_keys_and_unreadable_values() {
    assert!(toml::from_str::<Partial<ListenerSettings>>("backlogg = 1").is_err());
    assert!(toml::from_str::<Partial<ConnectSettings>>("name = \"x\"").is_err());
    assert!(toml::from_str::<Partial<ListenerSettings>>("backlog = \"many\"").is_err());
}
