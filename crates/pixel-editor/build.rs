//! Gibt der Windows-exe das Pixel-Herz als Symbol, dasselbe wie im Fenster.
//! Explorer, Taskleiste und Verknüpfungen zeigen es dann.

fn main() {
    println!("cargo:rerun-if-changed=assets/pixelart-studio.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/pixelart-studio.ico")
        .set("ProductName", "PixelArt Studio")
        .set("FileDescription", "PixelArt Studio");
    if let Err(err) = res.compile() {
        // Unter Windows muss das klappen, damit kein Release ohne Symbol entsteht.
        // Beim Prüfen von einem anderen System aus fehlt oft nur das Werkzeug dafür.
        if cfg!(windows) {
            panic!("Symbol für die exe ließ sich nicht einbinden: {err}");
        }
        println!("cargo:warning=exe icon not embedded: {err}");
    }
}
