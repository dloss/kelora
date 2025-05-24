# Makefile for kelora

.PHONY: all build test test-unit test-integration test-full clean help install

# Default target
all: build test

# Build the project
build:
	@echo "🔨 Building kelora..."
	cargo build --release

# Run all tests
test: test-unit test-integration

# Run only unit tests
test-unit:
	@echo "🧪 Running unit tests..."
	cargo test --lib

# Run only integration tests
test-integration:
	@echo "🔄 Running integration tests..."
	cargo test --test integration_tests

# Run comprehensive test suite (includes manual tests)
test-full:
	@echo "🚀 Running comprehensive test suite..."
	@chmod +x test_kelora.sh
	./test_kelora.sh

# Clean build artifacts
clean:
	@echo "🧹 Cleaning build artifacts..."
	cargo clean

# Install dependencies and setup
install:
	@echo "📦 Installing dependencies..."
	cargo fetch

# Run clippy for code quality
lint:
	@echo "🔍 Running clippy..."
	cargo clippy -- -D warnings

# Format code
fmt:
	@echo "✨ Formatting code..."
	cargo fmt

# Check everything (format, lint, test)
check: fmt lint test

# Run the application with sample data
demo:
	@echo "🎬 Running demo..."
	@echo '{"timestamp":"2023-07-18T15:04:23.456Z","level":"ERROR","message":"Demo error","component":"test"}' | cargo run -- -f jsonl -c
	@echo '{"timestamp":"2023-07-18T15:04:24.456Z","level":"INFO","message":"Demo info","component":"test"}' | cargo run -- -f jsonl -c

# Show help
help:
	@echo "Kelora Makefile Commands:"
	@echo ""
	@echo "  make build          - Build the project"
	@echo "  make test           - Run unit and integration tests"
	@echo "  make test-unit      - Run only unit tests"
	@echo "  make test-integration - Run only integration tests"
	@echo "  make test-full      - Run comprehensive test suite with manual tests"
	@echo "  make lint           - Run clippy for code quality"
	@echo "  make fmt            - Format code"
	@echo "  make check          - Run format, lint, and test"
	@echo "  make demo           - Run a quick demo"
	@echo "  make clean          - Clean build artifacts"
	@echo "  make install        - Install dependencies"
	@echo "  make help           - Show this help"