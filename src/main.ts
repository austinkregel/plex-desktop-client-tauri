import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

// Plex Desktop App
// Handles OAuth authentication via system browser and token extraction

console.log("Plex Desktop App initialized");

// Listen for messages from the injected script in the webview
window.addEventListener("message", async (event) => {
  // HIGH-002: Only accept messages from the expected origin (app.plex.tv)
  if (event.origin !== "https://app.plex.tv") {
    console.warn("Rejected postMessage from unauthorized origin:", event.origin);
    return;
  }

  // Validate payload shape - ensure event.data is an object
  if (!event.data || typeof event.data !== "object") {
    console.warn("Rejected postMessage with invalid payload shape");
    return;
  }

  if (event.data.type === "plex-token-found") {
    // Validate token is a string
    if (typeof event.data.token !== "string") {
      console.warn("Rejected postMessage with invalid token type");
      return;
    }
    
    const token = event.data.token;
    console.log("Received token from webview via postMessage (redacted)");
    
    // Store the token
    try {
      await invoke("set_auth_token", { token });
      console.log("Token stored successfully");
      
      // Discover servers
      await discoverAndAddServers(token);
    } catch (error) {
      console.error("Failed to store token:", error);
    }
  }
  
  if (event.data.type === "plex-client-id-found") {
    // Validate clientId is a string
    if (typeof event.data.clientId !== "string") {
      console.warn("Rejected postMessage with invalid clientId type");
      return;
    }
    
    const clientId = event.data.clientId;
    console.log("Received clientID from webview:", clientId);
    try {
      await invoke("set_client_id", { clientId });
    } catch (error) {
      console.error("Failed to store clientID:", error);
    }
  }
});

// Listen for OAuth completion events
listen<string>("oauth-complete", async (event) => {
  console.log("OAuth authentication completed", event.payload);
  // Token is now stored, discover servers
  await discoverAndAddServers(event.payload);
});

// Function to discover and add servers using a token
async function discoverAndAddServers(token: string) {
  try {
    console.log("Discovering servers with token...");
    const servers = await invoke<Array<{name: string, base_url: string, machine_identifier: string | null}>>("discover_servers_from_token", { token });
    console.log(`Discovered and added ${servers.length} servers`);
    
    // Emit an event to notify that servers were discovered
    if (servers.length > 0) {
      console.log("Servers discovered:", servers.map(s => s.name));
    }
  } catch (error) {
    console.error("Failed to discover servers:", error);
  }
}

// Periodically check for intercepted token and discover servers
let tokenCheckInterval: number | null = null;
let tokenFound = false;
let lastTokenCheck: string | null = null;

async function checkForToken() {
  try {
    // First, install the token interceptor if not already done (only once)
    if (!tokenFound) {
      await invoke("extract_token_from_webview");
    }
    
    // Check if we have a stored token (in case app was restarted or token was just stored)
    const storedToken = await invoke<string | null>("get_auth_token");
    
    // If we found a token and haven't processed it yet, discover servers
    if (storedToken && storedToken !== lastTokenCheck && !tokenFound) {
      console.log("Found stored token, discovering servers...");
      lastTokenCheck = storedToken;
      tokenFound = true;
      await discoverAndAddServers(storedToken);
      
      // Stop checking once we have a token and servers
      if (tokenCheckInterval !== null) {
        clearInterval(tokenCheckInterval);
        tokenCheckInterval = null;
      }
      return;
    }
    
    // Update last check even if token hasn't changed
    if (storedToken) {
      lastTokenCheck = storedToken;
    } else if (!tokenFound) {
      // No token yet, log for debugging
      console.debug("No token found yet, will keep checking...");
    }
    
  } catch (error) {
    // Log errors for debugging
    console.error("Token check error:", error);
  }
}

// Start checking for tokens after a delay (to let the page load)
setTimeout(() => {
  console.log("Starting token extraction...");
  // Install interceptor
  invoke("extract_token_from_webview");
  
  // Check immediately
  checkForToken();
  
  // Then check every 2 seconds (more frequent to catch tokens faster)
  tokenCheckInterval = window.setInterval(checkForToken, 2000);
  
  // Also check on focus and when page becomes visible
  window.addEventListener("focus", () => {
    setTimeout(checkForToken, 500);
  });
  
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      setTimeout(checkForToken, 500);
    }
  });
  
  // Also listen for storage events (when localStorage changes)
  window.addEventListener("storage", (e) => {
    if (e.key === "myPlexAccessToken" || e.key === "token" || e.key === "authToken") {
      console.log("Storage event detected for token key:", e.key);
      setTimeout(checkForToken, 500);
    }
  });
}, 1000); // Reduced delay to 1 second
