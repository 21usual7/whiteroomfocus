//! Простой: протокол Wayland ext-idle-notify-v1 (его поддерживает Hyprland, через него работает hypridle).

use crate::Res;
use wayland_client::protocol::{wl_registry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{self, ExtIdleNotificationV1};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

#[derive(Default)]
struct State {
    seat: Option<WlSeat>,
    notifier: Option<ExtIdleNotifierV1>,
    idle: Option<bool>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(s: &mut Self, reg: &wl_registry::WlRegistry, ev: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = ev {
            match interface.as_str() {
                "wl_seat" if s.seat.is_none() => s.seat = Some(reg.bind(name, 1, qh, ())),
                "ext_idle_notifier_v1" => s.notifier = Some(reg.bind(name, version.min(2), qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(_: &mut Self, _: &WlSeat, _: wayland_client::protocol::wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtIdleNotifierV1, ()> for State {
    fn event(_: &mut Self, _: &ExtIdleNotifierV1, _: <ExtIdleNotifierV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtIdleNotificationV1, ()> for State {
    fn event(s: &mut Self, _: &ExtIdleNotificationV1, ev: ext_idle_notification_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            ext_idle_notification_v1::Event::Idled => s.idle = Some(true),
            ext_idle_notification_v1::Event::Resumed => s.idle = Some(false),
            _ => {}
        }
    }
}

/// Вызывает f(true), когда нет ввода timeout_ms, и f(false), когда ввод вернулся. Работает, пока жив композитор.
pub fn watch(timeout_ms: u32, mut f: impl FnMut(bool)) -> Res<()> {
    let conn = Connection::connect_to_env().map_err(|x| format!("Wayland: {x}"))?;
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut s = State::default();
    q.roundtrip(&mut s).map_err(|x| x.to_string())?;
    let (Some(seat), Some(n)) = (&s.seat, &s.notifier) else {
        return Err("композитор не поддерживает ext-idle-notify-v1".into());
    };
    // v2: только ввод. Видео или веб-приложение, запрещающие сон экрана (idle inhibitor), не должны прятать простой.
    if wayland_client::Proxy::version(n) >= 2 {
        n.get_input_idle_notification(timeout_ms, seat, &qh, ());
    } else {
        n.get_idle_notification(timeout_ms, seat, &qh, ());
    }
    loop {
        q.blocking_dispatch(&mut s).map_err(|x| x.to_string())?;
        if let Some(idle) = s.idle.take() {
            f(idle);
        }
    }
}
