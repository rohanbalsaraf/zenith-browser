# Contributing to Zenith Browser

Thank you for your interest in contributing to Zenith Browser! This document provides guidelines and instructions for contributing.

## Getting Started

### Prerequisites
- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [Node.js](https://nodejs.org/) 18+ 
- macOS, Linux, or Windows

### Setup Development Environment

```bash
# Clone the repository
git clone https://github.com/yourusername/zenith-browser.git
cd zenith-browser

# Install frontend dependencies
cd frontend
npm install
cd ..

# Build frontend
cd frontend && npm run build && cd ..

# Run development backend
cargo run
```

## Project Structure

```
zenith-browser/
├── frontend/          # React TypeScript UI
│   ├── src/
│   │   ├── components/    # Reusable React components
│   │   ├── lib/          # Utilities
│   │   ├── App.tsx       # Main app
│   │   └── ipc.ts        # IPC bridge
│   └── package.json
├── src/              # Rust backend
│   ├── main.rs      # Entry point
│   ├── app.rs       # Application logic
│   ├── ipc.rs       # IPC handlers
│   ├── db.rs        # Database
│   └── tab.rs       # Tab management
└── Cargo.toml
```

## Development Workflow

### Frontend Development

```bash
cd frontend

# Development server
npm run dev

# Build for production
npm run build

# Lint code
npm run lint

# Run tests
npm run test
npm run test:ui  # Interactive test UI
```

### Backend Development

```bash
# Development build
cargo run

# Release build
cargo build --release

# Run tests
cargo test

# Format code
cargo fmt

# Lint with Clippy
cargo clippy -- -D warnings
```

## Making Changes

1. **Create a branch** for your changes
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes** following the code style

3. **Test your changes**
   - Frontend: `npm run test`
   - Backend: `cargo test`

4. **Lint and format**
   - Frontend: `npm run lint`
   - Backend: `cargo fmt && cargo clippy`

5. **Commit with clear messages**
   ```bash
   git commit -m "feat: description of feature"
   git commit -m "fix: description of fix"
   ```

6. **Push and create a Pull Request**
   ```bash
   git push origin feature/your-feature-name
   ```

## Code Style

### Frontend (TypeScript/React)
- Use functional components with hooks
- Use TypeScript for type safety
- Follow ESLint rules
- Use Tailwind CSS for styling

### Backend (Rust)
- Follow Rust naming conventions
- Use `cargo fmt` for formatting
- Fix Clippy warnings
- Add tests for new features

## Commit Message Format

- `feat:` New feature
- `fix:` Bug fix
- `docs:` Documentation changes
- `style:` Code style changes
- `refactor:` Code refactoring
- `ci:` CI/CD changes
- `test:` Test additions/changes

## Pull Request Process

1. Update relevant documentation
2. Ensure all tests pass
3. Ensure code is formatted and linted
4. Provide clear PR description
5. Link related issues

## Reporting Issues

- Use clear, descriptive titles
- Provide steps to reproduce
- Include your OS and version
- Include browser console output if relevant

## Questions?

Feel free to open an issue or discussion for questions!
