//! Bluetooth through BlueZ.
//!
//! [`run`] keeps a [`BluetoothState`] snapshot current and carries out [`Request`]s. It also
//! registers a pairing agent: when a device needs a passkey confirmed or entered, an
//! [`Event::Prompt`] goes to the UI and BlueZ waits for the matching [`Request::Reply`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot;
use zbus::Connection;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use crate::bus::{Bus, Emit, Props, describe_error};

const SERVICE: &str = "org.bluez";
const ADAPTER_IFACE: &str = "org.bluez.Adapter1";
const DEVICE_IFACE: &str = "org.bluez.Device1";
const BATTERY_IFACE: &str = "org.bluez.Battery1";
const AGENT_MANAGER_IFACE: &str = "org.bluez.AgentManager1";
const AGENT_PATH: &str = "/io/github/kexfff/DmManager/BluetoothAgent";

const DEBOUNCE: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, PartialEq)]
pub struct Adapter {
    /// The name other devices see.
    pub name: String,
    pub address: String,
    pub powered: bool,
    pub discovering: bool,
    path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// The D-Bus path; it identifies the device in requests.
    pub path: String,
    pub address: String,
    pub name: String,
    /// The device announced a name (otherwise `name` is just its address).
    pub named: bool,
    /// A freedesktop icon name such as `audio-headset` or `input-mouse`.
    pub icon: Option<String>,
    pub paired: bool,
    pub trusted: bool,
    pub connected: bool,
    pub rssi: Option<i16>,
    pub battery: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BluetoothState {
    /// BlueZ is reachable.
    pub service: bool,
    pub adapter: Option<Adapter>,
    pub devices: Vec<Device>,
}

#[derive(Debug)]
pub enum Request {
    SetPowered(bool),
    /// Start or stop looking for nearby devices.
    Discover(bool),
    Connect(String),
    Disconnect(String),
    /// Pair, trust and connect.
    Pair(String),
    /// Unpair and forget.
    Remove(String),
    /// The answer to an [`Event::Prompt`].
    Reply {
        id: u32,
        reply: Reply,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Prompt {
    /// Both devices show `passkey`; the user confirms they match.
    Confirm { passkey: u32 },
    /// Allow the device to pair, no code involved.
    Authorize,
    /// Type a PIN code (legacy devices).
    PinCode,
    /// Type the 6-digit passkey the device shows.
    Passkey,
    /// Show `passkey` for the user to type on the device. Needs no reply.
    DisplayPasskey { passkey: u32 },
    /// Show `pin` for the user to type on the device. Needs no reply.
    DisplayPinCode { pin: String },
}

#[derive(Debug, Clone)]
pub enum Reply {
    Accept,
    Reject,
    PinCode(String),
    Passkey(u32),
}

#[derive(Debug)]
pub enum Event {
    State(BluetoothState),
    /// A request on this device started or finished.
    Busy {
        path: String,
        busy: bool,
    },
    Error(String),
    Prompt {
        id: u32,
        device: String,
        prompt: Prompt,
    },
    /// The device gave up; close any open prompt.
    PromptCanceled,
}

type ManagedObjects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;
type Pending = Arc<Mutex<HashMap<u32, oneshot::Sender<Reply>>>>;

pub async fn run(
    mut requests: UnboundedReceiver<Request>,
    emit: impl Fn(Event) + Send + Sync + 'static,
) {
    let emit: Emit<Event> = Arc::new(emit);
    let conn = match Connection::system().await {
        Ok(conn) => conn,
        Err(e) => {
            emit(Event::State(BluetoothState::default()));
            emit(Event::Error(format!(
                "No system bus: {}",
                describe_error(&e)
            )));
            while requests.recv().await.is_some() {}
            return;
        }
    };
    let bt = Bt {
        bus: Bus::new(conn.clone(), SERVICE),
        emit: emit.clone(),
        pending: Pending::default(),
    };

    let agent = Agent {
        bus: bt.bus.clone(),
        emit: emit.clone(),
        pending: bt.pending.clone(),
        next: AtomicU32::new(1),
    };
    if let Err(e) = conn.object_server().at(AGENT_PATH, agent).await {
        log::warn!("can't serve the pairing agent: {e}");
    }

    let mut signals = match bt.bus.signals().await {
        Ok(s) => Some(s),
        Err(e) => {
            log::warn!("can't watch BlueZ: {e}");
            None
        }
    };
    let mut state = bt.snapshot().await;
    if state.service {
        bt.register_agent().await;
    }
    emit(Event::State(state.clone()));

    let debounce = tokio::time::sleep(Duration::from_secs(3600));
    tokio::pin!(debounce);
    let mut dirty = false;

    loop {
        tokio::select! {
            req = requests.recv() => match req {
                Some(req) => bt.handle(req, &state),
                None => break,
            },
            msg = next_signal(&mut signals) => match msg {
                Some(Ok(_)) => {
                    if !dirty {
                        debounce.as_mut().reset(tokio::time::Instant::now() + DEBOUNCE);
                        dirty = true;
                    }
                }
                Some(Err(e)) => log::debug!("BlueZ signal: {e}"),
                None => signals = None,
            },
            () = &mut debounce, if dirty => {
                dirty = false;
                let new = bt.snapshot().await;
                // bluetoothd (re)started: it forgot our agent.
                if new.service && !state.service {
                    bt.register_agent().await;
                }
                if new != state {
                    state = new;
                    emit(Event::State(state.clone()));
                }
            }
        }
    }
}

type Signals = futures_util::stream::Select<zbus::MessageStream, zbus::MessageStream>;

async fn next_signal(signals: &mut Option<Signals>) -> Option<zbus::Result<zbus::Message>> {
    match signals {
        Some(s) => s.next().await,
        None => std::future::pending().await,
    }
}

struct Bt {
    bus: Bus,
    emit: Emit<Event>,
    pending: Pending,
}

impl Bt {
    async fn register_agent(&self) {
        let path = ObjectPath::from_static_str_unchecked(AGENT_PATH);
        let registered = self
            .bus
            .invoke(
                "/org/bluez",
                AGENT_MANAGER_IFACE,
                "RegisterAgent",
                &(&path, "KeyboardDisplay"),
            )
            .await;
        if let Err(e) = registered {
            log::warn!("can't register the pairing agent: {e}");
            return;
        }
        if let Err(e) = self
            .bus
            .invoke(
                "/org/bluez",
                AGENT_MANAGER_IFACE,
                "RequestDefaultAgent",
                &(&path,),
            )
            .await
        {
            log::debug!("not the default pairing agent: {e}");
        }
    }

    async fn snapshot(&self) -> BluetoothState {
        let objects: ManagedObjects = match self
            .bus
            .call(
                "/",
                "org.freedesktop.DBus.ObjectManager",
                "GetManagedObjects",
                &(),
            )
            .await
        {
            Ok(objects) => objects,
            Err(e) => {
                log::debug!("BlueZ unavailable: {e}");
                return BluetoothState::default();
            }
        };
        let mut objects: Vec<(String, HashMap<String, Props>)> = objects
            .into_iter()
            .map(|(path, ifaces)| {
                (
                    path.to_string(),
                    ifaces.into_iter().map(|(i, p)| (i, Props(p))).collect(),
                )
            })
            .collect();
        objects.sort_by(|a, b| a.0.cmp(&b.0));

        let mut state = BluetoothState {
            service: true,
            ..Default::default()
        };
        let Some((path, adapter)) = objects
            .iter()
            .find_map(|(path, i)| Some((path, i.get(ADAPTER_IFACE)?)))
        else {
            return state;
        };
        let adapter = Adapter {
            name: adapter
                .str("Alias")
                .or_else(|| adapter.str("Name"))
                .unwrap_or_default(),
            address: adapter.str("Address").unwrap_or_default(),
            powered: adapter.bool("Powered").unwrap_or(false),
            discovering: adapter.bool("Discovering").unwrap_or(false),
            path: path.clone(),
        };

        for (path, ifaces) in &objects {
            let Some(d) = ifaces.get(DEVICE_IFACE) else {
                continue;
            };
            if d.str("Adapter").as_deref() != Some(adapter.path.as_str()) {
                continue;
            }
            let address = d.str("Address").unwrap_or_default();
            let named = d.str("Name").is_some();
            state.devices.push(Device {
                path: path.clone(),
                name: d.str("Alias").unwrap_or_else(|| address.clone()),
                address,
                named,
                icon: d.str("Icon"),
                paired: d.bool("Paired").unwrap_or(false) || d.bool("Bonded").unwrap_or(false),
                trusted: d.bool("Trusted").unwrap_or(false),
                connected: d.bool("Connected").unwrap_or(false),
                rssi: d.i16("RSSI"),
                battery: ifaces.get(BATTERY_IFACE).and_then(|b| b.u8("Percentage")),
            });
        }
        state.devices.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        state.adapter = Some(adapter);
        state
    }

    fn handle(&self, req: Request, state: &BluetoothState) {
        let bus = self.bus.clone();
        let emit = self.emit.clone();
        let adapter = state.adapter.as_ref().map(|a| a.path.clone());
        let name_of = |path: &str| {
            state
                .devices
                .iter()
                .find(|d| d.path == path)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "the device".into())
        };
        match req {
            Request::SetPowered(on) => {
                let Some(adapter) = adapter else { return };
                tokio::spawn(async move {
                    if let Err(e) = set_powered(&bus, &adapter, on).await {
                        let verb = if on { "on" } else { "off" };
                        emit(Event::Error(format!(
                            "Couldn't turn Bluetooth {verb}: {}",
                            describe_error(&e)
                        )));
                    }
                });
            }
            Request::Discover(on) => {
                let Some(adapter) = adapter else { return };
                tokio::spawn(async move {
                    let method = if on {
                        "StartDiscovery"
                    } else {
                        "StopDiscovery"
                    };
                    // Fails harmlessly when already (not) discovering.
                    if let Err(e) = bus.invoke(&adapter, ADAPTER_IFACE, method, &()).await {
                        log::debug!("{method}: {e}");
                    }
                });
            }
            Request::Connect(path) => {
                let name = name_of(&path);
                tokio::spawn(busy(emit.clone(), path.clone(), async move {
                    bus.invoke(&path, DEVICE_IFACE, "Connect", &())
                        .await
                        .map_err(|e| format!("Couldn't connect to {name}: {}", describe_error(&e)))
                }));
            }
            Request::Disconnect(path) => {
                let name = name_of(&path);
                tokio::spawn(busy(emit.clone(), path.clone(), async move {
                    bus.invoke(&path, DEVICE_IFACE, "Disconnect", &())
                        .await
                        .map_err(|e| format!("Couldn't disconnect {name}: {}", describe_error(&e)))
                }));
            }
            Request::Pair(path) => {
                let name = name_of(&path);
                tokio::spawn(busy(emit.clone(), path.clone(), async move {
                    pair(&bus, &path, &name).await
                }));
            }
            Request::Remove(path) => {
                let Some(adapter) = adapter else { return };
                let name = name_of(&path);
                tokio::spawn(async move {
                    let device = match ObjectPath::try_from(path.as_str()) {
                        Ok(p) => p,
                        Err(e) => return emit(Event::Error(e.to_string())),
                    };
                    if let Err(e) = bus
                        .invoke(&adapter, ADAPTER_IFACE, "RemoveDevice", &(device,))
                        .await
                    {
                        emit(Event::Error(format!(
                            "Couldn't forget {name}: {}",
                            describe_error(&e)
                        )));
                    }
                });
            }
            Request::Reply { id, reply } => {
                if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(reply);
                }
            }
        }
    }
}

/// Runs `work` with the device marked busy, reporting a failure as an error.
async fn busy(emit: Emit<Event>, path: String, work: impl Future<Output = Result<(), String>>) {
    emit(Event::Busy {
        path: path.clone(),
        busy: true,
    });
    let result = work.await;
    emit(Event::Busy { path, busy: false });
    if let Err(message) = result {
        emit(Event::Error(message));
    }
}

async fn set_powered(bus: &Bus, adapter: &str, on: bool) -> zbus::Result<()> {
    let result = bus
        .set(adapter, ADAPTER_IFACE, "Powered", Value::from(on))
        .await;
    let blocked = matches!(&result, Err(e) if {
        let text = describe_error(e).to_lowercase();
        text.contains("rfkill") || text.contains("blocked")
    });
    if on && blocked {
        // Soft-blocked; the session user may unblock it (uaccess on /dev/rfkill).
        let unblocked = tokio::process::Command::new("rfkill")
            .args(["unblock", "bluetooth"])
            .status()
            .await;
        if matches!(unblocked, Ok(s) if s.success()) {
            tokio::time::sleep(Duration::from_millis(400)).await;
            return bus
                .set(adapter, ADAPTER_IFACE, "Powered", Value::from(on))
                .await;
        }
    }
    result
}

async fn pair(bus: &Bus, path: &str, name: &str) -> Result<(), String> {
    match bus.invoke(path, DEVICE_IFACE, "Pair", &()).await {
        Ok(()) => {}
        Err(zbus::Error::MethodError(err, _, _))
            if err.as_str() == "org.bluez.Error.AlreadyExists" => {}
        Err(e) => return Err(format!("Couldn't pair with {name}: {}", describe_error(&e))),
    }
    // Trusted devices reconnect on their own and don't need service authorization.
    if let Err(e) = bus
        .set(path, DEVICE_IFACE, "Trusted", Value::from(true))
        .await
    {
        log::debug!("trust {path}: {e}");
    }
    bus.invoke(path, DEVICE_IFACE, "Connect", &())
        .await
        .map_err(|e| {
            format!(
                "Paired with {name}, but couldn't connect: {}",
                describe_error(&e)
            )
        })
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
}

fn rejected() -> AgentError {
    AgentError::Rejected("Rejected by user".into())
}

/// The pairing agent BlueZ calls into (`org.bluez.Agent1`).
struct Agent {
    bus: Bus,
    emit: Emit<Event>,
    pending: Pending,
    next: AtomicU32,
}

impl Agent {
    async fn device_name(&self, device: &str) -> String {
        match self.bus.props(device, DEVICE_IFACE).await {
            Ok(p) => p.str("Alias").unwrap_or_else(|| "A device".into()),
            Err(_) => "A device".into(),
        }
    }

    /// Shows `prompt` and waits for the answer (rejecting if the UI drops it).
    async fn ask(&self, device: &OwnedObjectPath, prompt: Prompt) -> Reply {
        let device = self.device_name(device.as_str()).await;
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        (self.emit)(Event::Prompt { id, device, prompt });
        rx.await.unwrap_or(Reply::Reject)
    }

    async fn show(&self, device: &OwnedObjectPath, prompt: Prompt) {
        let device = self.device_name(device.as_str()).await;
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        (self.emit)(Event::Prompt { id, device, prompt });
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    async fn release(&self) {}

    async fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, AgentError> {
        match self.ask(&device, Prompt::PinCode).await {
            Reply::PinCode(pin) => Ok(pin),
            _ => Err(rejected()),
        }
    }

    async fn display_pin_code(&self, device: OwnedObjectPath, pincode: String) {
        self.show(&device, Prompt::DisplayPinCode { pin: pincode })
            .await;
    }

    async fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, AgentError> {
        match self.ask(&device, Prompt::Passkey).await {
            Reply::Passkey(key) => Ok(key),
            _ => Err(rejected()),
        }
    }

    async fn display_passkey(&self, device: OwnedObjectPath, passkey: u32, entered: u16) {
        // Called again for every key typed on the device; show it once.
        if entered == 0 {
            self.show(&device, Prompt::DisplayPasskey { passkey }).await;
        }
    }

    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), AgentError> {
        match self.ask(&device, Prompt::Confirm { passkey }).await {
            Reply::Accept => Ok(()),
            _ => Err(rejected()),
        }
    }

    async fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), AgentError> {
        match self.ask(&device, Prompt::Authorize).await {
            Reply::Accept => Ok(()),
            _ => Err(rejected()),
        }
    }

    async fn authorize_service(
        &self,
        device: OwnedObjectPath,
        _uuid: String,
    ) -> Result<(), AgentError> {
        // Services of devices the user paired are fine; others aren't asked about.
        match self.bus.props(device.as_str(), DEVICE_IFACE).await {
            Ok(p) if p.bool("Paired").unwrap_or(false) => Ok(()),
            _ => Err(rejected()),
        }
    }

    async fn cancel(&self) {
        self.pending.lock().unwrap().clear();
        (self.emit)(Event::PromptCanceled);
    }
}
