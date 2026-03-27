# Agent Observatory

# Run in dev mode
dev:
    cd frontend && cargo tauri dev

# Build release
build:
    cd frontend && cargo tauri build

# Check Rust backend compiles
check:
    cd frontend/src-tauri && cargo check

# Run frontend only (no Tauri shell)
frontend:
    cd frontend && bun run dev
