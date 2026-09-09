//! Bootstrapper da release.
//!
//! Baixa dependências somente de suas origens oficiais. O Dorion é validado
//! pelo SHA-256 fornecido pela API de Releases do GitHub; o UrbanVPN, cuja URL
//! oficial não publica hash, precisa ter assinatura Authenticode válida da
//! Urban Cyber Security Inc.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const API_DORION: &str = "https://api.github.com/repos/SpikeHD/Dorion/releases/latest";
const PREFIXO_DORION: &str = "https://github.com/SpikeHD/Dorion/releases/download/";
const URL_URBAN: &str = "https://install.urban-vpn.com/UrbanVPN.exe";

pub fn confirmar_inicio(dorion: bool, urban: bool) -> bool {
    let ausentes = match (dorion, urban) {
        (true, true) => "Dorion e UrbanVPN já estão instalados.",
        (false, true) => "Dorion não foi encontrado e será baixado do GitHub oficial.",
        (true, false) => "UrbanVPN não foi encontrado e será baixado do site oficial.",
        (false, false) => {
            "Dorion e UrbanVPN não foram encontrados e serão baixados dos sites oficiais."
        }
    };
    perguntar(&format!(
        "Dorion GoLive {}\n\n{}\n\nOs instaladores de terceiros serão validados antes de executar. Deseja continuar?",
        env!("CARGO_PKG_VERSION"),
        ausentes
    ))
}

pub fn confirmar_desinstalacao() -> bool {
    perguntar("Deseja remover o Dorion GoLive?\n\nO Dorion e o UrbanVPN permanecerão instalados.")
}

pub fn garantir_dependencias() -> Result<()> {
    let temporarios = Temporarios::criar()?;

    if crate::discord::lancador().is_none() {
        informar("Baixando e instalando o Dorion pelo canal oficial do GitHub.");
        instalar_dorion(&temporarios.pasta)?;
        aguardar_arquivo("Dorion", Duration::from_secs(90), || {
            crate::discord::lancador().is_some()
        })?;
    }

    if !crate::urban::instalado() {
        informar("Baixando e instalando o aplicativo UrbanVPN pelo site oficial.");
        instalar_urban(&temporarios.pasta)?;
        aguardar_arquivo(
            "UrbanVPN",
            Duration::from_secs(120),
            crate::urban::instalado,
        )?;
    }

    Ok(())
}

fn instalar_dorion(pasta: &Path) -> Result<()> {
    let manifesto = pasta.join("dorion-release.json");
    baixar(API_DORION, &manifesto)?;
    let release: Value = serde_json::from_slice(
        &std::fs::read(&manifesto).context("lendo a resposta de Releases do Dorion")?,
    )
    .context("interpretando a resposta de Releases do Dorion")?;
    let (url, hash) = escolher_dorion(&release)?;
    let instalador = pasta.join("Dorion-Setup.exe");
    baixar(&url, &instalador)?;

    let obtido = sha256(&instalador)?;
    if !obtido.eq_ignore_ascii_case(&hash) {
        bail!("o SHA-256 do instalador Dorion não corresponde ao publicado pelo GitHub");
    }

    executar_instalador(&instalador, &["/S"]).context("executando o instalador oficial do Dorion")
}

fn instalar_urban(pasta: &Path) -> Result<()> {
    let instalador = pasta.join("UrbanVPN-Setup.exe");
    baixar(URL_URBAN, &instalador)?;
    validar_assinatura_urban(&instalador)?;
    executar_instalador(&instalador, &["/exenoui", "/qn", "/norestart"])
        .context("executando o instalador oficial do UrbanVPN")
}

fn escolher_dorion(release: &Value) -> Result<(String, String)> {
    let assets = release["assets"]
        .as_array()
        .context("a release do Dorion não contém a lista de arquivos")?;
    let asset = assets
        .iter()
        .find(|asset| {
            asset["name"]
                .as_str()
                .is_some_and(|nome| nome.ends_with("_x64-setup.exe"))
        })
        .context("a release atual do Dorion não contém instalador x64")?;
    let url = asset["browser_download_url"]
        .as_str()
        .context("a release do Dorion não informou a URL do instalador")?;
    if !url.starts_with(PREFIXO_DORION) {
        bail!("a API do Dorion retornou uma origem de download inesperada");
    }
    let hash = asset["digest"]
        .as_str()
        .and_then(|valor| valor.strip_prefix("sha256:"))
        .filter(|valor| valor.len() == 64 && valor.bytes().all(|b| b.is_ascii_hexdigit()))
        .context("a release do Dorion não publicou um SHA-256 válido")?;
    Ok((url.to_owned(), hash.to_owned()))
}

fn baixar(url: &str, destino: &Path) -> Result<()> {
    let curl = ferramenta_sistema("curl.exe")?;
    let status = comando_oculto(curl)
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "20",
            "--retry",
            "2",
            "--user-agent",
            concat!("DorionGoLive/", env!("CARGO_PKG_VERSION")),
            "--output",
        ])
        .arg(destino)
        .arg(url)
        .status()
        .with_context(|| format!("iniciando o download de {url}"))?;
    if !status.success() || !destino.is_file() {
        bail!("download oficial falhou: {url} ({status})");
    }
    Ok(())
}

fn sha256(caminho: &Path) -> Result<String> {
    let mut arquivo = File::open(caminho)?;
    let mut hash = Sha256::new();
    let mut bloco = [0_u8; 64 * 1024];
    loop {
        let lidos = arquivo.read(&mut bloco)?;
        if lidos == 0 {
            break;
        }
        hash.update(&bloco[..lidos]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn validar_assinatura_urban(caminho: &Path) -> Result<()> {
    const VERIFICAR: &str = r#"
$assinatura = Get-AuthenticodeSignature -LiteralPath $env:DORION_GOLIVE_SIGNATURE_FILE
if ($assinatura.Status -ne 'Valid') { exit 10 }
if ($assinatura.SignerCertificate.Subject -notmatch 'Urban Cyber Security Inc\.') { exit 11 }
exit 0
"#;
    let powershell = ferramenta_sistema(r"WindowsPowerShell\v1.0\powershell.exe")?;
    let status = comando_oculto(powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", VERIFICAR])
        .env("DORION_GOLIVE_SIGNATURE_FILE", caminho)
        .status()
        .context("validando a assinatura digital do instalador UrbanVPN")?;
    if !status.success() {
        bail!(
            "o instalador UrbanVPN não possui assinatura válida da Urban Cyber Security Inc. ({status})"
        );
    }
    Ok(())
}

fn ferramenta_sistema(nome: &str) -> Result<PathBuf> {
    let raiz = std::env::var_os("SystemRoot").context("SystemRoot não está definido")?;
    let caminho = PathBuf::from(raiz).join("System32").join(nome);
    if !caminho.is_file() {
        bail!(
            "ferramenta do Windows não encontrada: {}",
            caminho.display()
        );
    }
    Ok(caminho)
}

fn executar_instalador(caminho: &Path, argumentos: &[&str]) -> Result<()> {
    let status = Command::new(caminho)
        .args(argumentos)
        .status()
        .with_context(|| format!("abrindo {}", caminho.display()))?;
    if codigo_aceito(status) {
        Ok(())
    } else {
        bail!("o instalador terminou com {status}")
    }
}

fn codigo_aceito(status: ExitStatus) -> bool {
    status.success() || matches!(status.code(), Some(1641 | 3010))
}

fn aguardar_arquivo(nome: &str, limite: Duration, pronto: impl Fn() -> bool) -> Result<()> {
    let inicio = std::time::Instant::now();
    while inicio.elapsed() < limite {
        if pronto() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    bail!(
        "{nome} não apareceu instalado após {} segundos",
        limite.as_secs()
    )
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

struct Temporarios {
    pasta: PathBuf,
}

impl Temporarios {
    fn criar() -> Result<Self> {
        let agora = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for tentativa in 0..100_u32 {
            let pasta = std::env::temp_dir().join(format!(
                "DorionGoLive-Setup-{}-{agora}-{tentativa}",
                std::process::id()
            ));
            match std::fs::create_dir(&pasta) {
                Ok(()) => return Ok(Self { pasta }),
                Err(erro) if erro.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(erro) => return Err(erro).context("criando a pasta temporária do instalador"),
            }
        }
        bail!("não foi possível criar uma pasta temporária exclusiva")
    }
}

impl Drop for Temporarios {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.pasta);
    }
}

#[cfg(windows)]
fn mensagem(texto: &str, estilo: u32) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;
    let titulo: Vec<u16> = "Dorion GoLive\0".encode_utf16().collect();
    let texto: Vec<u16> = texto.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            texto.as_ptr(),
            titulo.as_ptr(),
            estilo,
        )
    }
}

#[cfg(not(windows))]
fn mensagem(texto: &str, _estilo: u32) -> i32 {
    eprintln!("{texto}");
    6
}

fn perguntar(texto: &str) -> bool {
    const MB_YESNO: u32 = 0x0000_0004;
    const MB_ICONINFORMATION: u32 = 0x0000_0040;
    const MB_DEFBUTTON2: u32 = 0x0000_0100;
    const IDYES: i32 = 6;
    mensagem(texto, MB_YESNO | MB_ICONINFORMATION | MB_DEFBUTTON2) == IDYES
}

fn informar(texto: &str) {
    const MB_OK: u32 = 0;
    const MB_ICONINFORMATION: u32 = 0x0000_0040;
    mensagem(texto, MB_OK | MB_ICONINFORMATION);
}

pub fn informar_sucesso() {
    informar("Instalação concluída. O Dorion GoLive iniciará automaticamente com o Windows.");
}

pub fn informar_desinstalado() {
    informar("Dorion GoLive removido. O Dorion e o UrbanVPN foram preservados.");
}

pub fn informar_erro(erro: &str) {
    const MB_OK: u32 = 0;
    const MB_ICONERROR: u32 = 0x0000_0010;
    mensagem(
        &format!("A instalação não foi concluída.\n\n{erro}"),
        MB_OK | MB_ICONERROR,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escolhe_somente_instalador_x64_com_hash_publicado() {
        let release = serde_json::json!({ "assets": [{
            "name": "Dorion_6.13.0_x64-setup.exe",
            "browser_download_url": "https://github.com/SpikeHD/Dorion/releases/download/v6.13.0/Dorion_6.13.0_x64-setup.exe",
            "digest": format!("sha256:{}", "a".repeat(64))
        }] });
        let (url, hash) = escolher_dorion(&release).unwrap();
        assert!(url.ends_with("_x64-setup.exe"));
        assert_eq!(hash, "a".repeat(64));
    }

    #[test]
    fn recusa_origem_alheia_mesmo_vinda_do_json() {
        let release = serde_json::json!({ "assets": [{
            "name": "Dorion_6.13.0_x64-setup.exe",
            "browser_download_url": "https://example.invalid/Dorion_6.13.0_x64-setup.exe",
            "digest": format!("sha256:{}", "a".repeat(64))
        }] });
        assert!(escolher_dorion(&release).is_err());
    }
}
