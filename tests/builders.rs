//! Each builder setter reaches the settings it builds, `settings` takes the whole, the last setter
//! wins, and the settings are values a file fills.

mod support;

use std::time::Duration;

use coqui_transport_pipe::{
    ConnectSettings, DEFAULT_BACKLOG, DEFAULT_CONNECT_TIMEOUT, DEFAULT_IO_TIMEOUT,
    DEFAULT_SOCKET_MODE, Error, Listener, ListenerSettings, Stream,
};
use guatiao_serde::Presentation;
use guatiao_serde::text::{json, toml};
use support::place;

fn plain() -> Presentation {
    Presentation::new()
}

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
    let builder = Listener::builder(&place.name)
        .read_timeout(Duration::from_secs(5))
        .write_timeout(Duration::MAX);
    let found = builder.non_defaults();
    let names: Vec<&str> = found.iter().map(|s| s.setting.as_str()).collect();
    assert_eq!(names, ["read_timeout", "write_timeout"]);
    assert_eq!(found[0].value, "[5,0]");
    assert_eq!(found[0].default, "[30,0]");
    let listener = builder.build().unwrap();
    assert_eq!(listener.non_defaults().len(), 2);
}

#[test]
fn settings_take_the_whole_and_the_last_setter_wins() {
    let place = place("builder-whole");
    let mut whole = ListenerSettings::default();
    whole.read_timeout = Duration::from_secs(7);
    let found = Listener::builder(&place.name)
        .write_timeout(Duration::from_secs(1))
        .settings(whole)
        .read_timeout(Duration::from_secs(8))
        .non_defaults();
    assert_eq!(
        found.len(),
        1,
        "settings replaced the earlier write timeout"
    );
    assert_eq!(found[0].setting, "read_timeout");
    assert_eq!(found[0].value, "[8,0]");
}

#[test]
fn the_connect_setters_reach_the_settings_it_reports() {
    let place = place("builder-connect");
    let _listener = Listener::bind(&place.name).unwrap();
    let builder = Stream::builder(&place.name)
        .timeout(Duration::from_secs(2))
        .read_timeout(Duration::from_secs(4));
    let names: Vec<String> = builder
        .non_defaults()
        .into_iter()
        .map(|s| s.setting)
        .collect();
    assert_eq!(names, ["read_timeout", "timeout"]);
    drop(builder.build().unwrap());
}

#[test]
fn a_meaningless_value_is_refused_at_build() {
    let place = place("builder-refused");
    assert!(matches!(
        Listener::builder(&place.name).backlog(0).build(),
        Err(Error::InvalidOptions)
    ));
    assert!(matches!(
        Stream::builder(&place.name).timeout(Duration::ZERO).build(),
        Err(Error::InvalidOptions)
    ));
}

#[test]
fn settings_read_from_toml_and_json_and_round_trip() {
    let settings: ListenerSettings =
        toml::from_str_as("backlog = 4\nread_timeout = 2\n", plain()).unwrap();
    assert_eq!(settings.backlog, 4);
    assert_eq!(settings.read_timeout, Duration::from_secs(2));
    assert_eq!(settings.write_timeout, DEFAULT_IO_TIMEOUT);
    assert_eq!(settings.socket_mode, DEFAULT_SOCKET_MODE);
    assert_eq!(
        toml::from_str_as::<ListenerSettings>("", plain()).unwrap(),
        ListenerSettings::default()
    );

    let mut changed = ListenerSettings::default();
    changed.backlog = DEFAULT_BACKLOG + 1;
    changed.socket_group = Some("staff".to_owned());
    changed.security_descriptor = Some("D:P(A;;GA;;;WD)".to_owned());
    changed.write_timeout = Duration::MAX;
    let text = toml::to_string_of(&changed, plain()).unwrap();
    assert_eq!(
        toml::from_str_as::<ListenerSettings>(&text, plain()).unwrap(),
        changed
    );
    let text = json::to_string_pretty_of(&changed, plain()).unwrap();
    assert_eq!(
        json::from_str_as::<ListenerSettings>(&text, plain()).unwrap(),
        changed
    );

    let mut connect = ConnectSettings::default();
    connect.timeout = Duration::from_millis(1500);
    connect.impersonation = true;
    let text = toml::to_string_of(&connect, plain()).unwrap();
    assert_eq!(
        toml::from_str_as::<ConnectSettings>(&text, plain()).unwrap(),
        connect
    );
    assert_eq!(
        toml::from_str_as::<ConnectSettings>("", plain())
            .unwrap()
            .timeout,
        DEFAULT_CONNECT_TIMEOUT
    );
}

#[test]
fn settings_refuse_unknown_keys() {
    assert!(toml::from_str_as::<ListenerSettings>("backlogg = 1", plain()).is_err());
    assert!(toml::from_str_as::<ConnectSettings>("name = \"x\"", plain()).is_err());
    assert!(toml::from_str_as::<ListenerSettings>("backlog = \"many\"", plain()).is_err());
}
