fn main() {
    // Embed Windows manifest for DPI awareness
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_manifest_file("app.manifest");
        if let Err(e) = res.compile() {
            eprintln!("Warning: Failed to embed manifest: {}", e);
        }
    }
}
