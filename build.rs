fn main() {
    #[cfg(windows)]
    {
        if std::path::Path::new("assets/logo.ico").exists() {
            let mut res = winres::WindowsResource::new();
            res.set_icon("assets/logo.ico");
            let _ = res.compile();
        }
    }
}
