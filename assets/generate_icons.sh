#!/bin/bash
# Generate icons for Windows, Linux, and macOS from SVG
# Requires: imagemagick (convert), librsvg2-bin (rsvg-convert), icotool (icoutils), png2icns (icnsutils)

set -e

ASSETS_DIR="$(dirname "$0")"
SVG_FILE="$ASSETS_DIR/icon.svg"

echo "Generating icons from $SVG_FILE..."

# Check for required tools
check_tool() {
    if ! command -v $1 &> /dev/null; then
        echo "Warning: $1 not found. Install with: $2"
        return 1
    fi
    return 0
}

# Generate PNG files at various sizes
generate_pngs() {
    echo "Generating PNG files..."
    
    if check_tool rsvg-convert "sudo apt install librsvg2-bin"; then
        for size in 16 32 48 64 128 256 512; do
            rsvg-convert -w $size -h $size "$SVG_FILE" -o "$ASSETS_DIR/icon_${size}x${size}.png"
            echo "  Created icon_${size}x${size}.png"
        done
    elif check_tool convert "sudo apt install imagemagick"; then
        for size in 16 32 48 64 128 256 512; do
            convert -background none -resize ${size}x${size} "$SVG_FILE" "$ASSETS_DIR/icon_${size}x${size}.png"
            echo "  Created icon_${size}x${size}.png"
        done
    else
        echo "Error: Need either rsvg-convert or imagemagick to generate PNGs"
        exit 1
    fi
}

# Generate Windows ICO
generate_ico() {
    echo "Generating Windows ICO..."
    
    if check_tool icotool "sudo apt install icoutils"; then
        icotool -c -o "$ASSETS_DIR/icon.ico" \
            "$ASSETS_DIR/icon_16x16.png" \
            "$ASSETS_DIR/icon_32x32.png" \
            "$ASSETS_DIR/icon_48x48.png" \
            "$ASSETS_DIR/icon_64x64.png" \
            "$ASSETS_DIR/icon_128x128.png" \
            "$ASSETS_DIR/icon_256x256.png"
        echo "  Created icon.ico"
    elif check_tool convert "sudo apt install imagemagick"; then
        convert "$ASSETS_DIR/icon_16x16.png" \
                "$ASSETS_DIR/icon_32x32.png" \
                "$ASSETS_DIR/icon_48x48.png" \
                "$ASSETS_DIR/icon_64x64.png" \
                "$ASSETS_DIR/icon_128x128.png" \
                "$ASSETS_DIR/icon_256x256.png" \
                "$ASSETS_DIR/icon.ico"
        echo "  Created icon.ico"
    else
        echo "Warning: Cannot create ICO file without icotool or imagemagick"
    fi
}

# Generate macOS ICNS
generate_icns() {
    echo "Generating macOS ICNS..."
    
    if check_tool png2icns "sudo apt install icnsutils"; then
        png2icns "$ASSETS_DIR/icon.icns" \
            "$ASSETS_DIR/icon_16x16.png" \
            "$ASSETS_DIR/icon_32x32.png" \
            "$ASSETS_DIR/icon_128x128.png" \
            "$ASSETS_DIR/icon_256x256.png" \
            "$ASSETS_DIR/icon_512x512.png"
        echo "  Created icon.icns"
    else
        echo "Warning: Cannot create ICNS file without png2icns (icnsutils)"
        echo "  On macOS, you can use iconutil instead"
    fi
}

# Generate Linux desktop entry icon (just copy 256px)
generate_linux() {
    echo "Generating Linux icon..."
    cp "$ASSETS_DIR/icon_256x256.png" "$ASSETS_DIR/xtreme_iptv.png"
    echo "  Created xtreme_iptv.png"
}

# Main
generate_pngs
generate_ico
generate_icns
generate_linux

echo ""
echo "Done! Generated icons:"
echo "  Windows: assets/icon.ico"
echo "  macOS:   assets/icon.icns"
echo "  Linux:   assets/xtreme_iptv.png"
echo "  PNG:     assets/icon_*.png"
