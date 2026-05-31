//! The agenda app — the mode axis made real. Two calendar connectors of differing
//! capability back one document: a read-only ICS feed and a writable local
//! calendar. `view` renders a `CalendarView` for each, choosing the mode from the
//! connector's capability (ReadOnly for the feed, Editable for the local cal) —
//! the appdx's "policy, not just capability." The components never see a connector.

use std::sync::Arc;

use inkapp::{flow, Document, Documents};
use inkapp_core::components::calendar_view::CalendarView;
use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_ics::IcsConnector;
use inkapp_localcal::LocalCal;

/// The agenda app's own config section.
#[derive(Debug, Clone, serde::Deserialize, inkapp_config::Config)]
#[serde(default)]
#[config(kind = "agenda", namespace = "app")]
pub struct AppConfig {
    /// On-device folder path for this instance's documents (device-neutral).
    #[config(default = String::from("/Agenda"))]
    pub device_folder: String,
    /// ICS feed connector instance ("ics.<instance>").
    #[config(default = inkapp_config::ConnectorRef { kind: "ics".into(), instance: "main".into() })]
    pub feed: inkapp_config::ConnectorRef,
    /// Local calendar connector instance ("localcal.<instance>").
    #[config(default = inkapp_config::ConnectorRef { kind: "localcal".into(), instance: "main".into() })]
    pub cal: inkapp_config::ConnectorRef,
}

/// No own state: the events live in the connectors.
pub struct App;

/// The one thing a user can do here: cancel an event on their own calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    EventCancelled { uid: String },
}

/// Two connectors of differing capability, each shared as `Arc`.
pub struct Connectors {
    pub feed: Arc<IcsConnector>,
    pub cal: Arc<LocalCal>,
}

impl Connectors {
    pub fn fake() -> Self {
        Self {
            feed: Arc::new(IcsConnector::from_fixture()),
            cal: Arc::new(LocalCal::fake()),
        }
    }

    /// `cal` loads/saves cancels from `path`; `feed` uses the committed fixture
    /// (a live build would fetch over HTTP inside `IcsConnector::refresh`).
    pub fn persisted(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            feed: Arc::new(IcsConnector::from_fixture()),
            cal: Arc::new(LocalCal::persisted(path)),
        }
    }

    /// Build both connectors from config: resolve the bound ICS + localcal
    /// instances (erroring if either binding names a missing instance) and
    /// construct each from its resolved config.
    pub fn from_config(
        store: &inkapp_config::ConfigStore,
        app: &AppConfig,
    ) -> Result<Self, inkapp_config::ConfigError> {
        use inkapp_config::Namespace;
        store.require_instance(Namespace::Connector, &app.feed.kind, &app.feed.instance)?;
        store.require_instance(Namespace::Connector, &app.cal.kind, &app.cal.instance)?;
        let ics_cfg: inkapp_ics::IcsConfig = store.resolve(&app.feed.instance)?;
        let cal_cfg: inkapp_localcal::LocalCalConfig = store.resolve(&app.cal.instance)?;
        Ok(Self {
            feed: Arc::new(IcsConnector::from_config(&ics_cfg)),
            cal: Arc::new(LocalCal::from_config(&cal_cfg)?),
        })
    }
}

impl ConnectorSet for Connectors {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> {
        vec![self.feed.clone(), self.cal.clone()]
    }
}

/// The only place app logic lives: route a cancel to the writable calendar.
pub fn update(msg: Msg, _m: &mut App, cx: &Connectors) {
    match msg {
        Msg::EventCancelled { uid } => cx.cal.cancel(&uid),
    }
}

/// One document: the read-only feed agenda (mode chosen from its read-only
/// capability) above the editable local calendar (mode chosen from its writable
/// capability). The component never sees a connector; `view` decides the mode.
pub fn view(_m: &App, cx: &Connectors) -> Documents<Msg> {
    Documents(vec![Document::keyed(
        "agenda",
        flow![
            CalendarView::<Msg>::read_only(cx.feed.events()),
            CalendarView::editable(cx.cal.events(), |uid| Msg::EventCancelled {
                uid: uid.to_string()
            }),
        ],
    )])
}
