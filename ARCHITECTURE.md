# Zenith Browser Architecture

## Overview

Zenith is a minimal, fast web browser built with Rust. It uses a backend-frontend architecture where:

- **Backend (Rust)**: Manages windows, WebViews, events, database, and IPC
- **Frontend (React)**: Provides the UI shell (chrome/toolbar)
- **IPC Bridge**: Enables communication between Rust and React

## Core Components

### Backend (Rust)

#### `main.rs`
- Entry point with Tokio async runtime
- Event loop handling
- Top-level event dispatching
- Menu action handling

#### `app.rs` - `BrowserApp`
Core application state:
- Window management
- Tab management (`Vec<BrowserTab>`)
- WebView context
- Active tab tracking
- Theme management
- Menu integration

Key methods:
- `new_tab()` - Create new browser tab
- `close_tab()` - Close tab and clean up
- `switch_tab()` - Make tab active
- `tab_action()` - Execute browser actions (back, forward, reload)
- `fetch_suggestions()` - Search history/bookmarks
- `sync_tab_data()` - Send data to tab via JavaScript injection

#### `tab.rs` - `BrowserTab`
Individual tab representation:
- WebView instance
- URL and title tracking
- Download handlers
- Permission requests
- IPC message handling
- JavaScript injection for features (notifications, permissions, etc.)

#### `db.rs` - `Database`
SQLite database management:
- **history**: URLs visited with timestamps
- **bookmarks**: User-saved pages
- **downloads**: Download tracking with status
- `search_suggestions()` - Full-text search
- Migration from JSON fallback

#### `ipc.rs` - IPC Protocol
Type-safe message definitions:

**From Frontend to Backend:**
- `new_tab`, `close_tab`, `switch_tab`, `navigate`
- `tab_action` - Browser actions
- `get_suggestions` - Search query
- `permission_decision` - User permission choice
- Settings changes

**From Backend to Frontend:**
- `ChromeStateResult` - Tab state update
- `SuggestionResults` - Search results
- `TabDataResult` - Injected JavaScript payload

#### `ui_handler.rs`
Custom protocol handler for `zenith://`:
- Maps requests to embedded assets
- Serves React SPA from `frontend/dist/`
- Handles asset MIME types

#### `menu.rs` - macOS Menu Integration
Native menu items, keyboard shortcuts.

### Frontend (React/TypeScript)

#### `App.tsx`
Main application component:
- State management (tabs, suggestions, search query)
- IPC setup and event listeners
- Keyboard handling (arrow keys, enter, escape)
- Component composition

#### `ipc.ts`
Type-safe IPC bridge:
- `ZenithIpc` singleton class
- `send()` - Post messages to Rust
- `onState()` - Listen for state changes
- `onSuggestions()` - Listen for suggestions
- Global React functions: `zenithSetState()`, `zenithSetSuggestions()`

#### Components

**`TabBar.tsx`**
- Display active tabs
- Tab switching and closing
- New tab button
- Framer Motion animations

**`Toolbar.tsx`**
- Navigation buttons (back, forward, reload)
- Address bar (search/URL input)
- Bookmark button
- Quick action buttons (history, downloads, settings)

**`SuggestionsDropdown.tsx`**
- Display suggestions (history, bookmarks, tabs)
- Selection highlighting
- Click to navigate

**`PaletteSearch.tsx`**
- Command palette UI
- Search input
- Backdrop overlay

**`ErrorBoundary.tsx`**
- React error boundary
- User-friendly error display
- Reload button for recovery

## Data Flow

### Navigation Example
```
1. User types in address bar & presses Enter
   ↓
2. React sends IPC: { type: 'navigate', url: '...' }
   ↓
3. Rust BrowserApp receives event
   ↓
4. Active tab's webview navigates
   ↓
5. Tab injects monitoring script
   ↓
6. Navigation complete, title/URL changes
   ↓
7. Rust sends IPC: { type: 'chrome_state', ... }
   ↓
8. React updates UI
```

### Suggestion Search Example
```
1. User types in search box
   ↓
2. React sends: { type: 'get_suggestions', query: '...' }
   ↓
3. Rust spawns async task:
   - Query database (bookmarks, history)
   - Get open tab titles
   - Filter and limit results
   ↓
4. Rust sends: { type: 'suggestions_results', [...] }
   ↓
5. React displays dropdown
```

## Key Design Decisions

### Single WebView for UI, Multiple for Content
- Chrome UI runs in a separate transparent WebView (`zenith://assets/ui`)
- Each tab gets its own WebView for isolation
- Reduces complexity vs. full web-based UI

### IPC-Based Communication
- Type-safe message protocol
- Backend remains responsive
- UI updates happen atomically

### Embedded Assets
- `rust-embed` bakes React build into binary
- No runtime file dependencies
- Single executable distribution

### SQLite for Persistence
- Lightweight, serverless
- WAL mode for concurrent access
- Indexes for fast search

### Tab Initialization Script
- Custom JavaScript injected into each tab
- Patches APIs (notifications, permissions)
- Monitors URL/title changes
- Handles secure context spoofing for non-https

## Platform Considerations

### macOS (Primary)
- Uses WKWebView via Wry
- Native menu bar integration with Muda
- Fullscreen support

### Linux (Needs Work)
- Use WebKitGTK via Wry
- No native menu (GTK alternative)

### Windows (Needs Work)
- Use WebView2 via Wry
- Windows menu integration

## Security Model

- WebViews are sandboxed by the OS
- Permission requests go through custom handlers
- No direct DOM access from Rust
- IPC messages are serialized JSON

## Performance Considerations

- Lazy tab creation (background tabs not created until needed)
- Database queries are async
- Suggestion search limited to 15 results
- Large downloads streamed (not buffered)

## Testing

**Frontend:**
- Vitest for unit tests
- React Testing Library for component tests
- Setup mocks for IPC

**Backend:**
- Cargo tests
- URL normalization tests
- Auth window detection tests

## Future Improvements

1. Cross-platform support (Linux, Windows)
2. Browser extensions API
3. Sync across devices
4. Privacy mode
5. Session restore
6. More search engine options
7. Custom shortcuts/keybindings
8. Themes beyond light/dark
