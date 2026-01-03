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

# Build for Linux x64
build_linux() {
    echo -e "${YELLOW}Building for Linux x64...${NC}"
    cargo build --release $FEATURES
    cp target/release/xtreme_iptv dist/xtreme_iptv_linux_x64
    echo -e "${GREEN}✓ Linux x64 build complete: dist/xtreme_iptv_linux_x64${NC}"
}

# Build for Linux ARM64
build_linux_arm() {
    echo -e "${YELLOW}Building for Linux ARM64...${NC}"
    
    # Check if target is installed
    if ! rustup target list | grep -q "aarch64-unknown-linux-gnu (installed)"; then
        echo "Installing Linux ARM64 target..."
        rustup target add aarch64-unknown-linux-gnu
    fi
    
    # Check for cross-compiler
    if ! command -v aarch64-linux-gnu-gcc &> /dev/null; then
        echo -e "${RED}Error: aarch64-linux-gnu-gcc not found. Install with:${NC}"
        echo "  Ubuntu/Debian: sudo apt install gcc-aarch64-linux-gnu"
        echo "  Fedora: sudo dnf install gcc-aarch64-linux-gnu"
        echo "  Arch: sudo pacman -S aarch64-linux-gnu-gcc"
        return 1
    fi
    
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
    cargo build --release --target aarch64-unknown-linux-gnu $FEATURES
    cp target/aarch64-unknown-linux-gnu/release/xtreme_iptv dist/xtreme_iptv_linux_arm64
    echo -e "${GREEN}✓ Linux ARM64 build complete: dist/xtreme_iptv_linux_arm64${NC}"
}

# Build for Linux RISC-V 64
build_linux_riscv() {
    echo -e "${YELLOW}Building for Linux RISC-V 64...${NC}"
    
    # Check if target is installed
    if ! rustup target list | grep -q "riscv64gc-unknown-linux-gnu (installed)"; then
        echo "Installing Linux RISC-V 64 target..."
        rustup target add riscv64gc-unknown-linux-gnu
    fi
    
    # Check for cross-compiler
    if ! command -v riscv64-linux-gnu-gcc &> /dev/null; then
        echo -e "${RED}Error: riscv64-linux-gnu-gcc not found. Install with:${NC}"
        echo "  Ubuntu/Debian: sudo apt install gcc-riscv64-linux-gnu"
        echo "  Fedora: sudo dnf install gcc-riscv64-linux-gnu"
        echo "  Arch: sudo pacman -S riscv64-linux-gnu-gcc"
        return 1
    fi
    
    export CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER=riscv64-linux-gnu-gcc
    cargo build --release --target riscv64gc-unknown-linux-gnu $FEATURES
    cp target/riscv64gc-unknown-linux-gnu/release/xtreme_iptv dist/xtreme_iptv_linux_riscv64
    echo -e "${GREEN}✓ Linux RISC-V 64 build complete: dist/xtreme_iptv_linux_riscv64${NC}"
}

# Build for Windows x64 (requires mingw-w64)
build_windows() {
    echo -e "${YELLOW}Building for Windows x64...${NC}"
    
    # Check if target is installed
    if ! rustup target list | grep -q "x86_64-pc-windows-gnu (installed)"; then
        echo "Installing Windows x64 target..."
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
    echo -e "${GREEN}✓ Windows x64 build complete: dist/xtreme_iptv_windows_x64.exe${NC}"
}

# Build for Windows ARM64
build_windows_arm() {
    echo -e "${YELLOW}Building for Windows ARM64...${NC}"
    
    # Check if target is installed
    if ! rustup target list | grep -q "aarch64-pc-windows-gnullvm (installed)"; then
        echo "Installing Windows ARM64 target..."
        rustup target add aarch64-pc-windows-gnullvm
    fi
    
    # Check for llvm-mingw or clang
    if ! command -v aarch64-w64-mingw32-clang &> /dev/null && ! command -v clang &> /dev/null; then
        echo -e "${RED}Error: llvm-mingw or clang not found for ARM64 cross-compilation${NC}"
        echo "Install llvm-mingw from: https://github.com/mstorsjo/llvm-mingw/releases"
        echo "Or use MSVC on native Windows ARM64"
        return 1
    fi
    
    # Note: Internal player may not work for Windows cross-compile
    if [[ -n "$FEATURES" ]]; then
        echo -e "${YELLOW}Warning: Internal player cross-compilation for Windows may fail${NC}"
    fi
    
    cargo build --release --target aarch64-pc-windows-gnullvm $FEATURES
    cp target/aarch64-pc-windows-gnullvm/release/xtreme_iptv.exe dist/xtreme_iptv_windows_arm64.exe
    echo -e "${GREEN}✓ Windows ARM64 build complete: dist/xtreme_iptv_windows_arm64.exe${NC}"
}

# Build for macOS x64 (Intel)
build_macos_x64() {
    echo -e "${YELLOW}Building for macOS x64 (Intel)...${NC}"
    
    # Check if target is installed
    if ! rustup target list | grep -q "x86_64-apple-darwin (installed)"; then
        echo "Installing macOS x64 target..."
        rustup target add x86_64-apple-darwin
    fi
    
    # Check if we're on macOS or have cross-compilation tools
    if [[ "$(uname)" == "Darwin" ]]; then
        cargo build --release --target x86_64-apple-darwin $FEATURES
        cp target/x86_64-apple-darwin/release/xtreme_iptv dist/xtreme_iptv_macos_x64
        echo -e "${GREEN}✓ macOS x64 build complete: dist/xtreme_iptv_macos_x64${NC}"
    else
        echo -e "${RED}Error: macOS cross-compilation from Linux requires OSXCross${NC}"
        echo "See: https://github.com/tpoechtrager/osxcross"
        echo "Or build natively on a Mac"
        return 1
    fi
}

# Build for macOS ARM64 (Apple Silicon)
build_macos_arm() {
    echo -e "${YELLOW}Building for macOS ARM64 (Apple Silicon)...${NC}"
    
    # Check if target is installed
    if ! rustup target list | grep -q "aarch64-apple-darwin (installed)"; then
        echo "Installing macOS ARM64 target..."
        rustup target add aarch64-apple-darwin
    fi
    
    # Check if we're on macOS or have cross-compilation tools
    if [[ "$(uname)" == "Darwin" ]]; then
        cargo build --release --target aarch64-apple-darwin $FEATURES
        cp target/aarch64-apple-darwin/release/xtreme_iptv dist/xtreme_iptv_macos_arm64
        echo -e "${GREEN}✓ macOS ARM64 build complete: dist/xtreme_iptv_macos_arm64${NC}"
    else
        echo -e "${RED}Error: macOS cross-compilation from Linux requires OSXCross${NC}"
        echo "See: https://github.com/tpoechtrager/osxcross"
        echo "Or build natively on a Mac"
        return 1
    fi
}

# Build Universal macOS binary (x64 + ARM64)
build_macos_universal() {
    echo -e "${YELLOW}Building Universal macOS binary...${NC}"
    
    if [[ "$(uname)" != "Darwin" ]]; then
        echo -e "${RED}Error: Universal binary can only be built on macOS${NC}"
        return 1
    fi
    
    # Build both architectures
    build_macos_x64
    build_macos_arm
    
    # Create universal binary with lipo
    echo -e "${YELLOW}Creating Universal binary with lipo...${NC}"
    lipo -create \
        dist/xtreme_iptv_macos_x64 \
        dist/xtreme_iptv_macos_arm64 \
        -output dist/xtreme_iptv_macos_universal
    
    echo -e "${GREEN}✓ macOS Universal build complete: dist/xtreme_iptv_macos_universal${NC}"
}

# Show help
show_help() {
    echo "Usage: ./build.sh [target] [options]"
    echo
    echo "Targets:"
    echo "  linux           - Build for Linux x64"
    echo "  linux-arm       - Build for Linux ARM64 (Raspberry Pi, Snapdragon)"
    echo "  linux-riscv     - Build for Linux RISC-V 64 (StarFive, SiFive)"
    echo "  windows         - Build for Windows x64 (requires mingw-w64)"
    echo "  windows-arm     - Build for Windows ARM64 (requires llvm-mingw)"
    echo "  macos           - Build for macOS x64 (Intel) - requires macOS"
    echo "  macos-arm       - Build for macOS ARM64 (Apple Silicon) - requires macOS"
    echo "  macos-universal - Build Universal macOS binary - requires macOS"
    echo
    echo "  all             - Build for Linux x64 + Windows x64"
    echo "  all-linux       - Build for all Linux platforms (x64 + ARM64 + RISC-V)"
    echo "  all-windows     - Build for all Windows platforms (x64 + ARM64)"
    echo "  all-macos       - Build for all macOS platforms (x64 + ARM64 + Universal)"
    echo "  everything      - Build for ALL platforms"
    echo "  clean           - Clean build artifacts"
    echo
    echo "Options:"
    echo "  --internal-player  - Enable built-in FFmpeg video player"
    echo "                       Requires: libavcodec-dev libavformat-dev libswscale-dev"
    echo
    echo "Examples:"
    echo "  ./build.sh linux"
    echo "  ./build.sh linux-arm"
    echo "  ./build.sh linux-riscv"
    echo "  ./build.sh windows"
    echo "  ./build.sh windows-arm"
    echo "  ./build.sh macos"
    echo "  ./build.sh macos-arm"
    echo "  ./build.sh macos-universal"
    echo "  ./build.sh all-linux"
    echo "  ./build.sh everything"
}

# Main - get first argument that isn't a flag
TARGET="linux"
for arg in "$@"; do
    case "$arg" in
        --internal-player)
            ;;
        linux|linux-arm|linux-riscv|windows|windows-arm|macos|macos-arm|macos-universal|all|all-linux|all-windows|all-macos|everything|clean|help|--help|-h)
            TARGET="$arg"
            ;;
    esac
done

case "$TARGET" in
    linux)
        build_linux
        ;;
    linux-arm)
        build_linux_arm
        ;;
    linux-riscv)
        build_linux_riscv
        ;;
    windows)
        build_windows
        ;;
    windows-arm)
        build_windows_arm
        ;;
    macos)
        build_macos_x64
        ;;
    macos-arm)
        build_macos_arm
        ;;
    macos-universal)
        build_macos_universal
        ;;
    all)
        build_linux
        echo
        build_windows
        echo
        echo -e "${GREEN}=== All builds complete ===${NC}"
        ls -lh dist/
        ;;
    all-linux)
        build_linux
        echo
        build_linux_arm
        echo
        build_linux_riscv
        echo
        echo -e "${GREEN}=== All Linux builds complete ===${NC}"
        ls -lh dist/
        ;;
    all-windows)
        build_windows
        echo
        build_windows_arm
        echo
        echo -e "${GREEN}=== All Windows builds complete ===${NC}"
        ls -lh dist/
        ;;
    all-macos)
        build_macos_x64
        echo
        build_macos_arm
        echo
        build_macos_universal
        echo
        echo -e "${GREEN}=== All macOS builds complete ===${NC}"
        ls -lh dist/
        ;;
    everything)
        build_linux
        echo
        build_linux_arm
        echo
        build_linux_riscv
        echo
        build_windows
        echo
        build_windows_arm
        echo
        if [[ "$(uname)" == "Darwin" ]]; then
            build_macos_x64
            echo
            build_macos_arm
            echo
            build_macos_universal
            echo
        else
            echo -e "${YELLOW}Skipping macOS builds (not on macOS)${NC}"
        fi
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
