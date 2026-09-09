//! Controle do túnel completo do UrbanVPN.
//!
//! O aplicativo não publica CLI, mas o serviço local usa Qt Remote Objects.
//! `urban-ipc.exe` envia somente `pushSaRequestedLocation`: `us` conecta e a
//! string vazia desconecta. O estado é confirmado pelo adaptador do Windows,
//! sem interpretar os tipos privados do Urban.

use anyhow::{bail, Context, Result};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const AUXILIAR: &[u8] = include_bytes!("urban-ipc/urban-ipc.exe");
const NOME_AUXILIAR: &str = "urban-ipc.exe";
const PAIS: &str = "us";
const ESPERA_TUNEL: Duration = Duration::from_secs(20);

#[derive(Default)]
struct Estado {
    geracao: u64,
    iniciado_por_nos: bool,
}

pub struct UrbanVpn {
    estado: Mutex<Estado>,
}

impl UrbanVpn {
    pub fn nova() -> Self {
        let vpn = Self {
            estado: Mutex::new(Estado::default()),
        };
        // Uma finalização forçada não pode deixar a VPN esquecida. O marcador
        // só existe para conexões iniciadas por este programa.
        if caminho_marcador().exists() {
            let _ = vpn.desligar_forcado();
        }
        vpn
    }

    /// Abre (ou renova) uma janela de uso. A geração impede um temporizador
    /// antigo de desligar a VPN depois que uma ação de live mais nova começou.
    pub fn ativar(&self, motivo: &str) -> Result<u64> {
        let mut estado = self.estado.lock().unwrap_or_else(|e| e.into_inner());
        estado.geracao = estado.geracao.wrapping_add(1).max(1);
        let geracao = estado.geracao;

        if tunel_ativo() {
            crate::log::linha(&format!(
                "UrbanVPN já estava ativo; janela {geracao} renovada ({motivo})"
            ));
            return Ok(geracao);
        }

        garantir_urban_aberto()?;
        let inicio = Instant::now();
        crate::log::linha(&format!(
            "UrbanVPN: enviando conectar ({motivo}); ação do Discord ainda bloqueada"
        ));
        executar(&["connect", PAIS]).context("enviando conectar ao UrbanVPN")?;
        crate::log::linha(
            "UrbanVPN: comando aceito; aguardando o adaptador do túnel ficar operacional",
        );
        esperar_estado(true)?;
        estado.iniciado_por_nos = true;
        std::fs::write(caminho_marcador(), b"owned\n").context("registrando a VPN temporária")?;
        crate::log::linha(&format!(
            "UrbanVPN ativo e confirmado em {} ms; janela {geracao} ({motivo})",
            inicio.elapsed().as_millis()
        ));
        Ok(geracao)
    }

    pub fn desativar(&self, geracao: u64, motivo: &str) -> Result<bool> {
        let mut estado = self.estado.lock().unwrap_or_else(|e| e.into_inner());
        if estado.geracao != geracao {
            crate::log::linha(&format!(
                "janela {geracao} não desligou a VPN: existe ação mais nova ({motivo})"
            ));
            return Ok(false);
        }
        if !estado.iniciado_por_nos && !caminho_marcador().exists() {
            return Ok(false);
        }

        executar(&["disconnect"]).context("enviando desconectar ao UrbanVPN")?;
        esperar_estado(false)?;
        estado.iniciado_por_nos = false;
        let _ = std::fs::remove_file(caminho_marcador());
        crate::log::linha(&format!(
            "UrbanVPN desligado e confirmado; tráfego direto restaurado ({motivo})"
        ));
        Ok(true)
    }

    /// Encerra a geração que está ativa neste instante. A checagem interna de
    /// propriedade e o desligamento ficam sob a mesma trava, de modo que uma
    /// ação nova não pode entrar no intervalo e ser desligada por engano.
    pub fn desativar_atual(&self, motivo: &str) -> Result<bool> {
        let mut estado = self.estado.lock().unwrap_or_else(|e| e.into_inner());
        if !estado.iniciado_por_nos && !caminho_marcador().exists() {
            return Ok(false);
        }

        executar(&["disconnect"]).context("enviando desconectar ao UrbanVPN")?;
        esperar_estado(false)?;
        estado.iniciado_por_nos = false;
        let _ = std::fs::remove_file(caminho_marcador());
        crate::log::linha(&format!(
            "UrbanVPN desligado e confirmado; tráfego direto restaurado ({motivo})"
        ));
        Ok(true)
    }

    pub fn desligar_forcado(&self) -> Result<()> {
        if tunel_ativo() {
            garantir_urban_aberto()?;
            executar(&["disconnect"])?;
            esperar_estado(false)?;
        }
        let _ = std::fs::remove_file(caminho_marcador());
        let mut estado = self.estado.lock().unwrap_or_else(|e| e.into_inner());
        estado.iniciado_por_nos = false;
        Ok(())
    }
}

pub fn instalar_auxiliar() -> Result<()> {
    validar_instalacao()?;
    let destino = caminho_auxiliar();
    if std::fs::read(&destino).ok().as_deref() != Some(AUXILIAR) {
        std::fs::write(&destino, AUXILIAR).context("instalando o controlador do UrbanVPN")?;
    }
    Ok(())
}

pub fn remover_auxiliar() {
    let _ = std::fs::remove_file(caminho_auxiliar());
    let _ = std::fs::remove_file(caminho_marcador());
}

pub fn disponivel() -> bool {
    caminho_auxiliar().is_file() && validar_instalacao().is_ok()
}

/// Indica se o aplicativo de desktop está instalado, independentemente de o
/// nosso auxiliar já ter sido extraído. Usado pelo bootstrapper da release.
pub fn instalado() -> bool {
    validar_instalacao().is_ok()
}

pub fn ativo_agora() -> bool {
    tunel_ativo()
}

fn validar_instalacao() -> Result<()> {
    let pasta = pasta_urban();
    for nome in [
        "urban-vpn-app.exe",
        "Qt6Core.dll",
        "Qt6Network.dll",
        "Qt6RemoteObjects.dll",
    ] {
        if !pasta.join(nome).is_file() {
            bail!(
                "UrbanVPN incompleto: não encontrei {}",
                pasta.join(nome).display()
            );
        }
    }
    Ok(())
}

fn garantir_urban_aberto() -> Result<()> {
    if crate::processos::esta_rodando("urban-vpn-service.exe") {
        return Ok(());
    }
    let app = pasta_urban().join("urban-vpn-app.exe");
    comando_oculto(&app)
        .arg("--tray-minimized")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("abrindo {}", app.display()))?;

    // `status` não usa o IPC e pode responder antes do serviço ficar pronto;
    // a primeira ação real ainda tem a própria confirmação/erro.
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn esperar_estado(esperado: bool) -> Result<()> {
    let inicio = Instant::now();
    while inicio.elapsed() < ESPERA_TUNEL {
        if tunel_ativo() == esperado {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    bail!(
        "o adaptador UrbanVPN não ficou {} em {} segundos",
        if esperado { "ativo" } else { "desativado" },
        ESPERA_TUNEL.as_secs()
    )
}

fn tunel_ativo() -> bool {
    executar_bruto(&["status"])
        .map(|saida| saida.status.success())
        .unwrap_or(false)
}

fn executar(argumentos: &[&str]) -> Result<Output> {
    let saida = executar_bruto(argumentos)?;
    if !saida.status.success() {
        let erro = String::from_utf8_lossy(&saida.stderr).trim().to_string();
        bail!(
            "urban-ipc terminou com {}{}",
            saida.status,
            if erro.is_empty() {
                String::new()
            } else {
                format!(": {erro}")
            }
        );
    }
    Ok(saida)
}

fn executar_bruto(argumentos: &[&str]) -> Result<Output> {
    let pasta = pasta_urban();
    let atual = std::env::var_os("PATH").unwrap_or_default();
    let mut caminhos = vec![pasta.clone()];
    caminhos.extend(std::env::split_paths(&atual));
    let path: OsString = std::env::join_paths(caminhos).context("montando PATH do UrbanVPN")?;

    comando_oculto(caminho_auxiliar())
        .args(argumentos)
        .env("PATH", path)
        .output()
        .context("executando o controlador do UrbanVPN")
}

fn caminho_auxiliar() -> PathBuf {
    crate::pasta_dados().join(NOME_AUXILIAR)
}

fn caminho_marcador() -> PathBuf {
    crate::pasta_dados().join("urban-vpn-owned")
}

fn pasta_urban() -> PathBuf {
    let base = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    PathBuf::from(base).join("UrbanVPN").join("bin")
}

fn comando_oculto(programa: impl AsRef<Path>) -> Command {
    let mut comando = Command::new(programa.as_ref());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        comando.creation_flags(CREATE_NO_WINDOW);
    }
    comando
}
