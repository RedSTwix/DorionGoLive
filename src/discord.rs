//! Localiza e reinicia o Dorion.
//!
//! A correção só vale a partir da próxima abertura do Dorion. Pedir isso ao
//! usuário é um passo que ele esquece — então o instalador faz sozinho.

use anyhow::Result;
use std::{ffi::OsStr, path::PathBuf, process::Command, time::Duration};
use winreg::{enums::*, RegKey};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::processos::Processo;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Identidade {
    pub pid: u32,
    pub criado_em: u64,
}

/// O executável do Dorion é detalhe interno e sempre nasce sem console.
fn comando_oculto(programa: impl AsRef<OsStr>) -> Command {
    let mut comando = Command::new(programa);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        comando.creation_flags(CREATE_NO_WINDOW);
    }
    comando
}

pub fn lancador() -> Option<PathBuf> {
    candidatos()
        .into_iter()
        .find(|p| p.is_file())
        .or_else(atalho)
}

/// O clique no atalho é o caminho de abertura que o usuário confirmou manter
/// corretamente os participantes `AO VIVO`. Abrir `Dorion.exe` como filho do
/// serviço não é equivalente, mesmo com a mesma pasta de trabalho.
fn atalho() -> Option<PathBuf> {
    candidatos_de_atalho().into_iter().find(|p| p.exists())
}

fn candidatos_de_atalho() -> Vec<PathBuf> {
    let mut caminhos = Vec::new();
    if let Ok(base) = std::env::var("USERPROFILE") {
        caminhos.push(PathBuf::from(base).join("Desktop").join("Dorion.lnk"));
    }
    if let Ok(base) = std::env::var("OneDrive") {
        caminhos.push(PathBuf::from(base).join("Desktop").join("Dorion.lnk"));
    }
    if let Ok(base) = std::env::var("PUBLIC") {
        caminhos.push(PathBuf::from(base).join("Desktop").join("Dorion.lnk"));
    }
    if let Ok(base) = std::env::var("APPDATA") {
        caminhos.push(
            PathBuf::from(base)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Dorion.lnk"),
        );
    }
    if let Ok(base) = std::env::var("ProgramData") {
        caminhos.push(
            PathBuf::from(base)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Dorion.lnk"),
        );
    }
    caminhos
}

fn candidatos() -> Vec<PathBuf> {
    let mut caminhos = Vec::new();
    if let Ok(base) = std::env::var("ProgramW6432") {
        caminhos.push(PathBuf::from(base).join("Dorion").join("Dorion.exe"));
    }
    if let Ok(base) = std::env::var("ProgramFiles") {
        caminhos.push(PathBuf::from(base).join("Dorion").join("Dorion.exe"));
    }
    if let Ok(base) = std::env::var("ProgramFiles(x86)") {
        caminhos.push(PathBuf::from(base).join("Dorion").join("Dorion.exe"));
    }
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        // O instalador NSIS oficial do Tauri usa este caminho no modo por
        // usuário. Ele é diferente do caminho adotado por alguns gerenciadores
        // de pacotes e precisa ser verificado diretamente.
        caminhos.push(PathBuf::from(&base).join("Dorion").join("Dorion.exe"));
        caminhos.push(
            PathBuf::from(base)
                .join("Programs")
                .join("Dorion")
                .join("Dorion.exe"),
        );
    }
    caminhos.extend(candidatos_do_registro());
    caminhos.sort_by_key(|p| p.to_string_lossy().to_ascii_lowercase());
    caminhos.dedup_by(|a, b| {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    });
    caminhos
}

fn candidatos_do_registro() -> Vec<PathBuf> {
    let mut encontrados = Vec::new();
    for raiz in [
        RegKey::predef(HKEY_CURRENT_USER),
        RegKey::predef(HKEY_LOCAL_MACHINE),
    ] {
        for chave_base in [
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let Ok(base) = raiz.open_subkey_with_flags(chave_base, KEY_READ) else {
                continue;
            };
            for nome in base.enum_keys().filter_map(Result::ok) {
                let Ok(chave) = base.open_subkey_with_flags(nome, KEY_READ) else {
                    continue;
                };
                let display = chave
                    .get_value::<String, _>("DisplayName")
                    .unwrap_or_default();
                if !display.eq_ignore_ascii_case("Dorion") {
                    continue;
                }
                if let Ok(pasta) = chave.get_value::<String, _>("InstallLocation") {
                    encontrados.push(PathBuf::from(pasta.trim_matches('"')).join("Dorion.exe"));
                }
                if let Ok(icone) = chave.get_value::<String, _>("DisplayIcon") {
                    let arquivo = icone
                        .trim()
                        .trim_matches('"')
                        .split(',')
                        .next()
                        .unwrap_or_default();
                    if !arquivo.is_empty() {
                        encontrados.push(PathBuf::from(arquivo));
                    }
                }
            }
        }
    }
    encontrados
}

const IMAGEM: &str = "Dorion.exe";

pub fn esta_rodando() -> bool {
    crate::processos::esta_rodando(IMAGEM)
}

/// O processo principal do Dorion no ar, com a hora em que nasceu.
///
/// O principal é o único cujo pai não é outro `Dorion.exe`. A hora de
/// criação vai junto porque o Windows reaproveita PIDs depressa o bastante
/// para um Dorion novo nascer com o número do antigo.
pub fn principal() -> Option<Identidade> {
    principal_entre(
        &crate::processos::processos_por_nome(IMAGEM),
        crate::processos::criado_em,
    )
}

fn principal_entre(
    processos: &[Processo],
    criado_em: impl Fn(u32) -> Option<u64>,
) -> Option<Identidade> {
    let nascidos: Vec<(Processo, Option<u64>)> =
        processos.iter().map(|p| (*p, criado_em(p.pid))).collect();

    // Um pai de verdade nasceu antes do filho. O lançador morre logo depois
    // de criar o principal, e o PID dele pode ser reaproveitado por um filho
    // do próprio Dorion — aí o principal pareceria ter pai Dorion, ninguém
    // sobraria como raiz, e o vigia declararia o Dorion fechado com ele
    // aberto. Sem hora de criação de um dos dois, vale só o PID.
    let e_pai_de_verdade = |pai: u32, filho_nasceu: Option<u64>| {
        nascidos.iter().any(|(q, q_nasceu)| {
            q.pid == pai
                && match (q_nasceu, filho_nasceu) {
                    (Some(pai_nasceu), Some(filho_nasceu)) => *pai_nasceu <= filho_nasceu,
                    _ => true,
                }
        })
    };

    // Entre candidatos, o mais antigo — e quem tem hora conhecida vence quem
    // não tem, para um processo que o Windows não deixa consultar nunca
    // passar na frente do Dorion deste usuário.
    let mais_antigo =
        |(p, nasceu): &&(Processo, Option<u64>)| (nasceu.is_none(), nasceu.unwrap_or(0), p.pid);

    let raiz = nascidos
        .iter()
        .filter(|(p, nasceu)| !e_pai_de_verdade(p.pai, *nasceu))
        .min_by_key(mais_antigo)
        // Com Dorion na lista sempre há um Dorion: se a árvore ficou sem
        // raiz por um pai fantasma, o mais antigo de todos é o principal.
        .or_else(|| nascidos.iter().min_by_key(mais_antigo))?;

    Some(Identidade {
        pid: raiz.0.pid,
        criado_em: raiz.1.unwrap_or(0),
    })
}

/// Encerra todas as janelas do Dorion e só volta quando elas saíram de fato.
/// A espera é por handle de processo, não por relógio: reabrir cedo demais faz
/// o Dorion fixar de novo a região errada.
fn encerrar() {
    crate::processos::encerrar_por_nome(IMAGEM);
    // Folga curta para o Windows soltar os arquivos antes do relançamento.
    std::thread::sleep(Duration::from_millis(500));
}

/// Fecha e reabre o Dorion. Devolve `false` quando não há Dorion instalado
/// — o que não é erro: o serviço fica de pé e corrige na primeira abertura.
pub fn reiniciar() -> Result<bool> {
    let Some(lancador) = lancador() else {
        return Ok(false);
    };
    let estava_aberto = esta_rodando();
    if estava_aberto {
        encerrar();
    }
    let (programa, argumento) = atalho()
        .map(|atalho| (PathBuf::from("explorer.exe"), Some(atalho)))
        .unwrap_or_else(|| (lancador.clone(), None));
    let mut comando = comando_oculto(programa);
    if let Some(atalho) = argumento {
        // Passar o .lnk ao Explorer usa exatamente o Shell do clique na área
        // de trabalho, inclusive o "Iniciar em" gravado no próprio atalho.
        comando.arg(atalho);
    } else if let Some(pasta) = lancador.parent() {
        comando.current_dir(pasta);
    }
    comando
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(true)
}

/// Só encerra, sem reabrir. Usado na desinstalação: reabrir na hora faria o
/// o Dorion fixar de novo a região errada.
pub fn encerrar_se_aberto() -> bool {
    if esta_rodando() {
        encerrar();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static AMBIENTE: Mutex<()> = Mutex::new(());

    fn p(pid: u32, pai: u32) -> Processo {
        Processo { pid, pai }
    }

    /// A árvore de uma máquina de verdade: o principal nasceu do lançador
    /// (que já saiu) e os seis filhos nasceram dele.
    fn arvore_comum() -> Vec<Processo> {
        vec![
            p(4000, 3990),
            p(4100, 4000),
            p(4200, 4000),
            p(4300, 4000),
            p(4400, 4000),
            p(4500, 4000),
            p(4600, 4000),
        ]
    }

    #[test]
    fn o_principal_e_quem_nao_tem_pai_discord() {
        let principal = principal_entre(&arvore_comum(), |pid| Some(u64::from(pid) * 10));
        assert_eq!(
            principal,
            Some(Identidade {
                pid: 4000,
                criado_em: 40_000
            })
        );
    }

    #[test]
    fn filho_novo_nao_muda_o_principal() {
        let mut arvore = arvore_comum();
        let antes = principal_entre(&arvore, |_| Some(1));

        // Um renderizador morreu e outro nasceu: Ctrl+R, troca de servidor.
        arvore.retain(|p| p.pid != 4300);
        arvore.push(p(4700, 4000));
        assert_eq!(principal_entre(&arvore, |_| Some(1)), antes);
    }

    #[test]
    fn sem_dorion_nao_ha_principal() {
        assert_eq!(principal_entre(&[], |_| Some(1)), None);
    }

    #[test]
    fn sem_hora_de_criacao_o_pid_ainda_identifica() {
        // O Windows recusou a pergunta — processo de outro usuário, por
        // exemplo. Melhor um Dorion identificado só pelo PID do que nenhum.
        let principal = principal_entre(&arvore_comum(), |_| None);
        assert_eq!(principal.map(|i| i.pid), Some(4000));
    }

    #[test]
    fn pid_do_lancador_reaproveitado_por_um_filho_nao_esconde_o_principal() {
        // O principal nasceu do lançador 3990, que morreu; um renderizador
        // nasceu depois com esse mesmo número. Pelo PID sozinho a árvore vira
        // um ciclo sem raiz — e o vigia declararia o Dorion fechado com ele
        // aberto, deixando a janela em Abertura sem prazo nenhum.
        let arvore = vec![p(4000, 3990), p(3990, 4000), p(4200, 4000), p(4300, 4000)];
        let hora = |pid: u32| {
            Some(if pid == 4000 {
                100
            } else {
                105 + u64::from(pid)
            })
        };
        assert_eq!(
            principal_entre(&arvore, hora),
            Some(Identidade {
                pid: 4000,
                criado_em: 100
            })
        );

        // Sem hora nenhuma o ciclo não tem como ser desfeito, mas a lista não
        // está vazia: ainda assim há um Dorion, e a escolha é estável.
        let sem_hora = principal_entre(&arvore, |_| None);
        assert!(sem_hora.is_some(), "Dorion de pé nunca vira None");
        assert_eq!(principal_entre(&arvore, |_| None), sem_hora);
    }

    #[test]
    fn raiz_sem_hora_nao_passa_na_frente_da_raiz_com_hora() {
        // Duas raízes: a deste usuário, com hora, e uma que o Windows não
        // deixa consultar. A desconhecida não pode virar a identidade.
        let arvore = vec![p(4000, 1), p(9000, 2)];
        let hora = |pid: u32| (pid == 4000).then_some(500);
        assert_eq!(principal_entre(&arvore, hora).map(|i| i.pid), Some(4000));
    }

    #[test]
    fn com_o_principal_morto_os_filhos_ainda_dao_uma_identidade_estavel() {
        // O principal caiu e os filhos ainda não: todos viram raiz. A escolha
        // é a mais antiga, e continua a mesma enquanto eles não morrerem.
        let orfaos = vec![p(4100, 4000), p(4200, 4000), p(4300, 4000)];
        let hora = |pid: u32| Some(u64::from(pid));
        assert_eq!(principal_entre(&orfaos, hora).map(|i| i.pid), Some(4100));
        assert_eq!(principal_entre(&orfaos, hora).map(|i| i.pid), Some(4100));
    }

    #[test]
    fn atalhos_consideram_area_do_usuario_onedrive_e_publica() {
        let _guarda = AMBIENTE.lock().unwrap();
        let antigas = [
            "USERPROFILE",
            "OneDrive",
            "PUBLIC",
            "APPDATA",
            "ProgramData",
        ]
        .map(|chave| (chave, std::env::var_os(chave)));
        std::env::set_var("USERPROFILE", r"C:\Usuarios\Teste");
        std::env::set_var("OneDrive", r"C:\Usuarios\Teste\OneDrive");
        std::env::set_var("PUBLIC", r"C:\Usuarios\Publico");
        std::env::set_var("APPDATA", r"C:\Usuarios\Teste\AppData\Roaming");
        std::env::set_var("ProgramData", r"C:\ProgramData");

        assert_eq!(
            candidatos_de_atalho(),
            vec![
                PathBuf::from(r"C:\Usuarios\Teste\Desktop\Dorion.lnk"),
                PathBuf::from(r"C:\Usuarios\Teste\OneDrive\Desktop\Dorion.lnk"),
                PathBuf::from(r"C:\Usuarios\Publico\Desktop\Dorion.lnk"),
                PathBuf::from(
                    r"C:\Usuarios\Teste\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Dorion.lnk",
                ),
                PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Dorion.lnk",),
            ]
        );

        for (chave, valor) in antigas {
            match valor {
                Some(valor) => std::env::set_var(chave, valor),
                None => std::env::remove_var(chave),
            }
        }
    }

    #[test]
    fn candidatos_incluem_o_destino_padrao_do_nsis() {
        let _guarda = AMBIENTE.lock().unwrap();
        let antiga = std::env::var_os("LOCALAPPDATA");
        std::env::set_var("LOCALAPPDATA", r"C:\Usuarios\Teste\AppData\Local");
        assert!(candidatos().contains(&PathBuf::from(
            r"C:\Usuarios\Teste\AppData\Local\Dorion\Dorion.exe"
        )));
        match antiga {
            Some(valor) => std::env::set_var("LOCALAPPDATA", valor),
            None => std::env::remove_var("LOCALAPPDATA"),
        }
    }
}
