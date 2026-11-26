//! Windows: manifesto pedindo administrador (controlar o serviço e editar o .ini do servidor
//! DHCP exigem) e com estilos visuais modernos.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        use embed_manifest::manifest::ExecutionLevel;
        use embed_manifest::{embed_manifest, new_manifest};
        embed_manifest(new_manifest("OpenDHCP").requested_execution_level(ExecutionLevel::RequireAdministrator))
            .expect("não consegui embutir o manifesto");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
