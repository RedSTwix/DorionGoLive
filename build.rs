//! Carimba identidade no executável do serviço.
//!
//! Um `.exe` sem nome, sem descrição, sem fabricante e sem ícone é o perfil que
//! os modelos de reputação de antivírus podem pontuar como suspeito antes
//! mesmo de olhar o que o programa faz.
//!
//! O recurso é compilado pelo `rc.exe` do Windows SDK, o mesmo que o empacotador
//! do Tauri já usa. Fora do Windows não há o que carimbar.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    carimbar_identidade();
}

#[cfg(windows)]
fn carimbar_identidade() {
    const ICONE: &str = "assets/icon.ico";

    println!("cargo:rerun-if-changed={ICONE}");

    let mut recurso = tauri_winres::WindowsResource::new();
    recurso
        .set("ProductName", "Dorion GoLive")
        .set(
            "FileDescription",
            "Instalador e serviço de VPN temporária para o Dorion",
        )
        .set("CompanyName", "Dorion GoLive local build")
        .set("LegalCopyright", "MIT contributors")
        .set("OriginalFilename", "DorionGoLive-Setup.exe")
        .set("InternalName", "dorion-golive");

    // Numa árvore incompleta o build continua: faltar ícone é menos grave do
    // que não compilar.
    if std::path::Path::new(ICONE).is_file() {
        recurso.set_icon(ICONE);
    } else {
        println!("cargo:warning=ícone {ICONE} ausente; o serviço sai sem ícone");
    }

    recurso
        .compile()
        .expect("não consegui carimbar o recurso de versão no serviço");
}
