// Embarque l'icône et les métadonnées produit dans l'exécutable Windows.
fn main() {
    println!("cargo:rerun-if-changed=icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !std::path::Path::new("icon.ico").exists() {
        println!("cargo:warning=icon.ico absent : régénérez avec `rustc tools/makeicon.rs`");
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("icon.ico");
    res.set("ProductName", "DFWLogger");
    res.set("FileDescription", "DFWLogger — superviseur familial transparent");
    res.set("LegalCopyright", "MIT / DFWLogger contributors");
    res.set("OriginalFilename", "dfwlogger.exe");
    if let Err(e) = res.compile() {
        println!("cargo:warning=icône non embarquée : {e}");
    }
}
