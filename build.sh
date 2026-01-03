#!/bin/bash

# Xtreme IPTV Player - Cross-platform build script

set -e

echo "=== Xtreme IPTV Player Build Script ==="
echo

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check for internal-player feature flag
FEATURES=""
if [[ "$*" == *"--internal-player"* ]]; then
    FEATURES="--features internal-player"
    echo -e "${YELLOW}Building with internal FFmpeg player${NC}"
fi

# Create output directory
mkdir -p dist

# Build for Linux
build_linux() {
    echo -e "${YELLOW}Building for Linux x64...${NC}"
    cargo build --release $FEATURES
    cp target/release/xtreme_iptv dist/xtreme_iptv_linux_x64
    echo -e "${GREEN}✓ Linux build complete: dist/xtreme_iptv_linux_x64${NC}"
}

# Build for Windows (requires mingw-w64)
build_windows() {
    echo -e "${YELLOW}Building for Windows x64...${NC}"
    
    # Check if target is installed
    if ! rustup target list | grep -q "x86_64-pc-windows-gnu (installed)"; then
        echo "Installing Windows target..."
        rustup target add x86_64-pc-windows-gnu
    fi
    
    # Check for mingw
    if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
        echo -e "${RED}Error: mingw-w64 not found. Install with:${NC}"
        echo "  Ubuntu/Debian: sudo apt install mingw-w64"
        echo "  Fedora: sudo dnf install mingw64-gcc"
        echo "  Arch: sudo pacman -S mingw-w64-gcc"
        return 1
    fi
    
    # Note: Internal player may not work for Windows cross-compile
    if [[ -n "$FEATURES" ]]; then
        echo -e "${YELLOW}Warning: Internal player cross-compilation for Windows may fail${NC}"
        echo -e "${YELLOW}FFmpeg libraries must be available for the Windows target${NC}"
    fi
    
    cargo build --release --target x86_64-pc-windows-gnu $FEATURES
    cp target/x86_64-pc-windows-gnu/release/xtreme_iptv.exe dist/xtreme_iptv_windows_x64.exe
    echo -e "${GREEN}✓ Windows build complete: dist/xtreme_iptv_windows_x64.exe${NC}"
}

# Show help
show_help() {
    echo "Usage: ./build.sh [target] [options]"
    echo
    echo "Targets:"
    echo "  linux    - Build for Linux x64"
    echo "  windows  - Build for Windows x64 (requires mingw-w64)"
    echo "  all      - Build for all platforms"
    echo "  clean    - Clean build artifacts"
    echo
    echo "Options:"
    echo "  --internal-player  - Enable built-in FFmpeg video player"
    echo "                       Requires: libavcodec-dev libavformat-dev libswscale-dev"
    echo
    echo "Examples:"
    echo "  ./build.sh linux"
    echo "  ./build.sh linux --internal-player"
    echo "  ./build.sh windows"
    echo "  ./build.sh all"
}

# Main - get first argument that isn't a flag
TARGET="linux"
for arg in "$@"; do
    case "$arg" in
        --internal-player)
            ;;
        linux|windows|all|clean|help|--help|-h)
            TARGET="$arg"
            ;;
    esac
done

case "$TARGET" in
    linux)
        build_linux
        ;;
    windows)
        build_windows
        ;;
    all)
        build_linux
        echo
        build_windows
        echo
        echo -e "${GREEN}=== All builds complete ===${NC}"
        ls -lh dist/
        ;;
    clean)
        echo "Cleaning build artifacts..."
        cargo clean
        rm -rf dist/
        echo -e "${GREEN}✓ Clean complete${NC}"
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        echo -e "${RED}Unknown target: $TARGET${NC}"
        show_help
        exit 1
        ;;
esac
