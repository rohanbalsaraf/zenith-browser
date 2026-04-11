# Zenith Browser

A minimal, fast, and elegant web browser built with Rust. Zenith combines native WebView performance with a modern, distraction-free interface powered by React.

> **Status**: ~80% feature-complete. Production-ready for macOS. Linux/Windows support in progress.

## ✨ Features

- **⚡ Blazing Fast**: Native WebView engine (WKWebView on macOS, WebKitGTK on Linux, WebView2 on Windows)
- **🎨 Modern UI**: Glassmorphism design with smooth animations
- **📦 Single Binary**: Everything compiled into one executable
- **🔒 Privacy-First**: No tracking, telemetry, or bloat
- **📚 Smart Suggestions**: Search history, bookmarks, and open tabs
- **🎯 Keyboard-Friendly**: Command palette and keyboard shortcuts
- **🌙 Theme Support**: Light and dark modes
- **📥 Download Manager**: Track your downloads
- **⭐ Bookmarking**: Save your favorite sites
- **🔍 History Search**: Full-text search across your history

## 🚀 Quick Start

### Prerequisites
- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [Node.js](https://nodejs.org/) 18+ (for frontend)

### Development

```bash
# Clone repository
git clone https://github.com/yourusername/zenith-browser.git
cd zenith-browser

# Install frontend dependencies
cd frontend && npm install && cd ..

# Run development build
cargo run
```

### Production Build

```bash
# Build frontend
cd frontend && npm run build && cd ..

# Build optimized binary
cargo build --release

# Run
./target/release/zenith-browser
```

## 📖 Usage

| Action | Method |
|--------|--------|
| **New Tab** | `Cmd+T` or `+` button |
| **Close Tab** | `Cmd+W` or `×` button |
| **Navigate Back** | `←` button or `Cmd+[` |
| **Navigate Forward** | `→` button or `Cmd+]` |
| **Reload Page** | `⟲` button or `Cmd+R` |
| **Search/URL** | Click address bar, or use suggestions with arrow keys + Enter |
| **Bookmark** | `Star` icon in address bar or `Cmd+D` |
| **Search History** | Click `History` button |
| **Downloads** | Click `Download` icon |
| **Settings** | Click `⚙️` button or `Cmd+,` |

## 🏗️ Architecture

Zenith uses a Rust backend + React frontend architecture:

- **Backend**: Manages windows, WebViews, IPC, and database
- **Frontend**: React UI shell with TypeScript
- **IPC Bridge**: Type-safe communication between layers

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed design documentation.

## 📁 Project Structure

```
zenith-browser/
├── frontend/              # React TypeScript UI
│   ├── src/
│   │   ├── components/   # Reusable components (TabBar, Toolbar, etc.)
│   │   ├── App.tsx       # Main application
│   │   ├── ipc.ts        # IPC bridge
│   │   └── ...
│   ├── package.json
│   └── vite.config.ts
├── src/                   # Rust backend
│   ├── main.rs          # Entry point
│   ├── app.rs           # Core application
│   ├── tab.rs           # Tab management
│   ├── ipc.rs           # IPC protocol
│   ├── db.rs            # Database (SQLite)
│   └── ...
└── Cargo.toml
```

## 🛠️ Development

### Frontend Development

```bash
cd frontend
npm run dev        # Start dev server with hot reload
npm run build      # Build for production
npm run lint       # Run ESLint
npm run test       # Run Vitest tests
npm run test:ui    # Interactive test UI
```

### Backend Development

```bash
cargo run          # Debug build
cargo build --release  # Release build
cargo test         # Run tests
cargo fmt          # Format code
cargo clippy       # Lint code
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for complete development guidelines.

## ✅ What's Complete

- ✅ Tab management
- ✅ Navigation (back/forward/reload)
- ✅ Address bar with URL/search
- ✅ Bookmarks & History (SQLite)
- ✅ Suggestions (smart search)
- ✅ Download tracking
- ✅ Permissions UI
- ✅ Theme switching
- ✅ Settings page
- ✅ React component architecture
- ✅ Error boundaries
- ✅ Unit testing framework
- ✅ CI/CD pipelines

## 🚧 In Progress

- 🔄 Linux support (WebKitGTK)
- 🔄 Windows support (WebView2)
- 🔄 More test coverage
- 🔄 Performance optimizations

## ⏳ Future Plans

- [ ] Extensions/plugins API
- [ ] Cloud sync
- [ ] Privacy mode (incognito)
- [ ] Session restore
- [ ] Custom shortcuts
- [ ] More themes
- [ ] Tab groups
- [ ] Reader mode

## 🔄 CI/CD

Zenith uses GitHub Actions for continuous integration:

- **PR checks**: Linting, formatting, tests
- **Builds**: Platform-specific binaries (macOS, Linux, Windows)
- **Releases**: Automated release builds on tags

View workflows in [`.github/workflows/`](.github/workflows)

## 🤝 Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for:
- Setup instructions
- Code style guidelines
- Development workflow
- Pull request process

## 📝 License

This project is licensed under the MIT License - see [LICENSE](LICENSE) for details.

## 💙 Acknowledgments

Built with:
- [Wry](https://github.com/tauri-apps/wry) - WebView rendering
- [Tao](https://github.com/tauri-apps/tao) - Native window management
- [React](https://react.dev) - UI framework
- [Tailwind CSS](https://tailwindcss.com) - Styling
- [SQLx](https://github.com/launchbadge/sqlx) - Database

