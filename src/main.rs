//! Dorion GoLive — usa um túnel completo somente na negociação das lives.
//!
//! O Discord usa UDP/WebRTC no vídeo; por isso um PAC/SOCKS não basta. Esta
//! versão liga o UrbanVPN antes da abertura/ação, confirma o adaptador do
//! Windows, deixa o Dorion negociar a transmissão e desliga o túnel em seguida.

#![windows_subsystem = "windows"]

mod controle_live;
mod discord;
mod dorion_eventos;
mod instalador;
mod instalador_ui;
mod log;
mod plugin;
mod processos;
mod urban;
mod windows;

use anyhow::{Context, Result};
use std::{ffi::OsStr, path::PathBuf, process::Command, time::Duration};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const PORTA_PAC: u16 = 9351;
const PORTA_CONTROLE: u16 = 9352;
const INTERVALO_VIGIA: Duration = Duration::from_millis(500);
const ASSENTAMENTO_REINICIO_PROPRIO: Duration = Duration::from_secs(45);

#[derive(Debug, PartialEq, Eq)]
struct OpcoesInstalar {
    reiniciar_discord: bool,
    criar_run_legado: bool,
}

fn opcoes_instalar(args: &[String]) -> OpcoesInstalar {
    OpcoesInstalar {
        reiniciar_discord: !args.iter().any(|arg| arg == "--sem-reiniciar"),
        criar_run_legado: !args.iter().any(|arg| arg == "--sem-autostart"),
    }
}

fn manter_arquivos(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--manter-arquivos")
}

fn url_pac() -> String {
    format!("http://127.0.0.1:{PORTA_PAC}/proxy.pac")
}

pub fn pasta_dados() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("DorionGoLive")
}

pub fn caminho_log() -> PathBuf {
    pasta_dados().join("dorion-golive.log")
}

fn caminho_instalado() -> PathBuf {
    pasta_dados().join("dorion-golive.exe")
}

/// Todo processo auxiliar nasce sem console. O núcleo é executado tanto pelo
/// PowerShell quanto pela janela; neste último caso, ferramentas como
/// `taskkill` criariam uma caixa preta curta se não receberem esta flag.
fn comando_oculto(programa: impl AsRef<OsStr>) -> Command {
    let mut comando = Command::new(programa);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        comando.creation_flags(CREATE_NO_WINDOW);
    }
    comando
}

fn main() -> Result<()> {
    anexar_console();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return instalacao_interativa();
    }
    let comando = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or("ajuda");

    match comando {
        "instalar" => instalar(opcoes_instalar(&args)),
        "desinstalar" => desinstalar(manter_arquivos(&args)),
        "desinstalar-ui" => desinstalacao_interativa(),
        "status" => status(),
        "reiniciar-dorion" | "reiniciar-discord" => reiniciar_discord(),
        "rodar" => rodar(),
        _ => {
            ajuda();
            Ok(())
        }
    }
}

/// O mesmo binário publicado como `DorionGoLive-Setup.exe` vira o serviço
/// depois da instalação. Abrir o arquivo sem argumentos aciona esta interface;
/// o autostart sempre usa o argumento `rodar` e nunca entra no instalador.
fn instalacao_interativa() -> Result<()> {
    instalador_ui::executar()
}

fn desinstalacao_interativa() -> Result<()> {
    if !instalador::confirmar_desinstalacao() {
        return Ok(());
    }
    match desinstalar(false) {
        Ok(()) => {
            instalador::informar_desinstalado();
            Ok(())
        }
        Err(erro) => {
            instalador::informar_erro(&format!("{erro:#}"));
            Err(erro)
        }
    }
}

fn ajuda() {
    println!(
        "\ndorion-golive {}\n\n\
         Uso:\n  \
         dorion-golive instalar      liga a correção, reinicia o Dorion e sobe com o Windows\n  \
         dorion-golive desinstalar   remove tudo e restaura o proxy anterior\n  \
         dorion-golive status        mostra o estado atual\n  \
         dorion-golive reiniciar-dorion fecha e abre só o Dorion\n  \
         dorion-golive rodar         roda em primeiro plano (para depurar)\n\n\
         Opções:\n  \
         --sem-reiniciar           não mexe no Dorion aberto; a correção vale na\n                            \
         próxima vez que você abrir\n  \
         --sem-autostart           não cria a entrada Run legada (uso do setup)\n  \
         --manter-arquivos         limpa a configuração sem apagar a pasta instalada\n",
        env!("CARGO_PKG_VERSION")
    );
}

fn instalar(opcoes: OpcoesInstalar) -> Result<()> {
    let destino = caminho_instalado();
    std::fs::create_dir_all(pasta_dados()).context("criando a pasta de dados")?;

    let atual = std::env::current_exe()?;
    if atual != destino {
        // Se já havia uma cópia rodando, ela precisa sair antes de ser trocada.
        encerrar_outras_instancias();
        std::fs::copy(&atual, &destino).context("copiando o executável")?;
    }

    // Valida o Urban e instala os dois componentes antes de alterar qualquer
    // configuração anterior. O auxiliar fica embutido neste executável.
    urban::instalar_auxiliar()?;
    plugin::instalar()?;

    if opcoes.criar_run_legado {
        windows::ativar_autostart(&format!("\"{}\" rodar", destino.display()))
            .context("registrando o autostart")?;
    }
    // Migração das versões 0.1.x: devolve o PAC anterior. A versão de túnel
    // completo não usa proxy automático.
    windows::desativar_pac(&url_pac()).context("removendo o PAC antigo")?;
    let _ = windows::adicionar_ao_path(&pasta_dados().display().to_string());
    windows::registrar_desinstalacao(&destino, &pasta_dados())?;

    // O marcador é de quem está subindo agora, não da instalação anterior — e
    // a mesma regra vale para a hora da última checagem: mostrar "há 3 d" numa
    // instalação recém-feita seria a janela mentindo sobre si mesma.
    let _ = std::fs::remove_file(pasta_dados().join("pronto"));
    let _ = std::fs::remove_file(pasta_dados().join("ultima-validacao-ms"));

    // Mesmo o processo principal é criado sem console: o instalador pode ter
    // sido chamado pela janela, pelo autostart ou pelo PowerShell.
    comando_oculto(&destino)
        .arg("rodar")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("subindo o serviço")?;

    let mut servico_pronto = false;
    for _ in 0..20 {
        if porta_ocupada(PORTA_CONTROLE) {
            servico_pronto = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    if !servico_pronto {
        anyhow::bail!(
            "o serviço foi copiado, mas não começou a responder na porta local {PORTA_CONTROLE}"
        );
    }

    println!("\nInstalado.\n");
    println!("  executável : {}", destino.display());
    println!("  log        : {}", caminho_log().display());
    println!(
        "  autostart  : {}",
        if opcoes.criar_run_legado {
            "sim"
        } else {
            "gerenciado pela interface"
        }
    );
    println!("  VPN        : UrbanVPN (túnel completo temporário)");
    println!("  PAC        : desligado");

    if opcoes.reiniciar_discord && discord::esta_rodando() {
        println!("\nO serviço detectou o Dorion aberto e fará a reabertura pela VPN.");
    } else {
        println!("\nNa próxima abertura do Dorion, a preparação será automática.");
    }

    println!("\nEm um terminal novo, o comando `dorion-golive` já funciona sozinho.");
    Ok(())
}

fn desinstalar(manter_arquivos: bool) -> Result<()> {
    windows::validar_autostart(&caminho_instalado())?;
    encerrar_outras_instancias();
    let vpn = urban::UrbanVpn::nova();
    let _ = vpn.desligar_forcado();
    plugin::desinstalar().context("removendo o detector de live do Dorion")?;
    windows::desativar_pac(&url_pac()).context("devolvendo o proxy automático")?;
    windows::desativar_autostart(&caminho_instalado()).context("removendo o autostart")?;
    let _ = windows::remover_do_path(&pasta_dados().display().to_string());
    windows::remover_registro_desinstalacao()?;
    urban::remover_auxiliar();

    // Fecha o Dorion sem reabrir: reabrir agora, com o proxy já desligado, é
    // exatamente o que o usuário quer — mas deixamos a escolha com ele.
    let estava_aberto = discord::encerrar_se_aberto();
    if !manter_arquivos {
        agendar_remocao_apos_sair()?;
    }

    println!(
        "{} A VPN temporária foi desligada e o proxy do Windows foi restaurado.",
        if manter_arquivos {
            "Configuração removida; os arquivos foram preservados para o setup."
        } else {
            "Removido."
        }
    );
    if estava_aberto {
        println!("O Dorion foi fechado. Abra de novo e ele já sai pelo seu IP normal.");
    } else {
        println!("Na próxima abertura, o Dorion já sai pelo seu IP normal.");
    }
    Ok(())
}

/// No Windows um processo não consegue apagar o próprio executável aberto.
/// Um PowerShell sem arquivo espera este PID terminar e remove apenas a pasta
/// fixa de dados do aplicativo.
fn agendar_remocao_apos_sair() -> Result<()> {
    let system_root = std::env::var_os("SystemRoot").context("SystemRoot não está definido")?;
    let powershell = PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if !powershell.is_file() {
        anyhow::bail!("PowerShell do Windows não encontrado");
    }
    const REMOVER: &str = r#"
Wait-Process -Id ([int]$env:DORION_GOLIVE_REMOVE_PID) -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300
Remove-Item -LiteralPath $env:DORION_GOLIVE_REMOVE_DIR -Recurse -Force -ErrorAction SilentlyContinue
"#;
    comando_oculto(powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", REMOVER])
        .env("DORION_GOLIVE_REMOVE_PID", std::process::id().to_string())
        .env("DORION_GOLIVE_REMOVE_DIR", pasta_dados())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("agendando a remoção dos arquivos instalados")?;
    Ok(())
}

fn status() -> Result<()> {
    println!("\ndorion-golive {}\n", env!("CARGO_PKG_VERSION"));
    println!("  instalado  : {}", sim_nao(caminho_instalado().exists()));
    println!("  autostart  : {}", sim_nao(windows::autostart_ativo()));
    println!("  PAC legado : {}", sim_nao(windows::pac_ativo(&url_pac())));
    println!("  rodando    : {}", sim_nao(porta_ocupada(PORTA_CONTROLE)));
    println!(
        "  no PATH    : {}",
        sim_nao(windows::path_ativo(&pasta_dados().display().to_string()))
    );
    println!("  UrbanVPN   : {}", sim_nao(urban::disponivel()));
    println!("  VPN ativa  : {}", sim_nao(urban::ativo_agora()));
    println!("  plugin     : {}", sim_nao(plugin::ativo()));
    println!("  log        : {}", caminho_log().display());
    Ok(())
}

/// Reinicia somente o Dorion. Não passa pela instalação nem aguarda a
/// validação da piscina: a interface usa este caminho quando o serviço já
/// está em execução.
fn reiniciar_discord() -> Result<()> {
    match discord::reiniciar()? {
        true => println!("Dorion reiniciado."),
        false => println!("Dorion não encontrado."),
    }
    Ok(())
}

/// Encerra cópias antigas do serviço — e só elas. O filtro por PID existe
/// porque o instalador tem o mesmo nome de imagem e mataria a si próprio.
fn encerrar_outras_instancias() {
    let eu = std::process::id();
    let antigas: Vec<u32> = processos::pids_por_nome("dorion-golive.exe")
        .into_iter()
        .filter(|pid| *pid != eu)
        .collect();
    processos::encerrar_todos(&antigas);
}

fn sim_nao(b: bool) -> &'static str {
    if b {
        "sim"
    } else {
        "não"
    }
}

fn porta_ocupada(porta: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{porta}").parse().unwrap(),
        Duration::from_millis(300),
    )
    .is_ok()
}

/// Intercepta qualquer forma de abrir o Dorion. Como o processo já pode ter
/// feito uma conexão direta quando aparece na lista, o vigia liga a VPN,
/// reinicia uma única vez e só então conta a estabilização. Isso também cobre
/// atalho da área de trabalho, menu Iniciar e executável aberto diretamente.
fn vigiar_abertura(vpn: std::sync::Arc<urban::UrbanVpn>) {
    std::thread::spawn(move || {
        let mut conhecido = None;
        let mut assentando_ate = None;
        loop {
            let atual = discord::principal();
            match atual {
                None => {
                    conhecido = None;
                    // Se o usuário realmente fechou o Dorion, a próxima
                    // abertura é nova mesmo que aconteça logo após a nossa.
                    assentando_ate = None;
                }
                Some(identidade) if Some(identidade) != conhecido => {
                    // Durante a inicialização, a árvore do WebView2 pode fazer
                    // a descoberta do processo principal trocar de identidade.
                    // Depois de um reinício feito por nós, isso é a mesma
                    // abertura assentando e não pode provocar outro reinício.
                    if assentando_ate.is_some_and(|limite| std::time::Instant::now() < limite) {
                        conhecido = Some(identidade);
                        std::thread::sleep(INTERVALO_VIGIA);
                        continue;
                    }
                    log::linha("Dorion aberto; preparando a primeira conexão pela VPN");
                    match vpn.ativar("abertura do Dorion") {
                        Ok(janela) => {
                            let reiniciado = match discord::reiniciar() {
                                Ok(true) => {
                                    log::linha("Dorion reiniciado dentro do túnel completo");
                                    let limite =
                                        std::time::Instant::now() + Duration::from_secs(15);
                                    while std::time::Instant::now() < limite {
                                        if discord::principal()
                                            .is_some_and(|novo| novo != identidade)
                                        {
                                            break;
                                        }
                                        std::thread::sleep(Duration::from_millis(250));
                                    }
                                    // O processo novo já nasceu sob a VPN. Só libera o
                                    // túnel depois que plugins e conexão realmente
                                    // estabilizaram; o prazo apenas evita VPN esquecida.
                                    if dorion_eventos::aguardar_inicializacao(Duration::from_secs(
                                        40,
                                    )) {
                                        log::linha("Dorion confirmou plugins e conexão estáveis");
                                    } else {
                                        log::linha(
                                            "Dorion não confirmou estabilidade no prazo; encerrando a VPN por segurança",
                                        );
                                    }
                                    true
                                }
                                Ok(false) => {
                                    log::linha("executável do Dorion não encontrado");
                                    false
                                }
                                Err(e) => {
                                    log::linha(&format!(
                                        "não foi possível reiniciar o Dorion pela VPN: {e}"
                                    ));
                                    false
                                }
                            };
                            let motivo = if reiniciado {
                                "Dorion inicializado"
                            } else {
                                "abertura do Dorion falhou"
                            };
                            if let Err(e) = vpn.desativar(janela, motivo) {
                                log::linha(&format!(
                                    "falha ao desligar UrbanVPN após a abertura: {e}"
                                ));
                            }
                            conhecido = discord::principal();
                            assentando_ate = reiniciado
                                .then(|| std::time::Instant::now() + ASSENTAMENTO_REINICIO_PROPRIO);
                        }
                        Err(e) => {
                            log::linha(&format!(
                                "não foi possível preparar a abertura pelo UrbanVPN: {e}"
                            ));
                            conhecido = Some(identidade);
                        }
                    }
                }
                Some(_) => {}
            }
            std::thread::sleep(INTERVALO_VIGIA);
        }
    });
}

fn rodar() -> Result<()> {
    std::fs::create_dir_all(pasta_dados()).context("criando pasta de dados")?;
    urban::instalar_auxiliar()?;
    // Garante que uma atualização sobre 0.1.x não deixe o PAC antigo ativo.
    windows::desativar_pac(&url_pac()).context("removendo PAC legado")?;

    let vpn = std::sync::Arc::new(urban::UrbanVpn::nova());
    let estado_streaming = std::sync::Arc::new(dorion_eventos::EstadoStreaming::default());
    vigiar_abertura(vpn.clone());
    dorion_eventos::vigiar(vpn.clone(), estado_streaming.clone());
    log::linha("serviço 0.2 ativo: UrbanVPN temporário, PAC/SOCKS desligados");
    controle_live::servir(PORTA_CONTROLE, vpn, estado_streaming)
}

/// Compilado como aplicativo de janela para não piscar console no autostart.
/// Quando chamado de um terminal, adota o console de quem chamou — e reabre
/// as saídas padrão apontando para ele, senão `println!` escreveria no vazio.
fn anexar_console() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
            Storage::FileSystem::{
                CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
                OPEN_EXISTING,
            },
            System::Console::{
                AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
                STD_OUTPUT_HANDLE,
            },
        };

        // Se quem nos chamou já entregou uma saída — um pipe, um arquivo, um
        // `>` —, ela é a saída correta. Sobrescrevê-la pelo console faria
        // `dorion-golive status > arquivo` gravar nada.
        let ja_temos = GetStdHandle(STD_OUTPUT_HANDLE);
        if !ja_temos.is_null() && ja_temos != INVALID_HANDLE_VALUE {
            return;
        }

        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return; // sem terminal chamador: rodando pelo autostart
        }

        let nome: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        let saida = CreateFileW(
            nome.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if saida != INVALID_HANDLE_VALUE {
            SetStdHandle(STD_OUTPUT_HANDLE, saida);
            SetStdHandle(STD_ERROR_HANDLE, saida);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_da_interface_instala_o_servico_sem_run_legado() {
        assert_eq!(
            opcoes_instalar(&[
                "instalar".into(),
                "--sem-reiniciar".into(),
                "--sem-autostart".into(),
            ]),
            OpcoesInstalar {
                reiniciar_discord: false,
                criar_run_legado: false,
            },
        );
    }

    #[test]
    fn cli_sem_novas_opcoes_mantem_o_autostart_legado() {
        assert_eq!(
            opcoes_instalar(&["instalar".into()]),
            OpcoesInstalar {
                reiniciar_discord: true,
                criar_run_legado: true,
            },
        );
    }

    #[test]
    fn desinstalar_com_manter_arquivos_nao_remove_a_pasta() {
        assert!(manter_arquivos(&[
            "desinstalar".into(),
            "--manter-arquivos".into()
        ]));
        assert!(!manter_arquivos(&["desinstalar".into()]));
    }
}
