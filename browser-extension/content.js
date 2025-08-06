// Content script for Tiny DFR Browser Helper
// Handles page-specific interactions

// Listen for messages from background script
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (message.type === 'focus_address_bar') {
        focusAddressBar();
    }
});

function focusAddressBar() {
    // Simulate Ctrl+L to focus the address bar
    const event = new KeyboardEvent('keydown', {
        key: 'l',
        code: 'KeyL',
        ctrlKey: true,
        bubbles: true,
        cancelable: true
    });
    
    document.dispatchEvent(event);
    
    // Also try to focus on the address bar directly if possible
    const addressBar = document.querySelector('input[type="url"], input[name="q"], #urlbar, #searchbox');
    if (addressBar) {
        addressBar.focus();
        addressBar.select();
    }
}

// Also listen for keyboard shortcuts that might be useful
document.addEventListener('keydown', (event) => {
    // You can add additional keyboard shortcuts here if needed
    // For example, to handle other browser shortcuts
}); 