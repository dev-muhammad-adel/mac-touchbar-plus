
use serde::Serialize;
use zbus::{Connection, MessageType, MessageStream, MatchRule};
use zbus::zvariant::OwnedObjectPath;
use zbus::fdo::DBusProxy;
use futures_lite::stream::StreamExt;
use tokio::sync::watch;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct SessionState {
    pub session_type: String,  // "x11", "wayland", or empty string
    pub is_logged_in: bool,    // true only for active X11/Wayland sessions
    pub user: String,          // username or empty string
    pub leader: Option<u32>,   // Session leader PID, if available
}

// Configuration for session monitoring
const DEBUG_LOGGING: bool = false;
const SESSION_CHECK_THROTTLE_MS: u64 = 1000; // Throttle session checks to avoid spam

async fn check_session_state(connection: &Connection, tx: &watch::Sender<SessionState>) -> zbus::Result<()> {
    if DEBUG_LOGGING {
        println!("[session_monitor] check_session_state: Starting session state check...");
    }
    
    let manager_proxy: zbus::Proxy<'_> = zbus::ProxyBuilder::new_bare(connection)
        .destination("org.freedesktop.login1")?
        .path("/org/freedesktop/login1")?
        .interface("org.freedesktop.login1.Manager")?
        .build()
        .await?;

    let reply = manager_proxy
        .call_method("ListSessions", &())
        .await?;
    let sessions: Vec<(String, u32, String, String, OwnedObjectPath)> = reply.body()?;

    let mut found_graphical_session: Option<(String, String, Option<u32>)> = None; // (session_type, user, leader_pid)

    for (_session_id, _uid, username, seat, path) in &sessions {
        // Only consider sessions with a seat
        if seat.is_empty() {
            continue;
        }

        let session_proxy: zbus::Proxy<'_> = zbus::ProxyBuilder::new_bare(connection)
            .destination("org.freedesktop.login1")?
            .path(path.as_str())?
            .interface("org.freedesktop.login1.Session")?
            .build()
            .await?;

        let class: String = session_proxy.get_property("Class").await.unwrap_or_else(|_| "unknown".into());
        let state: String = session_proxy.get_property("State").await.unwrap_or_else(|_| "unknown".into());
        let session_type: String = session_proxy.get_property("Type").await.unwrap_or_else(|_| "unknown".into());
        let user: String = session_proxy.get_property("User").await.unwrap_or_else(|_| username.clone());
        let leader: Option<u32> = session_proxy.get_property("Leader").await.ok().and_then(|l: zbus::zvariant::Value| {
            match l {
                zbus::zvariant::Value::U32(pid) => Some(pid),
                zbus::zvariant::Value::U64(pid) => Some(pid as u32),
                _ => None,
            }
        });

        if DEBUG_LOGGING {
            println!("[session_monitor] Session: class={}, state={}, type={}, user={}, seat={}, leader={:?}", 
                    class, state, session_type, user, seat, leader);
        }

        // Check for active graphical user session (X11 or Wayland)
        if class == "user" && (state == "active" || state == "online") {
            match session_type.as_str() {
                "x11" | "wayland" => {
                    found_graphical_session = Some((session_type.clone(), user.clone(), leader));
                    if DEBUG_LOGGING {
                        println!("[session_monitor] Found active graphical session: type={}, user={}, leader={:?}", session_type, user, leader);
                    }
                    break;
                }
                _ => {
                    // Any other session type (tty, unspecified, etc.) is not considered graphical
                    if DEBUG_LOGGING {
                        println!("[session_monitor] Session not graphical: class={}, type={}, user={}", class, session_type, user);
                    }
                }
            }
        } else {
            // Not a user session, or not active/online, or no seat - not logged into graphical session
            if DEBUG_LOGGING {
                println!("[session_monitor] Session not logged into graphical: class={}, state={}, type={}, user={}, seat={}", 
                        class, state, session_type, user, seat);
            }
        }

        // Note: We don't need to track greeter separately anymore since any non-graphical session
        // (including greeter) will result in is_logged_in: false
    }

    // Fallback: Environment-based detection for cases where login1 doesn't report correctly
    if found_graphical_session.is_none() {
        if DEBUG_LOGGING {
            println!("[session_monitor] No session found via login1, trying environment-based detection");
        }
        
        // Check for X11 display
        if let Ok(display) = std::env::var("DISPLAY") {
            if !display.is_empty() {
                if let Ok(user) = std::env::var("USER") {
                    if !user.is_empty() && user != "root" {
                        found_graphical_session = Some(("x11".to_string(), user.clone(), None));
                        if DEBUG_LOGGING {
                            println!("[session_monitor] Found X11 session via environment: user={}", user);
                        }
                    }
                }
            }
        }
        // Check for Wayland display
        else if let Ok(wayland_display) = std::env::var("WAYLAND_DISPLAY") {
            if !wayland_display.is_empty() {
                if let Ok(user) = std::env::var("USER") {
                    if !user.is_empty() && user != "root" {
                        found_graphical_session = Some(("wayland".to_string(), user.clone(), None));
                        if DEBUG_LOGGING {
                            println!("[session_monitor] Found Wayland session via environment: user={}", user);
                        }
                    }
                }
            }
        }
    }

    let new_state = if let Some((session_type, user, leader_pid)) = found_graphical_session {
        SessionState {
            session_type: session_type,
            is_logged_in: true,  // Active graphical session
            user: user,
            leader: leader_pid,
        }
    } else {
        SessionState {
            session_type: "".to_string(),  // No graphical session
            is_logged_in: false,           // Not logged in to graphical session
            user: "".to_string(),
            leader: None,
        }
    };

    if DEBUG_LOGGING {
        println!("[session_monitor] check_session_state: Sending state: {:?}", new_state);
    }
    
    tx.send(new_state).map_err(|_| zbus::Error::Failure("Send failed".into()))
}

pub async fn monitor_sessions(tx: watch::Sender<SessionState>) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let mut stream = MessageStream::from(&connection);
    let mut last_check = Instant::now();

    println!("[session_monitor] Starting session monitor...");

    // Subscribe to D-Bus signals
    let dbus_proxy = DBusProxy::new(&connection).await?;
    
    // Subscribe to all relevant login1 signals
    let rules = vec![
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.login1.Manager")?
            .build(),
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.DBus.Properties")?
            .path_namespace("/org/freedesktop/login1")?
            .build(),
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.login1.Session")?
            .build(),
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.login1.User")?
            .build(),
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.login1.Seat")?
            .build(),
    ];

    for rule in rules {
        dbus_proxy.add_match_rule(rule).await?;
    }

    // Send initial session state
    check_session_state(&connection, &tx).await?;

    println!("🟢 Monitoring session changes...");
    println!("[session_monitor] To manually check session state, send SIGUSR1 to the process");

    // Manual trigger via signal
    let connection_clone = connection.clone();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sig = signal(SignalKind::user_defined1()).unwrap();
        while sig.recv().await.is_some() {
            println!("[session_monitor] Manual trigger via SIGUSR1");
            if let Err(e) = check_session_state(&connection_clone, &tx_clone).await {
                println!("[session_monitor] Manual check error: {:?}", e);
            }
        }
    });

    // Main event loop with throttling
    while let Some(msg) = stream.next().await {
        let msg = msg?;
        let header = msg.header()?;
        
        // Only process signal messages
        if msg.message_type() != MessageType::Signal {
            continue;
        }
        
        // Throttle session checks to avoid spam
        let now = Instant::now();
        if now.duration_since(last_check) < Duration::from_millis(SESSION_CHECK_THROTTLE_MS) {
            continue;
        }
        
        if let Some(member) = header.member()? {
            let member_str = member.as_str();
            
            // Only check session state for relevant signals
            let should_check = matches!(member_str, 
                "SessionNew" | "SessionRemoved" | "UserNew" | "UserRemoved" | 
                "SeatNew" | "SeatRemoved" | "PropertiesChanged"
            ) || member_str.contains("Session") || member_str.contains("User") || member_str.contains("Login");
            
            if should_check {
                if DEBUG_LOGGING {
                    println!("[session_monitor] Processing signal: {}", member_str);
                }
                check_session_state(&connection, &tx).await?;
                last_check = now;
            }
        }
    }

    Ok(())
}
