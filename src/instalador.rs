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
use std::os::windows::{ffi::OsStrExt, process::CommandExt};

const API_DORION: &str = "https://api.github.com/repos/SpikeHD/Dorion/releases/latest";
const PREFIXO_DORION: &str = "https://github.com/SpikeHD/Dorion/releases/download/";
const URL_URBAN: &str = "https://install.urban-vpn.com/UrbanVPN.exe";

#[derive(Clone, Debug)]
pub struct Diagnostico {
    pub windows: String,
    pub windows_ok: bool,
    pub internet_ok: bool,
    pub administrador: bool,
    pub espaco_livre_gib: u64,
    pub espaco_ok: bool,
    pub dorion: Option<PathBuf>,
    pub urban: bool,
    pub plugin: bool,
    pub servico: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Componente {
    Dorion,
    UrbanVpn,
    Plugin,
    Servico,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstadoComponente {
    Aguardando,
    Baixando,
    Validando,
    Instalando,
    Configurando,
    Concluido,
}

#[derive(Clone, Debug)]
pub struct Atualizacao {
    pub componente: Componente,
    pub estado: EstadoComponente,
    pub detalhe: String,
    pub progresso: f32,
}

pub fn verificar_sistema() -> Diagnostico {
    let (windows, windows_ok) = versao_windows();
    let internet_ok = internet_disponivel();
    let espaco_livre = fs2::available_space(std::env::temp_dir()).unwrap_or_default();
    let espaco_livre_gib = espaco_livre / 1024 / 1024 / 1024;
    let dorion = crate::discord::lancador();
    Diagnostico {
        windows,
        windows_ok,
        internet_ok,
        administrador: usuario_elevado(),
        espaco_livre_gib,
        espaco_ok: espaco_livre >= 350 * 1024 * 1024,
        dorion,
        urban: crate::urban::instalado(),
        plugin: crate::plugin::ativo(),
        servico: crate::caminho_instalado().is_file(),
    }
}

fn versao_windows() -> (String, bool) {
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
    let chave = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let Ok(chave) = chave else {
        return ("Windows não identificado".into(), false);
    };
    let mut produto = chave
        .get_value::<String, _>("ProductName")
        .unwrap_or_else(|_| "Windows".into());
    let build = chave
        .get_value::<String, _>("CurrentBuildNumber")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or_default();
    if build >= 22_000 {
        produto = produto.replace("Windows 10", "Windows 11");
    }
    (format!("{produto} (build {build})"), build >= 10_240)
}

fn internet_disponivel() -> bool {
    let Ok(curl) = ferramenta_sistema("curl.exe") else {
        return false;
    };
    comando_oculto(curl)
        .args([
            "--fail",
            "--head",
            "--location",
            "--silent",
            "--output",
            "NUL",
            "--proto",
            "=https",
            "--connect-timeout",
            "8",
            API_DORION,
        ])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn usuario_elevado() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

#[cfg(not(windows))]
fn usuario_elevado() -> bool {
    false
}

pub fn confirmar_desinstalacao() -> bool {
    perguntar("Deseja remover o Dorion GoLive?\n\nO Dorion e o UrbanVPN permanecerão instalados.")
}

pub fn garantir_dependencias_com(mut relatar: impl FnMut(Atualizacao)) -> Result<()> {
    let temporarios = Temporarios::criar()?;

    if crate::discord::lancador().is_none() {
        relatar(atualizacao(
            Componente::Dorion,
            EstadoComponente::Baixando,
            "Consultando a versão oficial mais recente…",
            0.08,
        ));
        let erro_execucao = instalar_dorion(&temporarios.pasta, &mut relatar)?;
        relatar(atualizacao(
            Componente::Dorion,
            EstadoComponente::Instalando,
            "Confirmando onde o Dorion foi instalado…",
            0.38,
        ));
        let limite = if erro_execucao.is_some() { 20 } else { 120 };
        let detectado = aguardar_arquivo("Dorion", Duration::from_secs(limite), || {
            crate::discord::lancador().is_some()
        });
        if let Err(erro_deteccao) = detectado {
            if let Some(erro_execucao) = erro_execucao {
                bail!("{erro_execucao}; {erro_deteccao}");
            }
            return Err(erro_deteccao);
        }
        relatar(atualizacao(
            Componente::Dorion,
            EstadoComponente::Concluido,
            &format!(
                "Encontrado em {}",
                crate::discord::lancador()
                    .as_deref()
                    .map(Path::display)
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "local não identificado".into())
            ),
            0.42,
        ));
    } else {
        relatar(atualizacao(
            Componente::Dorion,
            EstadoComponente::Concluido,
            "Já estava instalado; download ignorado.",
            0.20,
        ));
    }

    if !crate::urban::instalado() {
        let erro_execucao = instalar_urban(&temporarios.pasta, &mut relatar)?;
        relatar(atualizacao(
            Componente::UrbanVpn,
            EstadoComponente::Instalando,
            "Aguardando os arquivos e serviços do UrbanVPN…",
            0.70,
        ));
        let limite = if erro_execucao.is_some() { 20 } else { 180 };
        let detectado = aguardar_arquivo(
            "UrbanVPN",
            Duration::from_secs(limite),
            crate::urban::instalado,
        );
        if let Err(erro_deteccao) = detectado {
            if let Some(erro_execucao) = erro_execucao {
                bail!("{erro_execucao}; {erro_deteccao}");
            }
            return Err(erro_deteccao);
        }
        relatar(atualizacao(
            Componente::UrbanVpn,
            EstadoComponente::Concluido,
            "Aplicativo e bibliotecas necessários confirmados.",
            0.74,
        ));
    } else {
        relatar(atualizacao(
            Componente::UrbanVpn,
            EstadoComponente::Concluido,
            "Já estava instalado; download ignorado.",
            0.55,
        ));
    }

    Ok(())
}

fn atualizacao(
    componente: Componente,
    estado: EstadoComponente,
    detalhe: &str,
    progresso: f32,
) -> Atualizacao {
    Atualizacao {
        componente,
        estado,
        detalhe: detalhe.into(),
        progresso,
    }
}

fn instalar_dorion(pasta: &Path, relatar: &mut impl FnMut(Atualizacao)) -> Result<Option<String>> {
    let manifesto = pasta.join("dorion-release.json");
    baixar(API_DORION, &manifesto, None, |_, _| {})?;
    let release: Value = serde_json::from_slice(
        &std::fs::read(&manifesto).context("lendo a resposta de Releases do Dorion")?,
    )
    .context("interpretando a resposta de Releases do Dorion")?;
    let (url, hash, tamanho) = escolher_dorion(&release)?;
    let instalador = pasta.join("Dorion-Setup.exe");
    baixar(&url, &instalador, Some(tamanho), |bytes, total| {
        let fracao = total
            .filter(|total| *total > 0)
            .map(|total| bytes as f32 / total as f32)
            .unwrap_or_default()
            .clamp(0.0, 1.0);
        relatar(atualizacao(
            Componente::Dorion,
            EstadoComponente::Baixando,
            &format!("Baixando Dorion… {}", formatar_bytes(bytes)),
            0.10 + fracao * 0.16,
        ));
    })?;

    relatar(atualizacao(
        Componente::Dorion,
        EstadoComponente::Validando,
        "Conferindo o SHA-256 publicado pelo GitHub…",
        0.28,
    ));
    let obtido = sha256(&instalador)?;
    if !obtido.eq_ignore_ascii_case(&hash) {
        bail!("o SHA-256 do instalador Dorion não corresponde ao publicado pelo GitHub");
    }

    relatar(atualizacao(
        Componente::Dorion,
        EstadoComponente::Instalando,
        "Instalando o Dorion; confirme uma solicitação do Windows se aparecer…",
        0.32,
    ));
    let erro_execucao = executar_instalador(&instalador, &["/S"])
        .context("executando o instalador oficial do Dorion")
        .err()
        .map(|erro| format!("{erro:#}"));
    if let Some(erro) = &erro_execucao {
        // Alguns bootstrappers encerram com um código não convencional depois
        // de delegar a instalação. A redetecção abaixo é a autoridade final.
        crate::log::linha(&format!(
            "instalador do Dorion retornou erro; aguardando redetecção: {erro:#}"
        ));
    }
    Ok(erro_execucao)
}

fn instalar_urban(pasta: &Path, relatar: &mut impl FnMut(Atualizacao)) -> Result<Option<String>> {
    let instalador = pasta.join("UrbanVPN-Setup.exe");
    baixar(URL_URBAN, &instalador, None, |bytes, _| {
        relatar(atualizacao(
            Componente::UrbanVpn,
            EstadoComponente::Baixando,
            &format!("Baixando UrbanVPN… {}", formatar_bytes(bytes)),
            0.46,
        ));
    })?;
    let tamanho_baixado = instalador.metadata().map(|m| m.len()).unwrap_or_default();
    let hash_baixado = sha256(&instalador).context("calculando o SHA-256 do UrbanVPN baixado")?;
    crate::log::linha(&format!(
        "UrbanVPN baixado: {tamanho_baixado} bytes; SHA-256 {hash_baixado}; arquivo {}",
        instalador.display()
    ));
    relatar(atualizacao(
        Componente::UrbanVpn,
        EstadoComponente::Validando,
        "Validando a assinatura digital da Urban Cyber Security Inc.…",
        0.55,
    ));
    validar_assinatura_urban(&instalador)?;
    relatar(atualizacao(
        Componente::UrbanVpn,
        EstadoComponente::Instalando,
        "Instalando UrbanVPN; confirme a solicitação do Windows…",
        0.62,
    ));
    let erro_execucao = executar_instalador(&instalador, &["/exenoui", "/qn", "/norestart"])
        .context("executando o instalador oficial do UrbanVPN")
        .err()
        .map(|erro| format!("{erro:#}"));
    if let Some(erro) = &erro_execucao {
        crate::log::linha(&format!(
            "instalador do UrbanVPN retornou erro; aguardando redetecção: {erro:#}"
        ));
    }
    Ok(erro_execucao)
}

fn escolher_dorion(release: &Value) -> Result<(String, String, u64)> {
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
    let tamanho = asset["size"].as_u64().unwrap_or_default();
    Ok((url.to_owned(), hash.to_owned(), tamanho))
}

fn baixar(
    url: &str,
    destino: &Path,
    tamanho: Option<u64>,
    mut progresso: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let curl = ferramenta_sistema("curl.exe")?;
    let mut processo = comando_oculto(curl)
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
        .spawn()
        .with_context(|| format!("iniciando o download de {url}"))?;
    let status = loop {
        if let Some(status) = processo.try_wait()? {
            break status;
        }
        let bytes = destino.metadata().map(|m| m.len()).unwrap_or_default();
        progresso(bytes, tamanho);
        std::thread::sleep(Duration::from_millis(150));
    };
    if !status.success() || !destino.is_file() {
        bail!("download oficial falhou: {url} ({status})");
    }
    progresso(
        destino.metadata().map(|m| m.len()).unwrap_or_default(),
        tamanho,
    );
    Ok(())
}

fn formatar_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else {
        format!("{} KB", bytes / 1024)
    }
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
    #[cfg(windows)]
    {
        validar_assinatura_windows(caminho)
    }
    #[cfg(not(windows))]
    {
        let _ = caminho;
        bail!("a assinatura Authenticode só pode ser validada no Windows")
    }
}

#[cfg(windows)]
fn validar_assinatura_windows(caminho: &Path) -> Result<()> {
    use windows_sys::Win32::Security::{
        Cryptography::{CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE},
        WinTrust::{
            WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain,
            WTHelperProvDataFromStateData, WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2,
            WINTRUST_DATA, WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_NONE,
            WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UICONTEXT_INSTALL, WTD_UI_NONE,
        },
    };

    let caminho_wide: Vec<u16> = caminho.as_os_str().encode_wide().chain(Some(0)).collect();

    // SAFETY: as estruturas seguem o contrato do WinVerifyTrust; os ponteiros
    // apontam para dados vivos durante toda a chamada e o estado é fechado.
    unsafe {
        let mut arquivo: WINTRUST_FILE_INFO = std::mem::zeroed();
        arquivo.cbStruct = std::mem::size_of::<WINTRUST_FILE_INFO>() as u32;
        arquivo.pcwszFilePath = caminho_wide.as_ptr();

        let mut dados: WINTRUST_DATA = std::mem::zeroed();
        dados.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
        dados.dwUIChoice = WTD_UI_NONE;
        dados.fdwRevocationChecks = WTD_REVOKE_NONE;
        dados.dwUnionChoice = WTD_CHOICE_FILE;
        dados.Anonymous.pFile = &mut arquivo;
        dados.dwStateAction = WTD_STATEACTION_VERIFY;
        dados.dwUIContext = WTD_UICONTEXT_INSTALL;

        let mut acao = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let status = WinVerifyTrust(
            std::ptr::null_mut(),
            &mut acao,
            &mut dados as *mut WINTRUST_DATA as *mut std::ffi::c_void,
        );

        let resultado = if status != 0 {
            Err(anyhow::anyhow!(
                "a assinatura Authenticode não é confiável (0x{:08x})",
                status as u32
            ))
        } else {
            let provedor = WTHelperProvDataFromStateData(dados.hWVTStateData);
            let assinante = if provedor.is_null() {
                std::ptr::null_mut()
            } else {
                WTHelperGetProvSignerFromChain(provedor, 0, 0, 0)
            };
            let certificado = if assinante.is_null() {
                std::ptr::null_mut()
            } else {
                WTHelperGetProvCertFromChain(assinante, 0)
            };

            if certificado.is_null() || (*certificado).pCert.is_null() {
                Err(anyhow::anyhow!(
                    "o Windows validou a assinatura, mas não informou o publicador"
                ))
            } else {
                let contexto = (*certificado).pCert;
                let tamanho = CertGetNameStringW(
                    contexto,
                    CERT_NAME_SIMPLE_DISPLAY_TYPE,
                    0,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    0,
                );
                if tamanho <= 1 {
                    Err(anyhow::anyhow!(
                        "não foi possível ler o publicador da assinatura"
                    ))
                } else {
                    let mut nome = vec![0_u16; tamanho as usize];
                    let lidos = CertGetNameStringW(
                        contexto,
                        CERT_NAME_SIMPLE_DISPLAY_TYPE,
                        0,
                        std::ptr::null(),
                        nome.as_mut_ptr(),
                        tamanho,
                    );
                    let publicador =
                        String::from_utf16_lossy(&nome[..lidos.saturating_sub(1) as usize]);
                    if publicador.eq_ignore_ascii_case("Urban Cyber Security Inc.") {
                        Ok(publicador)
                    } else {
                        Err(anyhow::anyhow!(
                            "publicador inesperado na assinatura: {publicador}"
                        ))
                    }
                }
            }
        };

        dados.dwStateAction = WTD_STATEACTION_CLOSE;
        let _ = WinVerifyTrust(
            std::ptr::null_mut(),
            &mut acao,
            &mut dados as *mut WINTRUST_DATA as *mut std::ffi::c_void,
        );

        let publicador = resultado?;
        crate::log::linha(&format!(
            "assinatura Authenticode válida; publicador confirmado: {publicador}"
        ));
        Ok(())
    }
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
    let status = match Command::new(caminho).args(argumentos).status() {
        Ok(status) => status,
        #[cfg(windows)]
        Err(erro) if erro.raw_os_error() == Some(740) => {
            crate::log::linha(
                "o instalador exige privilégios administrativos; solicitando elevação pelo UAC",
            );
            return executar_instalador_elevado(caminho, argumentos);
        }
        Err(erro) => return Err(erro).with_context(|| format!("abrindo {}", caminho.display())),
    };
    if codigo_aceito(status) {
        Ok(())
    } else {
        bail!("o instalador terminou com {status}")
    }
}

#[cfg(windows)]
fn executar_instalador_elevado(caminho: &Path, argumentos: &[&str]) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE},
        UI::{
            Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    let verbo: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
    let arquivo: Vec<u16> = caminho.as_os_str().encode_wide().chain(Some(0)).collect();
    let parametros_texto = argumentos.join(" ");
    let parametros: Vec<u16> = OsStr::new(&parametros_texto)
        .encode_wide()
        .chain(Some(0))
        .collect();

    // SAFETY: todos os ponteiros permanecem válidos durante ShellExecuteExW;
    // SEE_MASK_NOCLOSEPROCESS devolve um handle que é esperado e fechado aqui.
    unsafe {
        let mut execucao: SHELLEXECUTEINFOW = std::mem::zeroed();
        execucao.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        execucao.fMask = SEE_MASK_NOCLOSEPROCESS;
        execucao.lpVerb = verbo.as_ptr();
        execucao.lpFile = arquivo.as_ptr();
        execucao.lpParameters = if argumentos.is_empty() {
            std::ptr::null()
        } else {
            parametros.as_ptr()
        };
        execucao.nShow = SW_SHOWNORMAL;

        if ShellExecuteExW(&mut execucao) == 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!(
                    "o Windows não autorizou a instalação administrativa de {}",
                    caminho.display()
                )
            });
        }
        if execucao.hProcess.is_null() {
            bail!(
                "o Windows iniciou {}, mas não devolveu o processo para acompanhamento",
                caminho.display()
            );
        }

        let espera = WaitForSingleObject(execucao.hProcess, INFINITE);
        let mut codigo = u32::MAX;
        let leu_codigo = GetExitCodeProcess(execucao.hProcess, &mut codigo) != 0;
        let _ = CloseHandle(execucao.hProcess);

        if espera != WAIT_OBJECT_0 {
            bail!("não foi possível acompanhar o instalador elevado ({espera})");
        }
        if !leu_codigo {
            return Err(std::io::Error::last_os_error())
                .context("lendo o resultado do instalador elevado");
        }
        if codigo == 0 || matches!(codigo, 1641 | 3010) {
            Ok(())
        } else {
            bail!("o instalador elevado terminou com o código {codigo}")
        }
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
        let (url, hash, tamanho) = escolher_dorion(&release).unwrap();
        assert!(url.ends_with("_x64-setup.exe"));
        assert_eq!(hash, "a".repeat(64));
        assert_eq!(tamanho, 0);
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
