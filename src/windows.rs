//! Marcas do aplicativo no perfil do usuário: migração do proxy antigo,
//! autostart, PATH e entrada de desinstalação. Todas são removíveis.

use anyhow::{bail, Context, Result};
use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Stdio},
};
use winreg::{enums::*, RegKey, RegValue};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CHAVE_INTERNET: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
const CHAVE_RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const CHAVE_DESINSTALAR: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\DorionGoLive";
const NOME_RUN: &str = "DorionGoLive";
const BACKUP: &str = "AutoConfigURL_backup_DorionGoLive";
const TAREFA_BANDEJA: &str = "DorionGoLive.Bandeja";

fn hkcu() -> RegKey {
    RegKey::predef(HKEY_CURRENT_USER)
}

fn comando_oculto(programa: impl AsRef<OsStr>) -> Command {
    let mut comando = Command::new(programa);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        comando.creation_flags(CREATE_NO_WINDOW);
    }
    comando
}

/// Aponta o proxy automático do Windows para o nosso PAC, guardando o valor
/// anterior para conseguir devolver na desinstalação.
#[allow(dead_code)]
pub fn ativar_pac(url: &str) -> Result<()> {
    let (k, _) = hkcu()
        .create_subkey(CHAVE_INTERNET)
        .context("abrindo Internet Settings")?;

    let atual: Result<String, _> = k.get_value("AutoConfigURL");
    if let Ok(anterior) = atual {
        if anterior != url && k.get_value::<String, _>(BACKUP).is_err() {
            k.set_value(BACKUP, &anterior)?;
        }
    }
    k.set_value("AutoConfigURL", &url.to_string())?;
    Ok(())
}

pub fn desativar_pac(url_nossa: &str) -> Result<()> {
    let (k, _) = hkcu().create_subkey(CHAVE_INTERNET)?;
    let atual = k.get_value::<String, _>("AutoConfigURL").ok();
    // Se outro programa ou o próprio usuário trocou o PAC depois da nossa
    // instalação, a desinstalação não pode sobrescrever essa escolha.
    if atual.as_deref() != Some(url_nossa) {
        return Ok(());
    }
    match k.get_value::<String, _>(BACKUP) {
        Ok(anterior) => {
            k.set_value("AutoConfigURL", &anterior)?;
            let _ = k.delete_value(BACKUP);
        }
        Err(_) => {
            let _ = k.delete_value("AutoConfigURL");
        }
    }
    Ok(())
}

pub fn pac_ativo(url: &str) -> bool {
    hkcu()
        .open_subkey(CHAVE_INTERNET)
        .and_then(|k| k.get_value::<String, _>("AutoConfigURL"))
        .map(|v| v == url)
        .unwrap_or(false)
}

pub fn ativar_autostart(comando: &str) -> Result<()> {
    let (k, _) = hkcu().create_subkey(CHAVE_RUN)?;
    k.set_value(NOME_RUN, &comando.to_string())?;
    Ok(())
}

fn entrada_run_e_nossa(valor: &str, servico: &Path) -> bool {
    valor
        .trim()
        .eq_ignore_ascii_case(&format!("\"{}\" rodar", servico.display()))
}

pub fn validar_autostart(servico: &Path) -> Result<()> {
    let Ok(chave) = hkcu().open_subkey(CHAVE_RUN) else {
        return Ok(());
    };
    let Ok(valor) = chave.get_value::<String, _>(NOME_RUN) else {
        return Ok(());
    };
    if !entrada_run_e_nossa(&valor, servico) {
        bail!("a entrada Run\\DorionGoLive não pertence ao serviço instalado pelo Dorion GoLive")
    }
    Ok(())
}

#[cfg(test)]
fn comandos_de_remocao_autostart() -> Vec<String> {
    vec![
        format!("schtasks /delete /tn {TAREFA_BANDEJA} /f"),
        format!(r"HKCU\{CHAVE_RUN}\{NOME_RUN}"),
    ]
}

fn remover_tarefa_bandeja() -> Result<()> {
    let saida = comando_oculto("schtasks")
        .args(["/delete", "/tn", TAREFA_BANDEJA, "/f"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("executando a remoção da tarefa de bandeja")?;
    if saida.success() {
        return Ok(());
    }

    let existe = comando_oculto("schtasks")
        .args(["/query", "/tn", TAREFA_BANDEJA])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(true);
    if existe {
        bail!("não consegui remover a tarefa {TAREFA_BANDEJA}")
    }
    Ok(())
}

pub fn desativar_autostart(servico: &Path) -> Result<()> {
    validar_autostart(servico)?;
    remover_tarefa_bandeja()?;
    if let Ok(k) = hkcu().open_subkey_with_flags(CHAVE_RUN, KEY_ALL_ACCESS) {
        if let Ok(valor) = k.get_value::<String, _>(NOME_RUN) {
            if entrada_run_e_nossa(&valor, servico) {
                k.delete_value(NOME_RUN)?;
            }
        }
    }
    Ok(())
}

pub fn autostart_ativo() -> bool {
    hkcu()
        .open_subkey(CHAVE_RUN)
        .and_then(|k| k.get_value::<String, _>(NOME_RUN))
        .is_ok()
}

pub fn registrar_desinstalacao(servico: &Path, pasta: &Path) -> Result<()> {
    let (chave, _) = hkcu()
        .create_subkey(CHAVE_DESINSTALAR)
        .context("criando a entrada de desinstalação")?;
    chave.set_value("DisplayName", &"Dorion GoLive")?;
    chave.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))?;
    chave.set_value("Publisher", &"RedSTwix")?;
    chave.set_value("InstallLocation", &pasta.display().to_string())?;
    chave.set_value("DisplayIcon", &servico.display().to_string())?;
    chave.set_value(
        "UninstallString",
        &format!("\"{}\" desinstalar-ui", servico.display()),
    )?;
    chave.set_value("NoModify", &1_u32)?;
    chave.set_value("NoRepair", &1_u32)?;
    Ok(())
}

pub fn remover_registro_desinstalacao() -> Result<()> {
    match hkcu().delete_subkey_all(CHAVE_DESINSTALAR) {
        Ok(()) => Ok(()),
        Err(erro) if erro.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(erro) => Err(erro).context("removendo a entrada de desinstalação"),
    }
}

// --- PATH do usuário -------------------------------------------------------
//
// O PATH costuma ser `REG_EXPAND_SZ` e conter coisas como `%USERPROFILE%\bin`.
// Reescrevê-lo como `REG_SZ` congelaria essas variáveis e quebraria o PATH de
// quem instalou. Por isso lemos e gravamos o valor bruto, preservando o tipo.

const CHAVE_ENV: &str = "Environment";

fn utf16_para_texto(v: &RegValue) -> String {
    let u: Vec<u16> = v
        .bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16_lossy(&u)
}

fn texto_para_utf16(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|c| c.to_le_bytes())
        .collect()
}

pub fn adicionar_ao_path(dir: &str) -> Result<()> {
    let (k, _) = hkcu()
        .create_subkey(CHAVE_ENV)
        .context("abrindo Environment")?;

    let (atual, vtype) = match k.get_raw_value("Path") {
        Ok(v) => (utf16_para_texto(&v), v.vtype),
        Err(_) => (String::new(), REG_EXPAND_SZ),
    };

    if esta_no_path(&atual, dir) {
        return Ok(());
    }

    let mut novo = atual;
    if !novo.is_empty() && !novo.ends_with(';') {
        novo.push(';');
    }
    novo.push_str(dir);

    k.set_raw_value(
        "Path",
        &RegValue {
            bytes: texto_para_utf16(&novo),
            vtype,
        },
    )?;
    Ok(())
}

pub fn remover_do_path(dir: &str) -> Result<()> {
    let Ok(k) = hkcu().open_subkey_with_flags(CHAVE_ENV, KEY_ALL_ACCESS) else {
        return Ok(());
    };
    let Ok(v) = k.get_raw_value("Path") else {
        return Ok(());
    };

    let atual = utf16_para_texto(&v);
    let novo: Vec<&str> = atual
        .split(';')
        .filter(|p| !p.trim().eq_ignore_ascii_case(dir.trim_end_matches('\\')))
        .filter(|p| !p.trim().eq_ignore_ascii_case(dir))
        .collect();
    let novo = novo.join(";");

    if novo != atual {
        k.set_raw_value(
            "Path",
            &RegValue {
                bytes: texto_para_utf16(&novo),
                vtype: v.vtype,
            },
        )?;
    }
    Ok(())
}

pub fn path_ativo(dir: &str) -> bool {
    hkcu()
        .open_subkey(CHAVE_ENV)
        .and_then(|k| k.get_raw_value("Path"))
        .map(|v| esta_no_path(&utf16_para_texto(&v), dir))
        .unwrap_or(false)
}

fn esta_no_path(path: &str, dir: &str) -> bool {
    let alvo = dir.trim_end_matches('\\');
    path.split(';')
        .any(|p| p.trim().trim_end_matches('\\').eq_ignore_ascii_case(alvo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconhece_entrada_ja_presente() {
        let p = r"C:\Windows;C:\Users\x\AppData\Local\DorionGoLive;C:\outro";
        assert!(esta_no_path(p, r"C:\Users\x\AppData\Local\DorionGoLive"));
        assert!(esta_no_path(p, r"C:\Users\x\AppData\Local\DorionGoLive\"));
        assert!(!esta_no_path(p, r"C:\Users\x\AppData\Local\Outro"));
    }

    #[test]
    fn ida_e_volta_preserva_o_texto() {
        let s = r"C:\Windows;%USERPROFILE%\bin";
        let v = RegValue {
            bytes: texto_para_utf16(s),
            vtype: REG_EXPAND_SZ,
        };
        assert_eq!(utf16_para_texto(&v), s);
    }

    #[test]
    fn remover_autostart_remove_task_e_run_do_dorion_golive() {
        let comandos = comandos_de_remocao_autostart();
        assert!(comandos
            .iter()
            .any(|comando| comando.contains("DorionGoLive.Bandeja")));
    }
}
