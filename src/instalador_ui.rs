//! Assistente visual do instalador.
//!
//! A interface nunca executa instalação na thread gráfica. Cada ação longa
//! informa o estágio real ao usuário e a conclusão só aparece depois que
//! Dorion, UrbanVPN, plugin e serviço foram redetectados no disco.

use crate::instalador::{Atualizacao, Componente, Diagnostico, EstadoComponente};
use anyhow::{anyhow, Result};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, RichText, Stroke, Vec2,
};
use std::{
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

const FUNDO: Color32 = Color32::from_rgb(10, 15, 23);
const PAINEL: Color32 = Color32::from_rgb(17, 24, 35);
const CARTAO: Color32 = Color32::from_rgb(22, 31, 44);
const BORDA: Color32 = Color32::from_rgb(48, 61, 78);
const TEXTO: Color32 = Color32::from_rgb(238, 242, 249);
const SECUNDARIO: Color32 = Color32::from_rgb(164, 175, 193);
const ROXO: Color32 = Color32::from_rgb(133, 70, 245);
const ROXO_CLARO: Color32 = Color32::from_rgb(169, 112, 255);
const VERDE: Color32 = Color32::from_rgb(61, 220, 139);
const VERMELHO: Color32 = Color32::from_rgb(248, 102, 113);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tela {
    BoasVindas,
    Requisitos,
    Componentes,
    Instalando,
    Conclusao,
    Ajuda,
}

enum Mensagem {
    Diagnostico(Diagnostico),
    Progresso(Atualizacao),
    Finalizado(std::result::Result<ResultadoFinal, String>),
}

#[derive(Clone, Debug)]
struct ResultadoFinal {
    dorion: bool,
    urban: bool,
    plugin: bool,
    servico: bool,
}

pub fn executar() -> Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Dorion GoLive - Instalador")
        .with_inner_size([790.0, 630.0])
        .with_min_inner_size([790.0, 630.0])
        .with_max_inner_size([790.0, 630.0])
        .with_resizable(false);
    if let Some(icone) = icone_janela() {
        viewport = viewport.with_icon(icone);
    }
    let opcoes = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Dorion GoLive - Instalador",
        opcoes,
        Box::new(|cc| Ok(Box::new(Assistente::novo(cc)))),
    )
    .map_err(|erro| anyhow!(erro.to_string()))
}

struct Assistente {
    tela: Tela,
    tx: Sender<Mensagem>,
    rx: Receiver<Mensagem>,
    diagnostico: Option<Diagnostico>,
    verificando: bool,
    instalando: bool,
    atualizacao: Option<Atualizacao>,
    estados: [EstadoComponente; 4],
    detalhes: [String; 4],
    progressos_componentes: [Option<f32>; 4],
    progresso: f32,
    resultado: Option<ResultadoFinal>,
    erro: Option<String>,
    mensagem_final: Option<String>,
}

impl Assistente {
    fn novo(cc: &eframe::CreationContext<'_>) -> Self {
        configurar_estilo(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        Self {
            tela: Tela::BoasVindas,
            tx,
            rx,
            diagnostico: None,
            verificando: false,
            instalando: false,
            atualizacao: None,
            estados: [EstadoComponente::Aguardando; 4],
            detalhes: std::array::from_fn(|_| "Aguardando…".into()),
            progressos_componentes: [None; 4],
            progresso: 0.0,
            resultado: None,
            erro: None,
            mensagem_final: None,
        }
    }

    fn iniciar_verificacao(&mut self) {
        self.tela = Tela::Requisitos;
        self.verificando = true;
        self.diagnostico = None;
        self.erro = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let diagnostico = crate::instalador::verificar_sistema();
            let _ = tx.send(Mensagem::Diagnostico(diagnostico));
        });
    }

    fn iniciar_instalacao(&mut self) {
        self.tela = Tela::Instalando;
        self.instalando = true;
        self.erro = None;
        self.resultado = None;
        self.progresso = 0.02;
        self.estados = [EstadoComponente::Aguardando; 4];
        self.detalhes = std::array::from_fn(|_| "Aguardando…".into());
        self.progressos_componentes = [None; 4];

        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let progresso_tx = tx.clone();
            let resultado = crate::instalador::garantir_dependencias_com(|atualizacao| {
                crate::log::linha(&format!(
                    "instalador: {:?} {:?}: {}",
                    atualizacao.componente, atualizacao.estado, atualizacao.detalhe
                ));
                let _ = progresso_tx.send(Mensagem::Progresso(atualizacao));
            })
            .and_then(|_| {
                enviar_progresso(
                    &tx,
                    Componente::Plugin,
                    EstadoComponente::Configurando,
                    "Instalando e habilitando o plugin no perfil do Dorion…",
                    0.80,
                );
                enviar_progresso(
                    &tx,
                    Componente::Servico,
                    EstadoComponente::Configurando,
                    "Copiando o serviço e configurando a inicialização automática…",
                    0.86,
                );
                crate::instalar(crate::OpcoesInstalar {
                    reiniciar_discord: true,
                    criar_run_legado: true,
                })?;
                enviar_progresso(
                    &tx,
                    Componente::Plugin,
                    EstadoComponente::Concluido,
                    "Plugin gravado e habilitado no Dorion.",
                    0.94,
                );
                enviar_progresso(
                    &tx,
                    Componente::Servico,
                    EstadoComponente::Concluido,
                    "Serviço instalado e respondendo localmente.",
                    0.98,
                );

                let finalizado = ResultadoFinal {
                    dorion: crate::discord::lancador().is_some(),
                    urban: crate::urban::instalado(),
                    plugin: crate::plugin::ativo(),
                    servico: crate::caminho_instalado().is_file()
                        && crate::porta_ocupada(crate::PORTA_CONTROLE),
                };
                if !finalizado.dorion
                    || !finalizado.urban
                    || !finalizado.plugin
                    || !finalizado.servico
                {
                    anyhow::bail!(
                        "a validação final falhou (Dorion: {}, UrbanVPN: {}, plugin: {}, serviço: {})",
                        sim_nao(finalizado.dorion),
                        sim_nao(finalizado.urban),
                        sim_nao(finalizado.plugin),
                        sim_nao(finalizado.servico)
                    );
                }
                Ok(finalizado)
            });
            let mensagem = resultado.map_err(|erro| format!("{erro:#}"));
            if let Err(erro) = &mensagem {
                crate::log::linha(&format!("instalador falhou: {erro}"));
            }
            let _ = tx.send(Mensagem::Finalizado(mensagem));
        });
    }

    fn receber_mensagens(&mut self) {
        while let Ok(mensagem) = self.rx.try_recv() {
            match mensagem {
                Mensagem::Diagnostico(diagnostico) => {
                    self.verificando = false;
                    self.diagnostico = Some(diagnostico);
                }
                Mensagem::Progresso(atualizacao) => {
                    let indice = indice(atualizacao.componente);
                    self.estados[indice] = atualizacao.estado;
                    self.detalhes[indice] = atualizacao.detalhe.clone();
                    self.progressos_componentes[indice] =
                        progresso_individual(&atualizacao, self.progressos_componentes[indice]);
                    self.progresso = self.progresso.max(atualizacao.progresso);
                    self.atualizacao = Some(atualizacao);
                }
                Mensagem::Finalizado(Ok(resultado)) => {
                    self.instalando = false;
                    self.progresso = 1.0;
                    self.resultado = Some(resultado);
                    self.tela = Tela::Conclusao;
                }
                Mensagem::Finalizado(Err(erro)) => {
                    self.instalando = false;
                    self.erro = Some(erro);
                }
            }
        }
    }

    fn requisitos_prontos(&self) -> bool {
        self.diagnostico.as_ref().is_some_and(|d| {
            d.windows_ok && d.espaco_ok && (d.internet_ok || (d.dorion.is_some() && d.urban))
        })
    }

    fn desenhar_topo(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            desenhar_logo(ui, 34.0);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Dorion GoLive")
                        .size(19.0)
                        .strong()
                        .color(TEXTO),
                );
                ui.label(RichText::new("INSTALADOR").size(10.0).color(ROXO_CLARO));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .size(12.0)
                        .color(SECUNDARIO),
                );
            });
        });
        ui.add_space(12.0);
        ui.separator();
    }

    fn desenhar_passos(&self, ui: &mut egui::Ui) {
        let atual = match self.tela {
            Tela::BoasVindas => 0,
            Tela::Requisitos => 1,
            Tela::Componentes => 2,
            Tela::Instalando => 3,
            Tela::Conclusao | Tela::Ajuda => 4,
        };
        ui.horizontal(|ui| {
            for (i, nome) in [
                "Início",
                "Requisitos",
                "Componentes",
                "Instalação",
                "Concluído",
            ]
            .iter()
            .enumerate()
            {
                if i > 0 {
                    let cor = if i <= atual { ROXO } else { BORDA };
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(42.0, 2.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 1.0, cor);
                }
                let cor = if i <= atual { ROXO } else { BORDA };
                let texto = if i == atual { TEXTO } else { SECUNDARIO };
                ui.vertical(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(23.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 11.0, cor);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        format!("{}", i + 1),
                        FontId::proportional(12.0),
                        Color32::WHITE,
                    );
                    ui.label(RichText::new(*nome).size(10.0).color(texto));
                });
            }
        });
        ui.add_space(16.0);
    }

    fn tela_boas_vindas(&mut self, ui: &mut egui::Ui) {
        ui.add_space(22.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            desenhar_logo(ui, 106.0);
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Dorion GoLive")
                        .size(34.0)
                        .strong()
                        .color(TEXTO),
                );
                ui.label(
                    RichText::new("Lives sem bloqueios. Automático. Simples.")
                        .size(15.0)
                        .color(SECUNDARIO),
                );
            });
        });
        ui.add_space(28.0);
        cartao(ui, |ui| {
            item_beneficio(ui, "D", "Baixa e configura o Dorion quando necessário");
            item_beneficio(ui, "U", "Instala e prepara o aplicativo UrbanVPN");
            item_beneficio(ui, "P", "Configura e habilita o plugin automaticamente");
            item_beneficio(ui, "S", "Cria o serviço em segundo plano e valida tudo");
        });
        ui.add_space(18.0);
        ui.label(
            RichText::new(
                "Os downloads são feitos apenas dos canais oficiais e validados antes da execução.",
            )
            .size(12.0)
            .color(SECUNDARIO),
        );
        rodape_direita(ui, |ui| {
            if botao_primario(ui, "Começar  ›").clicked() {
                self.iniciar_verificacao();
            }
        });
    }

    fn tela_requisitos(&mut self, ui: &mut egui::Ui) {
        titulo(
            ui,
            "Verificando seu sistema",
            "Antes de instalar, confirmamos os requisitos essenciais.",
        );
        ui.add_space(12.0);
        cartao(ui, |ui| {
            if self.verificando {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Executando verificações…").color(TEXTO));
                });
                ui.add_space(170.0);
            } else if let Some(d) = &self.diagnostico {
                linha_requisito(ui, "1", "Windows 10 ou superior", &d.windows, d.windows_ok);
                linha_requisito(
                    ui,
                    "2",
                    "Conexão com a internet",
                    if d.internet_ok {
                        "Canal oficial acessível"
                    } else {
                        "Não confirmada"
                    },
                    d.internet_ok || (d.dorion.is_some() && d.urban),
                );
                linha_requisito(
                    ui,
                    "3",
                    "Permissões de administrador",
                    if d.administrador {
                        "Instalador elevado"
                    } else {
                        "O Windows solicitará via UAC se necessário"
                    },
                    true,
                );
                linha_requisito(
                    ui,
                    "4",
                    "Espaço em disco",
                    &format!("{} GB livres", d.espaco_livre_gib),
                    d.espaco_ok,
                );
            }
        });
        ui.add_space(12.0);
        if !self.verificando && !self.requisitos_prontos() {
            ui.colored_label(VERMELHO, "Corrija os itens marcados antes de continuar.");
        }
        if rodape_duplo(ui, "Voltar", |ui| {
            if ui
                .add_enabled(self.requisitos_prontos(), botao_widget("Avançar  ›"))
                .clicked()
            {
                self.tela = Tela::Componentes;
            }
            if !self.verificando && botao_secundario(ui, "Verificar novamente").clicked() {
                self.iniciar_verificacao();
            }
        }) {
            self.tela = Tela::BoasVindas;
        }
    }

    fn tela_componentes(&mut self, ui: &mut egui::Ui) {
        titulo(
            ui,
            "Componentes necessários",
            "Itens já encontrados serão preservados; somente os ausentes serão baixados.",
        );
        ui.add_space(12.0);
        cartao(ui, |ui| {
            let d = self.diagnostico.as_ref();
            linha_componente(
                ui,
                "D",
                "Dorion",
                "Cliente compatível com plugins",
                d.and_then(|d| d.dorion.as_ref()).is_some(),
            );
            linha_componente(
                ui,
                "U",
                "UrbanVPN",
                "VPN usada somente durante a negociação",
                d.is_some_and(|d| d.urban),
            );
            linha_componente(
                ui,
                "P",
                "Plugin Dorion GoLive",
                "Integração automática com assistir e transmitir",
                d.is_some_and(|d| d.plugin),
            );
            linha_componente(
                ui,
                "S",
                "Serviço Dorion GoLive",
                "Gerencia a VPN em segundo plano",
                d.is_some_and(|d| d.servico),
            );
        });
        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Todos os quatro componentes são necessários para o funcionamento correto.",
            )
            .size(12.0)
            .color(SECUNDARIO),
        );
        if rodape_duplo(ui, "Voltar", |ui| {
            if botao_primario(ui, "Instalar  ›").clicked() {
                self.iniciar_instalacao();
            }
        }) {
            self.tela = Tela::Requisitos;
        }
    }

    fn tela_instalando(&mut self, ui: &mut egui::Ui) {
        if let Some(erro) = self.erro.clone() {
            self.tela_erro_instalacao(ui, &erro);
            return;
        }

        let fase = self
            .atualizacao
            .as_ref()
            .map(|a| match a.estado {
                EstadoComponente::Baixando => "Baixando componentes",
                EstadoComponente::Validando => "Validando downloads",
                EstadoComponente::Instalando => "Instalando componentes",
                EstadoComponente::Configurando => "Configurando o ambiente",
                _ => "Finalizando a instalação",
            })
            .unwrap_or("Preparando a instalação");
        titulo(
            ui,
            fase,
            "Não feche esta janela enquanto a configuração estiver em andamento.",
        );
        ui.add_space(12.0);
        cartao(ui, |ui| {
            for (i, (icone, nome)) in [
                ("D", "Dorion"),
                ("U", "UrbanVPN"),
                ("P", "Plugin Dorion GoLive"),
                ("S", "Serviço Dorion GoLive"),
            ]
            .iter()
            .enumerate()
            {
                linha_progresso(
                    ui,
                    icone,
                    nome,
                    self.estados[i],
                    &self.detalhes[i],
                    self.progressos_componentes[i],
                );
            }
        });
        ui.add_space(16.0);
        let largura_progresso = ui.available_width();
        ui.add(
            egui::ProgressBar::new(self.progresso)
                .desired_width(largura_progresso)
                .fill(ROXO)
                .text(format!("Progresso geral: {:.0}%", self.progresso * 100.0)),
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                RichText::new("Acompanhando o resultado real de cada componente…")
                    .color(SECUNDARIO),
            );
        });
    }

    fn tela_erro_instalacao(&mut self, ui: &mut egui::Ui, erro: &str) {
        titulo(
            ui,
            "A instalação foi interrompida",
            "Nenhuma conclusão é exibida até todos os componentes serem confirmados.",
        );
        ui.add_space(18.0);

        let largura_interna = (ui.available_width() - 36.0).max(0.0);
        egui::Frame::new()
            .fill(Color32::from_rgb(57, 24, 31))
            .stroke(Stroke::new(1.0, VERMELHO))
            .corner_radius(12)
            .inner_margin(18)
            .show(ui, |ui| {
                ui.set_min_width(largura_interna);
                ui.colored_label(
                    VERMELHO,
                    RichText::new("Não foi possível concluir esta etapa")
                        .size(15.0)
                        .strong(),
                );
                ui.add_space(5.0);
                egui::ScrollArea::vertical()
                    .max_height(125.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(erro).size(12.0).color(TEXTO)).wrap(),
                        );
                    });
            });

        ui.add_space(12.0);
        ui.label(
            RichText::new(format!(
                "Progresso preservado em {:.0}%. Você pode corrigir a causa e tentar novamente.",
                self.progresso * 100.0
            ))
            .size(12.0)
            .color(SECUNDARIO),
        );

        if rodape_duplo(ui, "Voltar aos componentes", |ui| {
            if botao_primario(ui, "Tentar novamente").clicked() {
                self.iniciar_instalacao();
            }
        }) {
            self.erro = None;
            self.tela = Tela::Componentes;
        }
    }

    fn tela_conclusao(&mut self, ui: &mut egui::Ui) {
        titulo(
            ui,
            "Instalação concluída!",
            "Todos os componentes foram redetectados e validados.",
        );
        ui.add_space(12.0);
        cartao(ui, |ui| {
            if let Some(r) = &self.resultado {
                linha_resultado(ui, "Dorion instalado", r.dorion);
                linha_resultado(ui, "UrbanVPN instalado", r.urban);
                linha_resultado(ui, "Plugin configurado e habilitado", r.plugin);
                linha_resultado(ui, "Serviço em execução", r.servico);
            }
            linha_resultado(ui, "Tudo pronto para usar", true);
        });
        if let Some(mensagem) = &self.mensagem_final {
            ui.add_space(10.0);
            ui.colored_label(VERDE, mensagem);
        }
        rodape_direita(ui, |ui| {
            if botao_secundario(ui, "Concluir").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if botao_secundario(ui, "Ver dicas rápidas").clicked() {
                self.tela = Tela::Ajuda;
            }
            if botao_primario(ui, "Iniciar Dorion agora").clicked() {
                self.mensagem_final = Some(match crate::discord::reiniciar() {
                    Ok(true) => "Dorion iniciado com o plugin configurado.".into(),
                    Ok(false) => "O Dorion não foi encontrado na validação de abertura.".into(),
                    Err(erro) => format!("Não foi possível abrir o Dorion: {erro}"),
                });
            }
        });
    }

    fn tela_ajuda(&mut self, ui: &mut egui::Ui) {
        titulo(
            ui,
            "Dicas rápidas",
            "O Dorion GoLive funciona automaticamente em segundo plano.",
        );
        ui.add_space(12.0);
        cartao(ui, |ui| {
            dica(ui, "L", "Iniciar uma live", "Escolha a tela normalmente. A VPN conecta antes de o Dorion liberar a transmissão.");
            dica(
                ui,
                "A",
                "Assistir lives",
                "O botão de assistir e o olho contextual aguardam a confirmação real da VPN.",
            );
            dica(ui, "S", "Funcionamento automático", "Depois que o vídeo estabiliza, a VPN é desligada e o serviço continua monitorando.");
            dica(
                ui,
                "?",
                "Precisa de ajuda?",
                "Consulte o README e a solução de problemas no repositório do projeto.",
            );
        });
        if rodape_duplo(ui, "Voltar", |ui| {
            if botao_primario(ui, "Concluir").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }) {
            self.tela = Tela::Conclusao;
        }
    }
}

impl eframe::App for Assistente {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let tamanho_disponivel = ui.available_size();
        self.receber_mensagens();
        ctx.request_repaint_after(Duration::from_millis(100));
        if self.instalando && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        egui::Frame::new()
            .fill(FUNDO)
            .inner_margin(egui::Margin::symmetric(26, 20))
            .show(ui, |ui| {
                ui.set_min_size(Vec2::new(
                    (tamanho_disponivel.x - 52.0).max(0.0),
                    (tamanho_disponivel.y - 40.0).max(0.0),
                ));
                self.desenhar_topo(ui);
                self.desenhar_passos(ui);
                match self.tela {
                    Tela::BoasVindas => self.tela_boas_vindas(ui),
                    Tela::Requisitos => self.tela_requisitos(ui),
                    Tela::Componentes => self.tela_componentes(ui),
                    Tela::Instalando => self.tela_instalando(ui),
                    Tela::Conclusao => self.tela_conclusao(ui),
                    Tela::Ajuda => self.tela_ajuda(ui),
                }
            });
    }
}

fn configurar_estilo(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut estilo = (*ctx.style_of(egui::Theme::Dark)).clone();
    estilo.visuals.dark_mode = true;
    estilo.visuals.panel_fill = FUNDO;
    estilo.visuals.window_fill = PAINEL;
    estilo.visuals.faint_bg_color = CARTAO;
    estilo.visuals.widgets.inactive.bg_fill = CARTAO;
    estilo.visuals.widgets.inactive.weak_bg_fill = CARTAO;
    estilo.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDA);
    estilo.visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 49, 65);
    estilo.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ROXO_CLARO);
    estilo.visuals.widgets.active.bg_fill = ROXO;
    estilo.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ROXO_CLARO);
    estilo.visuals.widgets.noninteractive.bg_fill = PAINEL;
    estilo.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDA);
    estilo.visuals.selection.bg_fill = ROXO;
    estilo.spacing.item_spacing = Vec2::new(10.0, 8.0);
    estilo.spacing.button_padding = Vec2::new(20.0, 11.0);
    ctx.set_style_of(egui::Theme::Dark, estilo);
}

fn icone_janela() -> Option<egui::IconData> {
    let imagem = image::load_from_memory_with_format(
        include_bytes!("../assets/icon.png"),
        image::ImageFormat::Png,
    )
    .ok()?
    .into_rgba8();
    let (width, height) = imagem.dimensions();
    Some(egui::IconData {
        rgba: imagem.into_raw(),
        width,
        height,
    })
}

fn titulo(ui: &mut egui::Ui, principal: &str, subtitulo: &str) {
    ui.label(RichText::new(principal).size(25.0).strong().color(TEXTO));
    ui.label(RichText::new(subtitulo).size(13.0).color(SECUNDARIO));
}

fn cartao(ui: &mut egui::Ui, conteudo: impl FnOnce(&mut egui::Ui)) {
    let largura_interna = (ui.available_width() - 36.0).max(0.0);
    egui::Frame::new()
        .fill(CARTAO)
        .stroke(Stroke::new(1.0, BORDA))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(18)
        .show(ui, |ui| {
            ui.set_min_width(largura_interna);
            conteudo(ui);
        });
}

fn desenhar_logo(ui: &mut egui::Ui, tamanho: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(tamanho), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect.shrink(tamanho * 0.08), tamanho * 0.22, ROXO);
    let interno = rect.shrink(tamanho * 0.20);
    painter.rect_filled(interno, tamanho * 0.16, Color32::from_rgb(24, 29, 46));
    let centro = rect.center();
    painter.circle_filled(
        egui::pos2(centro.x - tamanho * 0.16, centro.y),
        tamanho * 0.13,
        Color32::WHITE,
    );
    painter.circle_filled(
        egui::pos2(centro.x + tamanho * 0.16, centro.y),
        tamanho * 0.13,
        Color32::WHITE,
    );
    painter.line_segment(
        [
            egui::pos2(centro.x - tamanho * 0.23, centro.y),
            egui::pos2(centro.x - tamanho * 0.09, centro.y),
        ],
        Stroke::new((tamanho * 0.045).max(2.0), ROXO),
    );
    painter.line_segment(
        [
            egui::pos2(centro.x - tamanho * 0.16, centro.y - tamanho * 0.07),
            egui::pos2(centro.x - tamanho * 0.16, centro.y + tamanho * 0.07),
        ],
        Stroke::new((tamanho * 0.045).max(2.0), ROXO),
    );
    painter.circle_filled(
        egui::pos2(centro.x + tamanho * 0.12, centro.y - tamanho * 0.04),
        (tamanho * 0.025).max(1.5),
        ROXO,
    );
    painter.circle_filled(
        egui::pos2(centro.x + tamanho * 0.20, centro.y + tamanho * 0.04),
        (tamanho * 0.025).max(1.5),
        ROXO,
    );
}

fn item_beneficio(ui: &mut egui::Ui, icone: &str, texto: &str) {
    ui.horizontal(|ui| {
        desenhar_badge(ui, icone, ROXO_CLARO);
        ui.add_space(3.0);
        ui.label(RichText::new(texto).size(15.0).color(TEXTO));
    });
    ui.add_space(5.0);
}

fn linha_requisito(ui: &mut egui::Ui, icone: &str, nome: &str, detalhe: &str, ok: bool) {
    ui.horizontal(|ui| {
        desenhar_badge(ui, icone, if ok { ROXO_CLARO } else { VERMELHO });
        ui.add_space(3.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(nome).size(14.0).strong().color(TEXTO));
            ui.label(RichText::new(detalhe).size(11.0).color(SECUNDARIO));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(if ok { "OK" } else { "Verificar" })
                    .strong()
                    .color(if ok { VERDE } else { VERMELHO }),
            );
        });
    });
    ui.add_space(7.0);
}

fn linha_componente(ui: &mut egui::Ui, icone: &str, nome: &str, detalhe: &str, instalado: bool) {
    ui.horizontal(|ui| {
        desenhar_badge(ui, icone, ROXO_CLARO);
        ui.add_space(3.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(nome).size(14.0).strong().color(TEXTO));
            ui.label(RichText::new(detalhe).size(11.0).color(SECUNDARIO));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(if instalado {
                    "Já instalado"
                } else {
                    "Será instalado"
                })
                .size(12.0)
                .color(if instalado { VERDE } else { ROXO_CLARO }),
            );
        });
    });
    ui.add_space(8.0);
}

fn linha_progresso(
    ui: &mut egui::Ui,
    icone: &str,
    nome: &str,
    estado: EstadoComponente,
    detalhe: &str,
    progresso: Option<f32>,
) {
    ui.horizontal(|ui| {
        desenhar_badge(ui, icone, ROXO_CLARO);
        ui.add_space(3.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(nome).size(14.0).strong().color(TEXTO));
            ui.label(RichText::new(detalhe).size(11.0).color(SECUNDARIO));
            if estado == EstadoComponente::Baixando {
                if let Some(valor) = progresso {
                    ui.add(
                        egui::ProgressBar::new(valor)
                            .desired_width(330.0)
                            .desired_height(6.0)
                            .fill(ROXO),
                    );
                } else {
                    barra_indeterminada(ui, 330.0);
                }
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match estado {
            EstadoComponente::Concluido => {
                ui.label(RichText::new("OK").size(13.0).strong().color(VERDE));
            }
            EstadoComponente::Aguardando => {
                ui.label(RichText::new("--").size(13.0).color(SECUNDARIO));
            }
            EstadoComponente::Baixando if progresso.is_some() => {
                ui.label(
                    RichText::new(format!("{:.0}%", progresso.unwrap_or_default() * 100.0))
                        .size(12.0)
                        .strong()
                        .color(ROXO_CLARO),
                );
            }
            _ => {
                ui.spinner();
            }
        });
    });
    ui.add_space(8.0);
}

fn linha_resultado(ui: &mut egui::Ui, texto: &str, ok: bool) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(if ok { "OK" } else { "ERRO" })
                .size(12.0)
                .color(if ok { VERDE } else { VERMELHO }),
        );
        ui.label(RichText::new(texto).size(14.0).color(TEXTO));
    });
    ui.add_space(5.0);
}

fn dica(ui: &mut egui::Ui, icone: &str, nome: &str, detalhe: &str) {
    ui.horizontal(|ui| {
        desenhar_badge(ui, icone, ROXO_CLARO);
        ui.add_space(3.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(nome).size(14.0).strong().color(TEXTO));
            ui.label(RichText::new(detalhe).size(12.0).color(SECUNDARIO));
        });
    });
    ui.add_space(10.0);
}

fn botao_widget(texto: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(texto).strong().color(Color32::WHITE))
        .fill(ROXO)
        .stroke(Stroke::new(1.0, ROXO_CLARO))
        .corner_radius(9)
        .min_size(Vec2::new(136.0, 42.0))
}

fn botao_primario(ui: &mut egui::Ui, texto: &str) -> egui::Response {
    ui.add(botao_widget(texto))
}

fn botao_secundario(ui: &mut egui::Ui, texto: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(texto).strong().color(TEXTO))
            .fill(PAINEL)
            .stroke(Stroke::new(1.0, BORDA))
            .corner_radius(9)
            .min_size(Vec2::new(126.0, 42.0)),
    )
}

fn rodape_direita(ui: &mut egui::Ui, conteudo: impl FnOnce(&mut egui::Ui)) {
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), conteudo);
    });
}

fn rodape_duplo(ui: &mut egui::Ui, voltar: &str, direita: impl FnOnce(&mut egui::Ui)) -> bool {
    let mut clicou_voltar = false;
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if botao_secundario(ui, voltar).clicked() {
                clicou_voltar = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), direita);
        });
        ui.add_space(8.0);
        ui.separator();
    });
    clicou_voltar
}

fn desenhar_badge(ui: &mut egui::Ui, texto: &str, cor: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 8.0, Color32::from_rgb(39, 31, 61));
    ui.painter()
        .rect_stroke(rect, 8.0, Stroke::new(1.0, cor), egui::StrokeKind::Inside);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        texto,
        FontId::proportional(13.0),
        cor,
    );
}

fn barra_indeterminada(ui: &mut egui::Ui, largura: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(largura, 6.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, PAINEL);

    let tempo = ui.input(|entrada| entrada.time) as f32;
    let ciclo = (tempo * 0.55).fract();
    let largura_segmento = rect.width() * 0.28;
    let inicio = rect.left() - largura_segmento + ciclo * (rect.width() + largura_segmento * 2.0);
    let fim = inicio + largura_segmento;
    let inicio_visivel = inicio.max(rect.left());
    let fim_visivel = fim.min(rect.right());
    if fim_visivel > inicio_visivel {
        let segmento = egui::Rect::from_min_max(
            egui::pos2(inicio_visivel, rect.top()),
            egui::pos2(fim_visivel, rect.bottom()),
        );
        ui.painter().rect_filled(segmento, 3.0, ROXO);
    }
}

fn enviar_progresso(
    tx: &Sender<Mensagem>,
    componente: Componente,
    estado: EstadoComponente,
    detalhe: &str,
    progresso: f32,
) {
    let _ = tx.send(Mensagem::Progresso(Atualizacao {
        componente,
        estado,
        detalhe: detalhe.into(),
        progresso,
    }));
}

fn indice(componente: Componente) -> usize {
    match componente {
        Componente::Dorion => 0,
        Componente::UrbanVpn => 1,
        Componente::Plugin => 2,
        Componente::Servico => 3,
    }
}

fn progresso_individual(atualizacao: &Atualizacao, anterior: Option<f32>) -> Option<f32> {
    match (atualizacao.componente, atualizacao.estado) {
        (Componente::Dorion, EstadoComponente::Baixando) => {
            Some(((atualizacao.progresso - 0.10) / 0.16).clamp(0.0, 1.0))
        }
        (_, EstadoComponente::Concluido) => Some(1.0),
        (_, EstadoComponente::Validando | EstadoComponente::Instalando) => {
            Some(anterior.unwrap_or(1.0))
        }
        _ => anterior,
    }
}

fn sim_nao(valor: bool) -> &'static str {
    if valor {
        "sim"
    } else {
        "não"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converte_o_download_real_do_dorion_em_progresso_individual() {
        let atualizacao = Atualizacao {
            componente: Componente::Dorion,
            estado: EstadoComponente::Baixando,
            detalhe: String::new(),
            progresso: 0.18,
        };

        let progresso = progresso_individual(&atualizacao, None).expect("progresso conhecido");
        assert!((progresso - 0.5).abs() < f32::EPSILON * 2.0);
    }

    #[test]
    fn conclusao_marca_o_componente_como_completo() {
        let atualizacao = Atualizacao {
            componente: Componente::UrbanVpn,
            estado: EstadoComponente::Concluido,
            detalhe: String::new(),
            progresso: 0.74,
        };

        assert_eq!(progresso_individual(&atualizacao, None), Some(1.0));
    }
}
