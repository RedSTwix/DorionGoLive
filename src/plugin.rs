//! Instalação do pequeno detector JavaScript suportado pelo próprio Dorion.
//! O arquivo conversa apenas com o serviço local em `127.0.0.1`, recebe a
//! confirmação da VPN e nunca envia token do Discord ou conteúdo da live.

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const ARQUIVO: &str = "DorionGoLive.js";
const CODIGO: &str = include_str!("dorion-plugin/DorionGoLive.js");

fn pasta() -> PathBuf {
    let base = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("dorion").join("plugins")
}

fn caminho_plugin() -> PathBuf {
    pasta().join(ARQUIVO)
}

fn caminho_lista() -> PathBuf {
    pasta().join("plugins.json")
}

fn ler_lista() -> Map<String, Value> {
    std::fs::read_to_string(caminho_lista())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn salvar_lista(lista: &Map<String, Value>) -> Result<()> {
    let texto = serde_json::to_string_pretty(lista)?;
    std::fs::write(caminho_lista(), texto).context("atualizando plugins.json do Dorion")
}

pub fn instalar() -> Result<()> {
    std::fs::create_dir_all(pasta()).context("criando a pasta de plugins do Dorion")?;
    std::fs::write(caminho_plugin(), CODIGO).context("instalando o detector de live no Dorion")?;

    let mut lista = ler_lista();
    lista.insert(
        ARQUIVO.into(),
        json!({ "name": "DorionGoLive", "enabled": true, "preload": false }),
    );
    salvar_lista(&lista)
}

pub fn desinstalar() -> Result<()> {
    let _ = std::fs::remove_file(caminho_plugin());
    let mut lista = ler_lista();
    lista.remove(ARQUIVO);
    if caminho_lista().exists() {
        salvar_lista(&lista)?;
    }
    Ok(())
}

pub fn ativo() -> bool {
    if !caminho_plugin().exists() {
        return false;
    }
    ler_lista()
        .get(ARQUIVO)
        .and_then(Value::as_object)
        .and_then(|v| v.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codigo_embutido_aguarda_o_controle_local() {
        assert!(CODIGO.contains("http://127.0.0.1:9352/golive"));
        assert!(CODIGO.contains("window.fetch !== window.nativeFetch"));
        assert!(CODIGO.contains("return window.fetch.bind(window)"));
        assert!(CODIGO.contains("x-dorion-golive-ready"));
        assert!(CODIGO.contains("x-dorion-golive-session"));
        assert!(!CODIGO.contains("const ESPERA_VPN_MS"));
        assert!(!CODIGO.contains("https://"));
        assert!(CODIGO.contains("getDisplayMedia"));
        assert!(CODIGO.contains("getUserMedia_desktop_interceptado"));
        assert!(CODIGO.contains("controle_nao_reconhecido"));
    }
}
