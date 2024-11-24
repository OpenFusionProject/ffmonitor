use std::{
    io::{BufRead as _, BufReader},
    net::{SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use log::*;

type Error = Box<dyn std::error::Error>;
type Result<T> = std::result::Result<T, Error>;

pub type MonitorNotificationCallback = Box<dyn Fn(MonitorNotification) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum MonitorNotification {
    Connected,
    Updated(MonitorUpdate),
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerEvent {
    pub x_coord: i32,
    pub y_coord: i32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEvent {
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EventInternal {
    Begin,
    Player(PlayerEvent),
    Chat(ChatEvent),
    End,
}
impl TryFrom<&str> for EventInternal {
    type Error = String;
    fn try_from(value: &str) -> std::result::Result<Self, String> {
        let parts: Vec<&str> = value.split_ascii_whitespace().collect();
        if parts.is_empty() {
            return Err("Empty event".into());
        }

        match parts[0] {
            "begin" => Ok(EventInternal::Begin),
            "end" => Ok(EventInternal::End),
            "player" => {
                if parts.len() < 4 {
                    return Err("player: Not enough tokens".into());
                }
                let x_coord = parts[1].parse().map_err(|_| "player: Invalid x coord")?;
                let y_coord = parts[2].parse().map_err(|_| "player: Invalid y coord")?;
                let name = parts[3..].join(" ");
                Ok(EventInternal::Player(PlayerEvent {
                    x_coord,
                    y_coord,
                    name,
                }))
            }
            "chat" => {
                if parts.len() < 2 {
                    return Err("chat: Not enough tokens".into());
                }
                let message = parts[1..].join(" ");
                Ok(EventInternal::Chat(ChatEvent { message }))
            }
            other => Err(format!("Unknown event type: {}", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Player(PlayerEvent),
    Chat(ChatEvent),
}
impl TryFrom<EventInternal> for Event {
    type Error = ();
    fn try_from(event: EventInternal) -> std::result::Result<Self, ()> {
        match event {
            EventInternal::Player(player) => Ok(Event::Player(player)),
            EventInternal::Chat(chat) => Ok(Event::Chat(chat)),
            _ => Err(()),
        }
    }
}

fn listen(addr: SocketAddr, callback: Arc<MonitorNotificationCallback>) -> Result<()> {
    info!("Connecting to monitor at {}", addr);
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    callback(MonitorNotification::Connected);
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut events = Vec::new();
    loop {
        if reader.read_line(&mut line)? == 0 {
            callback(MonitorNotification::Disconnected);
            return Ok(());
        }

        let event_parsed = EventInternal::try_from(line.trim());
        line.clear();
        let event = match event_parsed {
            Ok(event) => event,
            Err(err) => {
                warn!("Failed to parse event ({})", err);
                continue;
            }
        };

        if events.is_empty() && event != EventInternal::Begin {
            warn!("Event received before begin event");
            continue;
        }

        events.push(event);
        if events.last().unwrap() == &EventInternal::End {
            let events_filtered = events
                .iter()
                .filter_map(|event| Event::try_from(event.clone()).ok())
                .collect();
            let update = MonitorUpdate {
                events: events_filtered,
            };
            callback(MonitorNotification::Updated(update));
            events.clear();
        }
    }
}

#[derive(Debug, Clone)]
pub struct MonitorUpdate {
    events: Vec<Event>,
}
impl MonitorUpdate {
    /// Decompose the MonitorUpdate into a Vec of Events
    pub fn get_events(self) -> Vec<Event> {
        self.events
    }

    pub fn get_player_count(&self) -> usize {
        self.events
            .iter()
            .filter(|event| matches!(event, Event::Player(_)))
            .count()
    }
}

pub struct Monitor {
    handle: JoinHandle<()>,
    rx: Receiver<MonitorUpdate>,
    connected: Arc<AtomicBool>,
    last_update: Arc<Mutex<Option<MonitorUpdate>>>,
}
impl Monitor {
    /// Create a new Monitor instance that connects to the given address.
    /// Updates are buffered and can be pulled with `poll()`.
    pub fn new(address: &str) -> Result<Self> {
        Self::new_internal(address, None)
    }

    /// Create a new Monitor instance that connects to the given address.
    /// Updates are passed to the given callback and not buffered.
    pub fn new_with_callback(address: &str, callback: MonitorNotificationCallback) -> Result<Self> {
        Self::new_internal(address, Some(callback))
    }

    fn new_internal(
        address: &str,
        user_callback: Option<MonitorNotificationCallback>,
    ) -> Result<Self> {
        info!("ffmonitor v{}", env!("CARGO_PKG_VERSION"));
        let address: SocketAddr = address.parse()?;
        let (tx, rx) = mpsc::channel();
        let connected = Arc::new(AtomicBool::new(false));
        let last_update = Arc::new(Mutex::new(None));

        let conn = connected.clone();
        let lu = last_update.clone();
        let callback: Arc<MonitorNotificationCallback> = Arc::new(Box::new(move |notification| {
            match notification.clone() {
                MonitorNotification::Connected => conn.store(true, Ordering::Release),
                MonitorNotification::Updated(update) => {
                    *lu.lock().unwrap() = Some(update.clone());
                    if user_callback.is_none() {
                        // don't buffer if user is handling updates
                        let _ = tx.send(update);
                    }
                }
                MonitorNotification::Disconnected => conn.store(false, Ordering::Release),
            }
            if let Some(cb) = &user_callback {
                cb(notification);
            }
        }));

        let handle = thread::spawn({
            move || loop {
                if let Err(err) = listen(address, callback.clone()) {
                    error!("Couldn't connect to monitor: {}", err);
                    thread::sleep(Duration::from_secs(1));
                }
            }
        });

        Ok(Self {
            handle,
            rx,
            connected,
            last_update,
        })
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    pub fn poll(&mut self) -> Option<MonitorUpdate> {
        self.rx.try_recv().ok()
    }

    pub fn get_last_update(&self) -> Option<MonitorUpdate> {
        self.last_update.lock().unwrap().clone()
    }

    pub fn shutdown(self) -> Result<()> {
        self.handle.join().map_err(|_| "Monitor thread panicked")?;
        Ok(())
    }
}
