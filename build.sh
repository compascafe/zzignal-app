#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Building Polymarket Dashboard ==="

# Build Rust backend
echo "[1/2] Building Rust backend..."
cd "$SCRIPT_DIR/backend_rust"
cargo build --release
echo "Rust backend: target/release/dashboard_poly"

# Build Java frontend
echo "[2/2] Building Java frontend..."
cd "$SCRIPT_DIR/frontend_java"
mvn clean package -DskipTests
echo "Java frontend: target/dashboard.jar"

echo ""
echo "=== Build complete ==="
echo "Run backend:  ./target/release/dashboard_poly"
echo "Run frontend: java -jar frontend_java/target/dashboard.jar"
