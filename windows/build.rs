// Met l'icône (resources/icon.ico) et le nom de l'app dans l'exe Windows.
fn main() {
    println!("cargo:rerun-if-changed=resources/icon.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("resources/icon.ico");
    res.set("ProductName", &std::env::var("CARGO_PKG_NAME").unwrap_or_default());
    res.set("FileDescription", &std::env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default());
    if let Err(e) = res.compile() { println!("cargo:warning=icône non intégrée : {e}"); }
}
