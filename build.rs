fn main() {
    // Embed Windows manifest and icon
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_manifest_file("app.manifest");
        
        // Embed icon if it exists
        if std::path::Path::new("assets/icon.ico").exists() {
            res.set_icon("assets/icon.ico");
        }
        
        if let Err(e) = res.compile() {
            eprintln!("Warning: Failed to embed manifest/icon: {}", e);
        }
    }
}
