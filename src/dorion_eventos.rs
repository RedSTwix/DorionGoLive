//! Detector nativo de transmissão pelo log do Dorion.
//!
//! O próprio Dorion registra `Setting tray icon to streaming` quando sua
//! integração interna percebe o início de uma transmissão. Esse sinal é mais
//! estável que classes ou textos do HTML do Discord e continua funcionando se
//! o Vencord substituir `getDisplayMedia` depois do nosso plugin.

use crate::urban::UrbanVpn;
use std::{
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const MARCADOR: &str = "Setting tray icon to ";
const IPC_INICIAR: &str = "golive-start";
const IPC_ESTAVEL: &str = "golive-stable";
const ESTABILIDADE: Duration = Duration::from_secs(8);
const INTERVALO: Duration = Duration::from_millis(250);
const SILENCIO_INICIALIZACAO: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Acao {
    AtivarStreaming,
    AtivarPlugin,
    EstabilizarStreaming,
    EstabilizarPlugin,
}

#[derive(Default)]
pub struct EstadoStreaming {
    confirmacoes: AtomicU64,
}

impl EstadoStreaming {
    pub fn confirmacoes(&self) -> u64 {
        self.confirmacoes.load(Ordering::Acquire)
    }

    fn confirmar(&self) {
        self.confirmacoes.fetch_add(1, Ordering::AcqRel);
    }
}

#[derive(Default)]
struct Detector {
    estado: String,
    streaming_desde: Option<Instant>,
}

#[derive(Default)]
struct Inicializacao {
    plugins_carregados: bool,
    conexao_observada: bool,
    ultima_mudanca: Option<Instant>,
}

impl Inicializacao {
    fn observar(&mut self, linha: &str, agora: Instant) {
        if linha.contains("Preload only: false") {
            self.plugins_carregados = true;
        }
        if let Some(estado) = linha.split_once(MARCADOR).map(|(_, estado)| estado.trim()) {
            if estado == "connected" {
                self.conexao_observada = true;
            }
            if self.conexao_observada && matches!(estado, "connected" | "disconnected") {
                self.ultima_mudanca = Some(agora);
            }
        }
    }

    fn pronta(&self, agora: Instant) -> bool {
        self.plugins_carregados
            && self.conexao_observada
            && self.ultima_mudanca.is_some_and(|ultima| {
                agora.saturating_duration_since(ultima) >= SILENCIO_INICIALIZACAO
            })
    }
}

impl Detector {
    fn observar(&mut self, linha: &str, agora: Instant) -> Option<Acao> {
        let novo = linha.split_once(MARCADOR)?.1.trim();

        // O plugin usa o IPC nativo `set_tray_icon` como uma ponte confiável
        // até este processo. Diferente de HTTP em uma página HTTPS, esse canal
        // não é bloqueado pelas regras de mixed content/PNA do WebView2.
        if novo == IPC_INICIAR {
            self.estado = novo.into();
            self.streaming_desde = None;
            return Some(Acao::AtivarPlugin);
        }
        if novo == IPC_ESTAVEL {
            self.estado = novo.into();
            self.streaming_desde = None;
            return Some(Acao::EstabilizarPlugin);
        }

        if novo == "streaming" && self.estado != "streaming" {
            self.estado = novo.into();
            self.streaming_desde = Some(agora);
            return Some(Acao::AtivarStreaming);
        }
        if novo != "streaming" {
            self.streaming_desde = None;
        }
        self.estado = novo.into();
        None
    }

    fn avaliar(&mut self, agora: Instant) -> Option<Acao> {
        let inicio = self.streaming_desde?;
        if agora.saturating_duration_since(inicio) < ESTABILIDADE {
            return None;
        }
        // Um único encerramento por período contínuo em `streaming`.
        self.streaming_desde = None;
        Some(Acao::EstabilizarStreaming)
    }
}

fn caminho_log() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base)
        .join("dorion")
        .join("logs")
        .join("latest.log")
}

/// Aguarda sinais reais do cliente recém-aberto: plugins carregados, conexão
/// estabelecida e ausência de novas oscilações por alguns segundos. O prazo é
/// apenas uma saída de segurança; ele nunca representa sucesso.
pub fn aguardar_inicializacao(prazo: Duration) -> bool {
    let caminho = caminho_log();
    let inicio = Instant::now();
    let mut posicao = std::fs::metadata(&caminho).map(|m| m.len()).unwrap_or(0);
    let mut estado = Inicializacao::default();

    while inicio.elapsed() < prazo {
        if let Ok(meta) = std::fs::metadata(&caminho) {
            if meta.len() < posicao {
                posicao = 0;
                estado = Inicializacao::default();
            }
            if meta.len() > posicao {
                if let Ok(mut arquivo) = std::fs::File::open(&caminho) {
                    if arquivo.seek(SeekFrom::Start(posicao)).is_ok() {
                        let mut novas = String::new();
                        if arquivo.read_to_string(&mut novas).is_ok() {
                            posicao += novas.len() as u64;
                            for linha in novas.lines() {
                                estado.observar(linha, Instant::now());
                            }
                        }
                    }
                }
            }
        }
        if estado.pronta(Instant::now()) {
            return true;
        }
        std::thread::sleep(INTERVALO);
    }
    false
}

fn executar(
    acao: Acao,
    vpn: &UrbanVpn,
    janela: &mut Option<u64>,
    estado_streaming: &EstadoStreaming,
) {
    match acao {
        Acao::AtivarStreaming => {
            crate::log::linha(
                "Dorion sinalizou streaming; ligando a VPN temporária pelo detector nativo",
            );
            match vpn.ativar("streaming detectado pelo Dorion") {
                Ok(geracao) => *janela = Some(geracao),
                Err(e) => crate::log::linha(&format!("falha ao ligar UrbanVPN: {e}")),
            }
        }
        Acao::AtivarPlugin => {
            crate::log::linha("plugin sinalizou ação de live; ligando a VPN temporária");
            match vpn.ativar("ação de assistir ou transmitir") {
                Ok(geracao) => *janela = Some(geracao),
                Err(e) => crate::log::linha(&format!("falha ao ligar UrbanVPN: {e}")),
            }
        }
        Acao::EstabilizarStreaming => {
            if let Some(geracao) = janela.take() {
                match vpn.desativar(geracao, "fim da negociação da transmissão") {
                    Ok(true) => {
                        estado_streaming.confirmar();
                        crate::log::linha(&format!(
                            "transmissão confirmada pelo Dorion; confirmação nativa {}",
                            estado_streaming.confirmacoes()
                        ));
                    }
                    Ok(false) => {}
                    Err(e) => crate::log::linha(&format!("falha ao desligar UrbanVPN: {e}")),
                }
            }
        }
        Acao::EstabilizarPlugin => {
            if let Some(geracao) = janela.take() {
                if let Err(e) = vpn.desativar(geracao, "fim da negociação da live") {
                    crate::log::linha(&format!("falha ao desligar UrbanVPN: {e}"));
                }
            }
        }
    }
}

/// Começa no fim do arquivo atual para não interpretar uma transmissão antiga
/// como nova. Quando o Dorion reinicia e recria/trunca `latest.log`, o cursor
/// volta ao começo automaticamente.
pub fn vigiar(vpn: Arc<UrbanVpn>, estado_streaming: Arc<EstadoStreaming>) {
    std::thread::spawn(move || {
        let caminho = caminho_log();
        let mut posicao = std::fs::metadata(&caminho).map(|m| m.len()).unwrap_or(0);
        let mut detector = Detector::default();
        let mut janela = None;
        crate::log::linha(&format!(
            "detector nativo acompanhando {}",
            caminho.display()
        ));

        loop {
            if let Ok(meta) = std::fs::metadata(&caminho) {
                if meta.len() < posicao {
                    posicao = 0;
                    detector = Detector::default();
                }
                if meta.len() > posicao {
                    if let Ok(mut arquivo) = std::fs::File::open(&caminho) {
                        if arquivo.seek(SeekFrom::Start(posicao)).is_ok() {
                            let mut novas = String::new();
                            if arquivo.read_to_string(&mut novas).is_ok() {
                                posicao += novas.len() as u64;
                                for linha in novas.lines() {
                                    if let Some(acao) = detector.observar(linha, Instant::now()) {
                                        executar(acao, &vpn, &mut janela, &estado_streaming);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(acao) = detector.avaliar(Instant::now()) {
                executar(acao, &vpn, &mut janela, &estado_streaming);
            }
            std::thread::sleep(INTERVALO);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_somente_a_transicao_para_streaming() {
        let t = Instant::now();
        let mut d = Detector::default();
        assert_eq!(d.observar("[hora] Setting tray icon to connected", t), None);
        assert_eq!(
            d.observar("[hora] Setting tray icon to streaming", t),
            Some(Acao::AtivarStreaming)
        );
        assert_eq!(
            d.observar("[hora] Setting tray icon to streaming", t),
            None,
            "linhas repetidas não reabrem a saída"
        );
    }

    #[test]
    fn oito_segundos_continuos_confirmam_estabilidade_uma_vez() {
        let t = Instant::now();
        let mut d = Detector::default();
        d.observar("Setting tray icon to streaming", t);
        assert_eq!(d.avaliar(t + ESTABILIDADE - Duration::from_millis(1)), None);
        assert_eq!(
            d.avaliar(t + ESTABILIDADE),
            Some(Acao::EstabilizarStreaming)
        );
        assert_eq!(d.avaliar(t + ESTABILIDADE * 2), None);
    }

    #[test]
    fn sair_de_streaming_cancela_a_confirmacao_pendente() {
        let t = Instant::now();
        let mut d = Detector::default();
        d.observar("Setting tray icon to streaming", t);
        d.observar("Setting tray icon to connected", t + Duration::from_secs(2));
        assert_eq!(d.avaliar(t + ESTABILIDADE), None);

        assert_eq!(
            d.observar(
                "Setting tray icon to streaming",
                t + Duration::from_secs(10)
            ),
            Some(Acao::AtivarStreaming),
            "uma transmissão nova volta a ser detectada"
        );
    }

    #[test]
    fn marcadores_ipc_ativam_e_encerram_a_janela() {
        let t = Instant::now();
        let mut d = Detector::default();
        assert_eq!(
            d.observar("Setting tray icon to golive-start", t),
            Some(Acao::AtivarPlugin)
        );
        assert_eq!(
            d.observar("Setting tray icon to golive-stable", t),
            Some(Acao::EstabilizarPlugin)
        );
    }

    #[test]
    fn duas_acoes_ipc_consecutivas_nao_sao_perdidas() {
        let t = Instant::now();
        let mut d = Detector::default();
        assert_eq!(
            d.observar("Setting tray icon to golive-start", t),
            Some(Acao::AtivarPlugin)
        );
        assert_eq!(
            d.observar("Setting tray icon to golive-start", t),
            Some(Acao::AtivarPlugin)
        );
    }

    #[test]
    fn inicializacao_exige_plugins_conexao_e_silencio() {
        let t = Instant::now();
        let mut estado = Inicializacao::default();
        estado.observar("Preload only: false", t);
        assert!(!estado.pronta(t + SILENCIO_INICIALIZACAO));

        estado.observar("Setting tray icon to connected", t + Duration::from_secs(1));
        assert!(!estado.pronta(t + SILENCIO_INICIALIZACAO));
        assert!(estado.pronta(t + Duration::from_secs(1) + SILENCIO_INICIALIZACAO));

        estado.observar(
            "Setting tray icon to disconnected",
            t + Duration::from_secs(8),
        );
        assert!(!estado.pronta(t + Duration::from_secs(9)));
    }
}
