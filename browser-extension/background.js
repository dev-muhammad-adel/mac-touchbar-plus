// Background service worker for Tiny DFR Browser Helper
// Handles communication with tiny-dfr via HTTP server

let serverUrl = 'http://localhost:3000';
let isConnected = false;
let reconnectInterval = null;

// Initialize connection to tiny-dfr server
function connectToServer() {
    fetch(`${serverUrl}/status`, { method: 'GET' })
        .then(response => {
            if (response.ok) {
                isConnected = true;
                console.log('[Tiny DFR] Connected to server');
                startStatusUpdates();
            }
        })
        .catch(error => {
            console.log('[Tiny DFR] Failed to connect to server:', error);
            isConnected = false;
            scheduleReconnect();
        });
}

function scheduleReconnect() {
    if (reconnectInterval) {
        clearInterval(reconnectInterval);
    }
    reconnectInterval = setInterval(() => {
        if (!isConnected) {
            connectToServer();
        }
    }, 5000); // Try to reconnect every 5 seconds
}

function startStatusUpdates() {
    // Send status updates every 2 seconds when connected
    setInterval(() => {
        if (isConnected) {
            sendBrowserStatus();
        }
    }, 2000);
}

async function sendBrowserStatus() {
    try {
        const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
        if (tabs.length === 0) return;

        const tab = tabs[0];
        const status = {
            url: tab.url,
            title: tab.title,
            favicon_url: tab.favIconUrl,
            can_go_back: tab.canGoBack,
            can_go_forward: tab.canGoForward,
            is_loading: tab.status === 'loading'
        };

        await fetch(`${serverUrl}/status`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(status)
        });
    } catch (error) {
        console.log('[Tiny DFR] Failed to send status:', error);
        isConnected = false;
    }
}

// Handle commands from tiny-dfr
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (message.type === 'command') {
        handleCommand(message.command);
    }
});

async function handleCommand(command) {
    try {
        const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
        if (tabs.length === 0) return;

        const tab = tabs[0];
        
        switch (command) {
            case 'back':
                if (tab.canGoBack) {
                    await chrome.tabs.goBack(tab.id);
                }
                break;
            case 'forward':
                if (tab.canGoForward) {
                    await chrome.tabs.goForward(tab.id);
                }
                break;
            case 'refresh':
                await chrome.tabs.reload(tab.id);
                break;
            case 'home':
                // Navigate to home page (you can customize this)
                await chrome.tabs.update(tab.id, { url: 'https://www.google.com' });
                break;
            case 'add_bookmark':
                // Toggle bookmark
                await chrome.bookmarks.create({
                    parentId: '1', // Bookmarks bar
                    title: tab.title,
                    url: tab.url
                });
                break;
            case 'new_tab':
                await chrome.tabs.create({});
                break;
            case 'focus_address_bar':
                // Send message to content script to focus address bar
                await chrome.tabs.sendMessage(tab.id, { type: 'focus_address_bar' });
                break;
            default:
                console.log('[Tiny DFR] Unknown command:', command);
        }
    } catch (error) {
        console.log('[Tiny DFR] Error executing command:', error);
    }
}

// Initialize on extension load
connectToServer();

// Listen for tab updates to send status immediately
chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
    if (changeInfo.status === 'complete' && tab.active) {
        sendBrowserStatus();
    }
});

chrome.tabs.onActivated.addListener(() => {
    sendBrowserStatus();
}); 