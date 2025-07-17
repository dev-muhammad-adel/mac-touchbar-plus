

use serde::Serialize;
use zbus::{Connection, MessageType, MessageStream, MatchRule};
use zbus::zvariant::OwnedObjectPath;
use zbus::fdo::DBusProxy;
use futures_lite::stream::StreamExt;
use tokio::sync::watch;

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct SessionState {
    pub session_type: String,
    pub is_logged_in: bool,
    pub user: String,
}

async fn check_session_state(connection: &Connection, tx: &watch::Sender<SessionState>) -> zbus::Result<()> {
    println!("[session_monitor] check_session_state: Starting session state check...");
    
    // Use basic zbus approach to get sessions
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

    println!("[session_monitor] check_session_state: Found {} sessions", sessions.len());

    for (session_id, uid, username, seat, path) in &sessions {
        println!("[session_monitor] check_session_state: Checking session uid: {}, user: {}, seat: {}, session_id: {}", uid, username, seat, session_id);
        
        let session_proxy: zbus::Proxy<'_> = zbus::ProxyBuilder::new_bare(connection)
            .destination("org.freedesktop.login1")?
            .path(path.as_str())?
            .interface("org.freedesktop.login1.Session")?
            .build()
            .await?;

        let class: String = session_proxy.get_property("Class").await?;
        let state: String = session_proxy.get_property("State").await?;
        
        let _session_type: String = session_proxy.get_property("Type").await
            .unwrap_or_else(|_| "unknown".into());
        let user: String = session_proxy.get_property("User").await
            .unwrap_or_else(|_| username.clone());
        
        if class == "user" && state == "active" {
            let new_state = SessionState {
                session_type: format!("desktop-logged-graphical"),
                is_logged_in: true,
                user,
            };
            
            println!("[session_monitor] check_session_state: Found active user session, sending state: {:?}", new_state);
            let result = tx.send(new_state);
            match &result {
                Ok(_) => println!("[session_monitor] check_session_state: Successfully sent session state"),
                Err(e) => println!("[session_monitor] check_session_state: Failed to send session state: {:?}", e),
            }
            return result.map_err(|_| zbus::Error::Failure("Send failed".into()));
        }
    }

    // Default to login screen if no active user session found
    let login_state = SessionState {
        session_type: "login-screen".into(),
        is_logged_in: false,
        user: "".into(),
    };
    
    println!("[session_monitor] check_session_state: No active user session found, sending login screen state: {:?}", login_state);
    let result = tx.send(login_state);
    match &result {
        Ok(_) => println!("[session_monitor] check_session_state: Successfully sent login screen state"),
        Err(e) => println!("[session_monitor] check_session_state: Failed to send login screen state: {:?}", e),
    }
    result.map_err(|_| zbus::Error::Failure("Send failed".into()))
}

pub async fn monitor_sessions(tx: watch::Sender<SessionState>) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let mut stream = MessageStream::from(&connection);

    println!("[session_monitor] Starting session monitor...");

    // Subscribe to D-Bus signals we want to monitor
    println!("[session_monitor] Subscribing to D-Bus signals...");
    let dbus_proxy = DBusProxy::new(&connection).await?;
    
    // Subscribe to login1 manager signals
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.login1.Manager")?
        .build();
    dbus_proxy.add_match_rule(rule).await?;
    
    // Subscribe to properties changes from login1 objects
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .path_namespace("/org/freedesktop/login1")?
        .build();
    dbus_proxy.add_match_rule(rule).await?;
    
    // Subscribe to individual session signals
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.login1.Session")?
        .build();
    dbus_proxy.add_match_rule(rule).await?;
    
    // Subscribe to user signals
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.login1.User")?
        .build();
    dbus_proxy.add_match_rule(rule).await?;
    
    // Subscribe to seat signals
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.login1.Seat")?
        .build();
    dbus_proxy.add_match_rule(rule).await?;
    
    println!("[session_monitor] Signal subscriptions added successfully");

    // Send initial session state
    println!("[session_monitor] Checking initial session state...");
    check_session_state(&connection, &tx).await?;

    println!("🟢 Monitoring session changes...");
    println!("[session_monitor] To manually check session state, send SIGUSR1 to the process");

    // Manual trigger via signal
    let connection_clone = connection.clone();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        println!("[session_monitor] Starting signal handler...");
        let mut sig = signal(SignalKind::user_defined1()).unwrap();
        while sig.recv().await.is_some() {
            println!("[session_monitor] Manual trigger via SIGUSR1");
            if let Err(e) = check_session_state(&connection_clone, &tx_clone).await {
                println!("[session_monitor] Manual check error: {:?}", e);
            }
        }
    });

    println!("[session_monitor] Starting main event loop...");
    while let Some(msg) = stream.next().await {
        let msg = msg?;
        let header = msg.header()?;
        
        // Only process signal messages
        if msg.message_type() != MessageType::Signal {
            continue;
        }
        
        // Log all event details for debugging
        let interface = match header.interface() {
            Ok(Some(i)) => Some(i.as_str()),
            _ => None,
        };
        let member = match header.member() {
            Ok(Some(m)) => Some(m.as_str()),
            _ => None,
        };
        let path = match header.path() {
            Ok(Some(p)) => Some(p.as_str()),
            _ => None,
        };
        
        println!("[session_monitor] Signal received:");
        println!("  Interface: {:?}", interface);
        println!("  Member: {:?}", member);
        println!("  Path: {:?}", path);
        
        if let Some(member) = header.member()? {
            let member_str = member.as_str();
            println!("[session_monitor] Processing signal: {}", member_str);
            
            if member_str == "SessionNew" {
                println!("[session_monitor] SessionNew signal detected");
                check_session_state(&connection, &tx).await?;
            } else if member_str == "SessionRemoved" {
                println!("[session_monitor] SessionRemoved signal detected");
                check_session_state(&connection, &tx).await?;
            } else if member_str == "UserNew" {
                println!("[session_monitor] UserNew signal detected");
                check_session_state(&connection, &tx).await?;
            } else if member_str == "UserRemoved" {
                println!("[session_monitor] UserRemoved signal detected");
                check_session_state(&connection, &tx).await?;
            } else if member_str == "SeatNew" {
                println!("[session_monitor] SeatNew signal detected");
                check_session_state(&connection, &tx).await?;
            } else if member_str == "SeatRemoved" {
                println!("[session_monitor] SeatRemoved signal detected");
                check_session_state(&connection, &tx).await?;
            } else if member_str == "PropertiesChanged" {
                println!("[session_monitor] PropertiesChanged signal detected");
                check_session_state(&connection, &tx).await?;
            } else if member_str.contains("Session") || member_str.contains("User") || member_str.contains("Login") {
                // Catch any session-related signals we might have missed
                println!("[session_monitor] Session-related signal detected: {}", member_str);
                check_session_state(&connection, &tx).await?;
            } else {
                // Log any other signals to see what's happening
                println!("[session_monitor] Other signal detected: {}", member_str);
                // Don't check session state for every signal to avoid spam
            }
        }
    }

    Ok(())
}
