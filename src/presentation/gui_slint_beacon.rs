macro_rules! sst_blue_slint {
    (export component SstBlueWindow inherits Window { $($body:tt)* }) => {
        ::slint::slint! {
            export component BaseSstBlueWindow inherits Window {
                in property <bool> beacon-enabled: true;
                in property <string> beacon-status-text: "● LAN 0";
                in property <string> beacon-alert-text: "";
                in property <bool> peer-island-active: false;
                in property <string> peer-island-title: "";
                in property <string> peer-island-detail: "";
                in property <string> peer-island-extra: "";
                callback beacon-toggle();

                $($body)*

                beacon-chip := Rectangle {
                    visible: root.beacon-alert-text == "";
                    x: island.x + island.width - 392px;
                    y: island.y + 1px;
                    width: 96px;
                    height: island.height - 2px;
                    border-radius: 10px;
                    background: beacon-touch.pressed
                        ? rgb(36, 49, 67)
                        : beacon-touch.has-hover ? rgb(23, 35, 52) : transparent;

                    Text {
                        x: 4px;
                        y: 0;
                        width: parent.width - 8px;
                        height: parent.height;
                        text: root.beacon-status-text;
                        color: root.beacon-enabled ? #80d19b : #8b95a5;
                        font-family: "Segoe UI Variable";
                        font-size: 12px;
                        font-weight: 600;
                        vertical-alignment: center;
                        horizontal-alignment: center;
                    }

                    beacon-touch := TouchArea {
                        mouse-cursor: pointer;
                        clicked => { root.beacon-toggle(); }
                    }
                }

                beacon-alert := Rectangle {
                    visible: root.beacon-alert-text != "";
                    x: island.x + 90px;
                    y: island.y + 1px;
                    width: max(120px, island.width - 210px);
                    height: island.height - 2px;
                    border-radius: 10px;
                    background: rgba(10, 13, 20, 0.98);

                    Text {
                        x: 10px;
                        y: 0;
                        width: parent.width - 20px;
                        height: parent.height;
                        text: root.beacon-alert-text;
                        color: #dfe8ef;
                        font-family: "Segoe UI Variable";
                        font-size: 12px;
                        font-weight: 600;
                        vertical-alignment: center;
                        horizontal-alignment: center;
                        overflow: elide;
                    }
                }

                peer-island := Rectangle {
                    width: root.peer-island-active ? 286px : 18px;
                    height: root.peer-island-active ? 58px : 18px;
                    x: root.width - self.width - 12px;
                    y: root.height - self.height - 12px;
                    border-radius: root.peer-island-active ? 12px : 9px;
                    background: root.peer-island-active
                        ? rgba(10, 13, 20, 0.96)
                        : rgba(10, 13, 20, 0.90);
                    border-width: 1px;
                    border-color: root.peer-island-active
                        ? #ef5d68
                        : (root.beacon-enabled ? #4f9cff : #6d7480);

                    Rectangle {
                        x: root.peer-island-active ? 11px : 4px;
                        y: root.peer-island-active ? 12px : 4px;
                        width: 10px;
                        height: 10px;
                        border-radius: 5px;
                        background: root.peer-island-active
                            ? #ef5d68
                            : (root.beacon-enabled ? #4f9cff : #6d7480);
                    }

                    Text {
                        visible: root.peer-island-active;
                        x: 31px;
                        y: 7px;
                        width: parent.width - 42px;
                        height: 20px;
                        text: root.peer-island-title;
                        color: #f3f6fa;
                        font-family: "Segoe UI Variable";
                        font-size: 12px;
                        font-weight: 700;
                        vertical-alignment: center;
                        overflow: elide;
                    }

                    Text {
                        visible: root.peer-island-active;
                        x: 31px;
                        y: 27px;
                        width: parent.width - 76px;
                        height: 20px;
                        text: root.peer-island-detail;
                        color: #aeb9c8;
                        font-family: "Segoe UI Variable";
                        font-size: 11px;
                        vertical-alignment: center;
                        overflow: elide;
                    }

                    Text {
                        visible: root.peer-island-active && root.peer-island-extra != "";
                        x: parent.width - 50px;
                        y: 27px;
                        width: 38px;
                        height: 20px;
                        text: root.peer-island-extra;
                        color: #ef9aa1;
                        font-family: "Segoe UI Variable";
                        font-size: 11px;
                        font-weight: 700;
                        vertical-alignment: center;
                        horizontal-alignment: right;
                    }
                }
            }
        }

        struct BeaconUiController {
            _timer: ::slint::Timer,
            _handle: std::rc::Rc<std::cell::RefCell<Option<super::lan_beacon::SstBeaconHandle>>>,
            _enabled: std::rc::Rc<std::cell::Cell<bool>>,
            _peer_count: std::rc::Rc<std::cell::Cell<usize>>,
            _alert_until: std::rc::Rc<std::cell::RefCell<Option<std::time::Instant>>>,
            _peers: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, super::lan_beacon::BeaconPeer>>>,
            _visible_peer: std::rc::Rc<std::cell::RefCell<Option<String>>>,
        }

        fn refresh_peer_island(
            ui: &BaseSstBlueWindow,
            peers: &std::collections::HashMap<String, super::lan_beacon::BeaconPeer>,
            visible_peer: &mut Option<String>,
        ) {
            if peers.is_empty() {
                *visible_peer = None;
                ui.set_peer_island_active(false);
                ui.set_peer_island_title("".into());
                ui.set_peer_island_detail("".into());
                ui.set_peer_island_extra("".into());
                return;
            }

            if visible_peer
                .as_ref()
                .is_none_or(|id| !peers.contains_key(id))
            {
                *visible_peer = peers.keys().next().cloned();
            }

            let Some(peer) = visible_peer.as_ref().and_then(|id| peers.get(id)) else {
                return;
            };

            ui.set_peer_island_active(true);
            ui.set_peer_island_title(peer.host.clone().into());
            ui.set_peer_island_detail(
                format!("{} · SST {}", peer.address, peer.version).into(),
            );
            ui.set_peer_island_extra(
                if peers.len() > 1 {
                    format!("+{}", peers.len() - 1)
                } else {
                    String::new()
                }
                .into(),
            );
        }

        impl BeaconUiController {
            fn attach(ui: &BaseSstBlueWindow) -> Self {
                let handle = super::lan_beacon::SstBeaconHandle::start().ok();
                let available = handle.is_some();
                let handle = std::rc::Rc::new(std::cell::RefCell::new(handle));
                let enabled = std::rc::Rc::new(std::cell::Cell::new(available));
                let peer_count = std::rc::Rc::new(std::cell::Cell::new(0usize));
                let alert_until = std::rc::Rc::new(std::cell::RefCell::new(
                    None::<std::time::Instant>,
                ));
                let peers = std::rc::Rc::new(std::cell::RefCell::new(
                    std::collections::HashMap::<String, super::lan_beacon::BeaconPeer>::new(),
                ));
                let visible_peer = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));

                ui.set_beacon_enabled(available);
                ui.set_beacon_status_text(
                    if available { "● LAN 0" } else { "○ LAN !" }.into(),
                );
                ui.set_peer_island_active(false);
                if !available {
                    ui.set_beacon_alert_text("Baliza SST LAN no disponible".into());
                    *alert_until.borrow_mut() = Some(
                        std::time::Instant::now() + std::time::Duration::from_secs(5),
                    );
                }

                {
                    let weak = ui.as_weak();
                    let handle = handle.clone();
                    let enabled = enabled.clone();
                    let peer_count = peer_count.clone();
                    let alert_until = alert_until.clone();
                    let peers = peers.clone();
                    let visible_peer = visible_peer.clone();
                    ui.on_beacon_toggle(move || {
                        let Some(ui) = weak.upgrade() else {
                            return;
                        };
                        let handle_ref = handle.borrow();
                        let Some(beacon) = handle_ref.as_ref() else {
                            ui.set_beacon_enabled(false);
                            ui.set_beacon_status_text("○ LAN !".into());
                            ui.set_beacon_alert_text("Baliza SST LAN no disponible".into());
                            *alert_until.borrow_mut() = Some(
                                std::time::Instant::now()
                                    + std::time::Duration::from_secs(3),
                            );
                            return;
                        };

                        let next = !enabled.get();
                        beacon.set_enabled(next);
                        enabled.set(next);
                        peer_count.set(0);
                        peers.borrow_mut().clear();
                        refresh_peer_island(
                            &ui,
                            &peers.borrow(),
                            &mut visible_peer.borrow_mut(),
                        );
                        ui.set_beacon_enabled(next);
                        ui.set_beacon_status_text(
                            if next { "● LAN 0" } else { "○ LAN OFF" }.into(),
                        );
                        ui.set_beacon_alert_text(
                            if next {
                                "Baliza SST LAN encendida"
                            } else {
                                "Baliza SST LAN apagada"
                            }
                            .into(),
                        );
                        *alert_until.borrow_mut() = Some(
                            std::time::Instant::now() + std::time::Duration::from_secs(2),
                        );
                    });
                }

                let timer = ::slint::Timer::default();
                {
                    let weak = ui.as_weak();
                    let handle = handle.clone();
                    let enabled = enabled.clone();
                    let peer_count = peer_count.clone();
                    let alert_until = alert_until.clone();
                    let peers = peers.clone();
                    let visible_peer = visible_peer.clone();

                    timer.start(
                        ::slint::TimerMode::Repeated,
                        std::time::Duration::from_millis(150),
                        move || {
                            let Some(ui) = weak.upgrade() else {
                                return;
                            };

                            if let Some(beacon) = handle.borrow().as_ref() {
                                while let Some(event) = beacon.try_event() {
                                    match event {
                                        super::lan_beacon::BeaconEvent::PeerJoined {
                                            peer,
                                            peer_count: count,
                                        } => {
                                            enabled.set(true);
                                            peer_count.set(count);
                                            let peer_id = peer.id.clone();
                                            peers.borrow_mut().insert(peer_id.clone(), peer.clone());
                                            *visible_peer.borrow_mut() = Some(peer_id);
                                            refresh_peer_island(
                                                &ui,
                                                &peers.borrow(),
                                                &mut visible_peer.borrow_mut(),
                                            );
                                            ui.set_beacon_enabled(true);
                                            ui.set_beacon_status_text(
                                                format!("● LAN {count}").into(),
                                            );
                                            ui.set_beacon_alert_text(
                                                format!(
                                                    "+ {} ({}) conectado",
                                                    peer.host, peer.address
                                                )
                                                .into(),
                                            );
                                            *alert_until.borrow_mut() = Some(
                                                std::time::Instant::now()
                                                    + std::time::Duration::from_secs(4),
                                            );
                                        }
                                        super::lan_beacon::BeaconEvent::PeerLeft {
                                            peer,
                                            peer_count: count,
                                        } => {
                                            peer_count.set(count);
                                            peers.borrow_mut().remove(&peer.id);
                                            refresh_peer_island(
                                                &ui,
                                                &peers.borrow(),
                                                &mut visible_peer.borrow_mut(),
                                            );
                                            ui.set_beacon_status_text(
                                                if enabled.get() {
                                                    format!("● LAN {count}")
                                                } else {
                                                    "○ LAN OFF".to_owned()
                                                }
                                                .into(),
                                            );
                                            if enabled.get() {
                                                ui.set_beacon_alert_text(
                                                    format!(
                                                        "− {} ({}) desconectado",
                                                        peer.host, peer.address
                                                    )
                                                    .into(),
                                                );
                                                *alert_until.borrow_mut() = Some(
                                                    std::time::Instant::now()
                                                        + std::time::Duration::from_secs(3),
                                                );
                                            }
                                        }
                                        super::lan_beacon::BeaconEvent::Status {
                                            enabled: worker_enabled,
                                            peer_count: count,
                                        } => {
                                            enabled.set(worker_enabled);
                                            peer_count.set(count);
                                            if !worker_enabled || count == 0 {
                                                peers.borrow_mut().clear();
                                                refresh_peer_island(
                                                    &ui,
                                                    &peers.borrow(),
                                                    &mut visible_peer.borrow_mut(),
                                                );
                                            }
                                            ui.set_beacon_enabled(worker_enabled);
                                            ui.set_beacon_status_text(
                                                if worker_enabled {
                                                    format!("● LAN {count}")
                                                } else {
                                                    "○ LAN OFF".to_owned()
                                                }
                                                .into(),
                                            );
                                        }
                                    }
                                }
                            }

                            let should_clear = alert_until
                                .borrow()
                                .is_some_and(|deadline| std::time::Instant::now() >= deadline);
                            if should_clear {
                                *alert_until.borrow_mut() = None;
                                ui.set_beacon_alert_text("".into());
                            }
                        },
                    );
                }

                Self {
                    _timer: timer,
                    _handle: handle,
                    _enabled: enabled,
                    _peer_count: peer_count,
                    _alert_until: alert_until,
                    _peers: peers,
                    _visible_peer: visible_peer,
                }
            }
        }

        struct SstBlueWindow {
            inner: BaseSstBlueWindow,
            beacon: std::rc::Rc<BeaconUiController>,
        }

        impl SstBlueWindow {
            fn new() -> Result<Self, ::slint::PlatformError> {
                let inner = BaseSstBlueWindow::new()?;
                let beacon = std::rc::Rc::new(BeaconUiController::attach(&inner));
                Ok(Self { inner, beacon })
            }

            fn as_weak(&self) -> SstBlueWeak {
                SstBlueWeak {
                    inner: self.inner.as_weak(),
                    beacon: self.beacon.clone(),
                }
            }
        }

        impl std::ops::Deref for SstBlueWindow {
            type Target = BaseSstBlueWindow;

            fn deref(&self) -> &Self::Target {
                &self.inner
            }
        }

        #[derive(Clone)]
        struct SstBlueWeak {
            inner: ::slint::Weak<BaseSstBlueWindow>,
            beacon: std::rc::Rc<BeaconUiController>,
        }

        impl SstBlueWeak {
            fn upgrade(&self) -> Option<SstBlueWindow> {
                self.inner.upgrade().map(|inner| SstBlueWindow {
                    inner,
                    beacon: self.beacon.clone(),
                })
            }
        }
    };
}

mod slint {
    pub use ::slint::{
        BackendSelector, ComponentHandle, Image, PlatformError, Rgba8Pixel, SharedPixelBuffer,
        SharedString, Timer, TimerMode, Weak, quit_event_loop, run_event_loop,
    };
    pub use ::slint::platform;
    pub(crate) use super::sst_blue_slint as slint;
}

include!("gui_slint.rs");
