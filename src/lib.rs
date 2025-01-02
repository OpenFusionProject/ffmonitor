use std::{
    io::{BufRead as _, BufReader},
    net::{SocketAddr, TcpStream},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc, LazyLock, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use log::*;
use regex::Regex;

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
impl PlayerEvent {
    fn parse(line: &str) -> Result<Self> {
        // player <x> <y> <name...>
        const PATTERN: &str = r"^player (\d+) (\d+) (.+)$";
        static REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(PATTERN).unwrap());

        let captures = REGEX.captures(line).ok_or("Malformed")?;
        let x_coord = captures[1].parse().map_err(|_| "Invalid x coordinate")?;
        let y_coord = captures[2].parse().map_err(|_| "Invalid y coordinate")?;
        let name = captures[3].to_string();
        Ok(Self {
            x_coord,
            y_coord,
            name,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatKind {
    FreeChat,
    MenuChat,
    BuddyChat,
    BuddyMenuChat,
    GroupChat,
    GroupMenuChat,
    TradeChat,
}
impl FromStr for ChatKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "freechat" => Ok(Self::FreeChat),
            "menuchat" => Ok(Self::MenuChat),
            "buddychat" => Ok(Self::BuddyChat),
            "buddymenuchat" => Ok(Self::BuddyMenuChat),
            "groupchat" => Ok(Self::GroupChat),
            "groupmenuchat" => Ok(Self::GroupMenuChat),
            "tradechat" => Ok(Self::TradeChat),
            other => Err(format!("Unknown chat kind '{}'", other).into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEvent {
    pub kind: ChatKind,
    pub from: String,
    pub to: Option<String>,
    pub message: String,
}
impl ChatEvent {
    fn parse(line: &str) -> Result<Self> {
        // chat [<kind>] <from>: <message...>
        // chat [<kind>] <from> (to <to>): <message...>
        const PATTERN: &str = r"^chat \[(.+?)\] (.+?)(?: \(to (.+)\))?: (.*)$";
        static REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(PATTERN).unwrap());

        let captures = REGEX.captures(line).ok_or("Malformed")?;
        let kind = captures[1].parse()?;
        let from = captures[2].to_string();
        let to = captures.get(3).map(|m| m.as_str().to_string());
        let message = captures[4].to_string();
        Ok(Self {
            kind,
            from,
            to,
            message,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailEvent {
    pub from: String,
    pub to: String,
    pub subject: Option<String>,
    pub body: Vec<String>,
}
impl EmailEvent {
    fn parse(header: &str, body: Vec<String>) -> Result<Self> {
        // email [Email] <from> (to <to>): <<subject>>
        const NO_SUBJECT_IDENTIFIER: &str = "No subject.";
        const PATTERN: &str = r"^email \[Email\] (.+?) \(to (.+?)\): <(.+)>$";
        static REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(PATTERN).unwrap());

        let captures = REGEX.captures(header).ok_or("Malformed")?;
        let from = captures[1].to_string();
        let to = captures[2].to_string();
        let subject = match captures[3].to_string().as_str() {
            NO_SUBJECT_IDENTIFIER => None,
            other => Some(other.to_string()),
        };
        Ok(Self {
            from,
            to,
            subject,
            body,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    Player(PlayerEvent),
    Chat(ChatEvent),
    Email(EmailEvent),
}

fn get_first_token(line: &str) -> Option<&str> {
    line.split_whitespace().next()
}

fn listen(addr: SocketAddr, callback: Arc<MonitorNotificationCallback>) -> Result<()> {
    info!("Connecting to monitor at {}", addr);
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    callback(MonitorNotification::Connected);
    let mut reader = BufReader::new(stream);
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        if !reader.read_line(&mut line).is_ok_and(|n| n > 0) {
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
            let first_line = lines.remove(0);
            let event = match get_first_token(&first_line) {
                Some("player") => match PlayerEvent::parse(&first_line) {
                    Ok(event) => Event::Player(event),
                    Err(err) => {
                        warn!("Bad player event ({}): {}", err, first_line);
                        continue;
                    }
                },
                Some("chat") => match ChatEvent::parse(&first_line) {
                    Ok(event) => Event::Chat(event),
                    Err(err) => {
                        warn!("Bad chat event ({}): {}", err, first_line);
                        continue;
                    }
                },
                Some("email") => {
                    // next lines with tabs at the beginning are part of the email body
                    let mut body = Vec::new();
                    while !lines.is_empty() && lines[0].starts_with('\t') {
                        body.push(lines.remove(0).trim_start().to_string());
                    }
                    if lines.is_empty() || !lines[0].starts_with("endemail") {
                        warn!("Malformed email event (no endemail)");
                        continue;
                    }
                    lines.remove(0); // remove endemail
                    match EmailEvent::parse(&first_line, body) {
                        Ok(event) => Event::Email(event),
                        Err(err) => {
                            warn!("Bad email event header ({}): {}", err, first_line);
                            continue;
                        }
                    }
                }
                Some(_) => {
                    warn!("Unknown event: {}", first_line);
                    continue;
                }
                None => {
                    warn!("Empty line in monitor update");
                    continue;
                }
            };
            events.push(event);
        }

        callback(MonitorNotification::Updated(MonitorUpdate { events }));
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
