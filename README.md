# Plex Desktop

A native Linux desktop application for Plex that wraps `app.plex.tv` in a webview and adds custom protocol deep link support to navigate directly to library items.

## Features

- Native Linux desktop app using Tauri
- Webview wrapper for `app.plex.tv`
- Custom protocol deep linking (`plex-desktop://`)
- Configurable server support (local and remote)
- Multiple server management
- Automatic server URL resolution
- OAuth authentication via system browser (credentials captured on redirect)
- Manual server configuration support

## Installation

### Development

1. Install dependencies:
```bash
npm install
```

2. Run in development mode:
```bash
npm run tauri dev
```

### Building

Build the application:
```bash
npm run tauri build
```

This will create a distributable package in `src-tauri/target/release/bundle/`.

## OAuth Authentication

The app handles OAuth authentication by redirecting to the system's default browser and capturing credentials upon redirect.

### How It Works

1. **OAuth Detection**: When the app detects an OAuth/login URL (e.g., `plex.tv/auth`, `plex.tv/login`), it automatically opens the URL in your system's default browser instead of the webview.

2. **Browser Authentication**: You complete the authentication flow in your browser (login, 2FA, etc.).

3. **Callback Handling**: After successful authentication, Plex redirects to a callback URL. The app captures this callback via the custom protocol handler:
   - Callback format: `plex-desktop://auth?token={token}` or `plex-desktop://auth?url={callback_url}`
   - The token is extracted and stored securely in the app's configuration

4. **Token Storage**: The authentication token is stored in `~/.config/plex-desktop/config.json` and used for subsequent API requests.

### Configuring OAuth Redirect

For the OAuth flow to work, Plex needs to be configured to redirect to the custom protocol. However, since Plex controls their OAuth flow, you may need to:

- **Option 1**: Use the app's built-in OAuth detection which automatically opens OAuth URLs in the browser
- **Option 2**: Manually trigger OAuth by calling the `open_in_browser` command with the OAuth URL
- **Option 3**: If Plex supports custom redirect URLs, configure it to redirect to `plex-desktop://auth`

### Manual Token Entry

If automatic OAuth capture doesn't work, you can manually set the token:

```javascript
// From the frontend
await invoke('set_auth_token', { token: 'your-plex-token' });
```

You can obtain your Plex token from:
- Plex Web UI: Network tab → Look for `X-Plex-Token` in API requests
- Plex.tv account settings
- Plex Media Server settings

## Configuration

The app stores server configurations in `~/.config/plex-desktop/config.json`.

### Quick Setup (Recommended)

If auto-discovery doesn't work, use the helper script:

```bash
./add-server.sh
```

The script will:
1. Ask for your server name and URL
2. Automatically fetch the machine identifier
3. Create or update the config file

### Manual Setup

For detailed manual setup instructions, see [MANUAL_SETUP.md](MANUAL_SETUP.md).

**Quick manual steps:**

1. **Find your server's machine identifier:**
   ```bash
   curl http://localhost:32400/ | grep machineIdentifier
   ```

2. **Edit the config file:**
   ```bash
   nano ~/.config/plex-desktop/config.json
   ```

3. **Add your server** (see `config.example.json` for format):
   ```json
   {
     "servers": [
       {
         "id": "server-1",
         "name": "My Plex Server",
         "base_url": "http://localhost:32400",
         "is_remote": false,
         "machine_identifier": "YOUR_MACHINE_ID_HERE"
       }
     ],
     "default_server_id": "server-1"
   }
   ```

4. **Restart the app**

### Adding Servers Programmatically

You can also add servers using Tauri commands (see API Reference below).

### Server URL Resolution Priority

When handling deep links, the app resolves server URLs in this order:

1. **Deep link override**: If `baseUrl` is provided in the deep link, it's used directly
2. **Server ID match**: If `serverId` is provided, the app looks up the matching server by `machineIdentifier`
3. **Default server**: Uses the configured default server
4. **First server**: Falls back to the first server in the list if no default is set

## Deep Link Formats

The app supports multiple deep link formats:

### Format 1: Direct Plex URL
```
plex-desktop://open?url=http%3A%2F%2F192.168.1.100%3A32400%2Fweb%2Findex.html%23%21%2Fserver%2Fabc123%2Fdetails%3Fkey%3D%2Flibrary%2Fmetadata%2F94153
```

### Format 2: Server + Key
```
plex-desktop://server/abc123/details?key=/library/metadata/94153
```
Uses the configured server matching the `serverId` (machineIdentifier).

### Format 3: Base URL + Server ID + Key
```
plex-desktop://open?baseUrl=http%3A%2F%2F192.168.1.100%3A32400&serverId=abc123&key=%2Flibrary%2Fmetadata%2F94153
```
Full construction with URL override.

### Format 4: Base URL + Key (Override)
```
plex-desktop://open?baseUrl=http%3A%2F%2F192.168.1.100%3A32400&key=%2Flibrary%2Fmetadata%2F94153
```
Override server URL for this link only.

### Format 5: Key Only
```
plex-desktop://open?key=/library/metadata/94153
```
Uses the default server from configuration.

## Protocol Handler Registration (Linux)

To register the `plex-desktop://` protocol handler on Linux, run the registration script:

```bash
./register-protocol.sh
```

This script will:
1. Find your plex-desktop binary (release or debug build)
2. Install the `.desktop` file to `~/.local/share/applications/`
3. Update the desktop database
4. Register the `x-scheme-handler/plex-desktop` MIME type

### Manual Registration

If the script doesn't work, you can register manually:

1. Install the `.desktop` file (update `Exec` path to your binary):
```bash
cp src-tauri/plex-desktop.desktop ~/.local/share/applications/
# Edit ~/.local/share/applications/plex-desktop.desktop and update Exec= to point to your binary
update-desktop-database ~/.local/share/applications/
```

2. Register the MIME type:
```bash
xdg-mime default plex-desktop.desktop x-scheme-handler/plex-desktop
```

3. Verify registration:
```bash
xdg-mime query default x-scheme-handler/plex-desktop
# Should output: plex-desktop.desktop
```

**Note:** After registration, you may need to log out and back in, or restart your desktop environment for the changes to take effect.

## API Reference

### Tauri Commands

The app exposes the following Tauri commands:

**Server Management:**
- `get_servers()` - Retrieve all configured servers
- `add_server(name: string, base_url: string, is_remote: boolean)` - Add new server
- `update_server(id: string, name?: string, base_url?: string)` - Update existing server
- `remove_server(id: string)` - Remove server
- `set_default_server(id: string)` - Set default server
- `get_default_server()` - Get default server configuration

**Navigation & OAuth:**
- `navigate_to_deep_link(url: string)` - Navigate to a deep link URL
- `get_auth_token()` - Get stored authentication token
- `set_auth_token(token: string)` - Set authentication token manually
- `handle_oauth_callback(url: string)` - Handle OAuth callback URL
- `open_in_browser(url: string)` - Open URL in system browser

## Testing Deep Links

You can test deep links using:

```bash
# Using xdg-open (Linux)
xdg-open "plex-desktop://server/abc123/details?key=/library/metadata/94153"

# Or directly if the app is running
./target/release/plex-desktop "plex-desktop://server/abc123/details?key=/library/metadata/94153"
```

## Architecture

The app uses:
- **Tauri 2** for the desktop framework
- **Rust** backend for deep link handling and configuration
- **WebView** to display `app.plex.tv`
- **JSON** configuration file for server management

## Development

### Project Structure

```
plex-desktop-app/
├── src/              # Frontend (TypeScript)
├── src-tauri/        # Rust backend
│   ├── src/
│   │   ├── main.rs   # Entry point
│   │   └── lib.rs    # Core logic (deep links, config)
│   └── tauri.conf.json
└── package.json
```

### Key Components

- **Configuration System**: Manages server configurations in `~/.config/plex-desktop/config.json`
- **Deep Link Parser**: Parses various deep link formats and extracts components
- **Server Resolver**: Resolves server URLs based on priority rules
- **URL Constructor**: Builds Plex web URLs from deep link components
- **Navigation Handler**: Navigates the webview to constructed URLs

## Troubleshooting

### Deep links not working

1. Ensure the protocol handler is registered (see Protocol Handler Registration)
2. Check that the app is built and installed correctly
3. Verify server configuration exists in `~/.config/plex-desktop/config.json`

### Server not found errors

1. Add a server using the Tauri commands or edit the config file directly
2. Set a default server if you have multiple servers
3. Ensure server URLs are valid (start with `http://` or `https://`)

## License

[MIT License](LICENSE.md)
