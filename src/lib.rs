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

trait FromLineStream: Sized {
    fn from_line_stream(lines: &mut Vec<String>) -> Result<Self>;
}

fn consume_next_token(line: &mut String) -> Option<String> {
    if line.is_empty() {
        return None;
    }

    match line.find(' ') {
        Some(idx) => {
            let token = line.drain(..idx).collect();
            line.drain(..1);
            Some(token)
        }
        None => Some(std::mem::take(line)),
    }
}

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
impl FromLineStream for PlayerEvent {
    fn from_line_stream(lines: &mut Vec<String>) -> Result<Self> {
        // player <x> <y> <name...>
        let mut line = lines.remove(0);
        assert!(consume_next_token(&mut line).is_some_and(|s| s == "player"));
        let x_coord = consume_next_token(&mut line)
            .ok_or("Missing x coordinate")?
            .parse()
            .map_err(|_| "Bad x coordinate")?;
        let y_coord = consume_next_token(&mut line)
            .ok_or("Missing y coordinate")?
            .parse()
            .map_err(|_| "Bad y coordinate")?;
        let name = line;
        Ok(Self {
            x_coord,
            y_coord,
            name,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEvent {
    pub message: String,
}
impl FromLineStream for ChatEvent {
    fn from_line_stream(lines: &mut Vec<String>) -> Result<Self> {
        // chat <message...>
        let mut line = lines.remove(0);
        assert!(consume_next_token(&mut line).is_some_and(|s| s == "chat"));
        let message = line;
        Ok(Self { message })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Player(PlayerEvent),
    Chat(ChatEvent),
}

fn listen(addr: SocketAddr, callback: Arc<MonitorNotificationCallback>) -> Result<()> {
    info!("Connecting to monitor at {}", addr);
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    callback(MonitorNotification::Connected);
    let mut reader = BufReader::new(stream);
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            callback(MonitorNotification::Disconnected);
            return Ok(());
        }
        line.pop(); // remove newline

        if line == "begin" {
            lines.clear();
            continue;
        }

        if line != "end" {
            lines.push(line);
            continue;
        }

        let mut events = Vec::new();
        while !lines.is_empty() {
            let event = match lines[0].split_whitespace().next() {
                Some("player") => match PlayerEvent::from_line_stream(&mut lines) {
                    Ok(event) => Event::Player(event),
                    Err(err) => {
                        warn!("Bad player event: {}", err);
                        continue;
                    }
                },
                Some("chat") => match ChatEvent::from_line_stream(&mut lines) {
                    Ok(event) => Event::Chat(event),
                    Err(err) => {
                        warn!("Bad chat event: {}", err);
                        continue;
                    }
                },
                Some(unknown) => {
                    warn!("Unknown event type: {}", unknown);
                    continue;
                }
                None => {
                    warn!("Empty line in monitor update");
                    continue;
                }
            };
            events.push(event);
        }

        if !events.is_empty() {
            callback(MonitorNotification::Updated(MonitorUpdate { events }));
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
