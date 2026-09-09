//! Controle local usado pelo plugin para aguardar a confirmação real da VPN.

use crate::{dorion_eventos::EstadoStreaming, urban::UrbanVpn};
use anyhow::{Context, Result};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    time::Duration,
};

const LIMITE_PEDIDO: usize = 8192;

#[derive(Debug, PartialEq, Eq)]
enum Rota {
    Iniciar,
    Estabilizar(u64),
    EstadoStreaming(u64),
    Registro {
        evento: String,
        detalhe: String,
        instante_ms: String,
    },
    Opcoes,
    Desconhecida,
}

struct Controle {
    vpn: Arc<UrbanVpn>,
    operacao: Mutex<()>,
    sessao_plugin: Mutex<Option<u64>>,
    streaming: Arc<EstadoStreaming>,
}

pub fn servir(porta: u16, vpn: Arc<UrbanVpn>, streaming: Arc<EstadoStreaming>) -> Result<()> {
    let endereco = format!("127.0.0.1:{porta}");
    let listener = TcpListener::bind(&endereco)
        .with_context(|| format!("abrindo controle local em {endereco}"))?;
    let controle = Arc::new(Controle {
        vpn,
        operacao: Mutex::new(()),
        sessao_plugin: Mutex::new(None),
        streaming,
    });
    crate::log::linha(&format!("controle local pronto em http://{endereco}"));

    for conexao in listener.incoming() {
        match conexao {
            Ok(stream) => {
                let controle = controle.clone();
                std::thread::spawn(move || {
                    if let Err(e) = atender(stream, &controle) {
                        crate::log::linha(&format!("pedido do plugin falhou: {e}"));
                    }
                });
            }
            Err(e) => crate::log::linha(&format!("controle local falhou: {e}")),
        }
    }
    Ok(())
}

fn atender(mut stream: TcpStream, controle: &Controle) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;

    let mut dados = [0u8; LIMITE_PEDIDO];
    let quantidade = stream.read(&mut dados)?;
    let pedido = String::from_utf8_lossy(&dados[..quantidade]);

    match interpretar(&pedido) {
        Rota::Iniciar => {
            crate::log::linha(
                "plugin pediu a VPN; ação do Discord deve continuar bloqueada até a resposta",
            );
            let _operacao = controle.operacao.lock().unwrap_or_else(|e| e.into_inner());
            match controle.vpn.ativar("ação confirmada pelo plugin") {
                Ok(sessao) => {
                    *controle
                        .sessao_plugin
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(sessao);
                    responder(
                        &mut stream,
                        200,
                        &[
                            ("X-Dorion-GoLive-Ready", "1".into()),
                            ("X-Dorion-GoLive-Session", sessao.to_string()),
                            (
                                "X-Dorion-GoLive-Streaming-Generation",
                                controle.streaming.confirmacoes().to_string(),
                            ),
                        ],
                    )?;
                }
                Err(e) => {
                    crate::log::linha(&format!("UrbanVPN não ficou pronta para o plugin: {e}"));
                    responder(&mut stream, 503, &[("X-Dorion-GoLive-Ready", "0".into())])?;
                }
            }
        }
        Rota::Registro {
            evento,
            detalhe,
            instante_ms,
        } => {
            crate::log::linha(&format!(
                "plugin [{instante_ms} ms] {evento}{}",
                if detalhe.is_empty() {
                    String::new()
                } else {
                    format!(": {detalhe}")
                }
            ));
            responder(&mut stream, 204, &[])?;
        }
        Rota::Estabilizar(sessao) => {
            let _operacao = controle.operacao.lock().unwrap_or_else(|e| e.into_inner());
            let corresponde = {
                let mut atual = controle
                    .sessao_plugin
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if *atual == Some(sessao) {
                    *atual = None;
                    true
                } else {
                    false
                }
            };

            if !corresponde {
                responder(&mut stream, 409, &[])?;
            } else {
                // O detector nativo pode ter renovado a geração ao observar o
                // mesmo streaming. Desligar a geração atual preserva a regra
                // contra uma ação realmente mais nova, cuja sessão já teria
                // substituído `sessao_plugin` acima.
                controle.vpn.desativar_atual("fim confirmado pelo plugin")?;
                responder(&mut stream, 200, &[])?;
            }
        }
        Rota::EstadoStreaming(anterior) => {
            let atual = controle.streaming.confirmacoes();
            responder(
                &mut stream,
                200,
                &[
                    (
                        "X-Dorion-GoLive-Streaming-Ready",
                        if atual > anterior { "1" } else { "0" }.into(),
                    ),
                    ("X-Dorion-GoLive-Streaming-Generation", atual.to_string()),
                ],
            )?;
        }
        Rota::Opcoes => responder(&mut stream, 204, &[])?,
        Rota::Desconhecida => responder(&mut stream, 404, &[])?,
    }
    Ok(())
}

fn interpretar(pedido: &str) -> Rota {
    let primeira = pedido.lines().next().unwrap_or_default();
    let mut partes = primeira.split_whitespace();
    let metodo = partes.next().unwrap_or_default();
    let alvo = partes.next().unwrap_or_default();

    if metodo == "OPTIONS" {
        return Rota::Opcoes;
    }
    if metodo != "POST" {
        return Rota::Desconhecida;
    }
    if alvo == "/golive/start" {
        return Rota::Iniciar;
    }
    if let Some(query) = alvo.strip_prefix("/golive/log?") {
        let obter = |nome: &str| {
            query
                .split('&')
                .filter_map(|parte| parte.split_once('='))
                .find_map(|(chave, valor)| (chave == nome).then(|| decodificar(valor)))
                .unwrap_or_default()
        };
        return Rota::Registro {
            evento: limpar_registro(&obter("event"), 80),
            detalhe: limpar_registro(&obter("detail"), 400),
            instante_ms: limpar_registro(&obter("t"), 20),
        };
    }
    if let Some(query) = alvo.strip_prefix("/golive/stable?session=") {
        if let Ok(sessao) = query.parse::<u64>() {
            if sessao > 0 {
                return Rota::Estabilizar(sessao);
            }
        }
    }
    if let Some(query) = alvo.strip_prefix("/golive/stream-state?after=") {
        if let Ok(geracao) = query.parse::<u64>() {
            return Rota::EstadoStreaming(geracao);
        }
    }
    Rota::Desconhecida
}

fn decodificar(valor: &str) -> String {
    let bytes = valor.as_bytes();
    let mut saida = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hexadecimal = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                b'A'..=b'F' => Some(b - b'A' + 10),
                _ => None,
            };
            if let (Some(a), Some(b)) = (hexadecimal(bytes[i + 1]), hexadecimal(bytes[i + 2])) {
                saida.push(a * 16 + b);
                i += 3;
                continue;
            }
        }
        saida.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&saida).into_owned()
}

fn limpar_registro(valor: &str, limite: usize) -> String {
    valor
        .chars()
        .filter(|c| !c.is_control())
        .take(limite)
        .collect()
}

fn responder(stream: &mut TcpStream, codigo: u16, extras: &[(&str, String)]) -> Result<()> {
    let texto = match codigo {
        200 => "OK",
        204 => "No Content",
        409 => "Conflict",
        503 => "Service Unavailable",
        _ => "Not Found",
    };
    let mut resposta = format!(
        "HTTP/1.1 {codigo} {texto}\r\n\
         Content-Length: 0\r\n\
         Connection: close\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: POST, OPTIONS\r\n\
         Access-Control-Expose-Headers: X-Dorion-GoLive-Ready, X-Dorion-GoLive-Session, X-Dorion-GoLive-Streaming-Ready, X-Dorion-GoLive-Streaming-Generation\r\n"
    );
    for (nome, valor) in extras {
        resposta.push_str(&format!("{nome}: {valor}\r\n"));
    }
    resposta.push_str("\r\n");
    stream.write_all(resposta.as_bytes())?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconhece_inicio_e_sessao_valida() {
        assert_eq!(
            interpretar("POST /golive/start HTTP/1.1\r\n"),
            Rota::Iniciar
        );
        assert_eq!(
            interpretar("POST /golive/stream-state?after=7 HTTP/1.1\r\n"),
            Rota::EstadoStreaming(7)
        );
        assert_eq!(
            interpretar("POST /golive/stable?session=42 HTTP/1.1\r\n"),
            Rota::Estabilizar(42)
        );
        assert_eq!(
            interpretar(
                "POST /golive/log?event=clique_bloqueado&detail=Assistir%20a%20live&t=125 HTTP/1.1\r\n"
            ),
            Rota::Registro {
                evento: "clique_bloqueado".into(),
                detalhe: "Assistir a live".into(),
                instante_ms: "125".into(),
            }
        );
    }

    #[test]
    fn rejeita_metodo_e_sessao_invalidos() {
        assert_eq!(
            interpretar("GET /golive/start HTTP/1.1\r\n"),
            Rota::Desconhecida
        );
        assert_eq!(
            interpretar("POST /golive/stable?session=0 HTTP/1.1\r\n"),
            Rota::Desconhecida
        );
    }
}
