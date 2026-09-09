/* Dorion GoLive — detector local de iniciar/assistir transmissão.
 * Não contém servidor externo, token, telemetria nem coordenadas de tela.
 */
(() => {
  if (window.__DORION_GOLIVE_PLUGIN__) return;
  window.__DORION_GOLIVE_PLUGIN__ = true;

  const CONTROLE = "http://127.0.0.1:9352/golive";
  const TIMEOUT_CONTROLE_MS = 5000;
  const liberados = new WeakSet();
  const agendados = new WeakSet();
  let preparando = null;
  let livePendente = false;
  let compartilhamentoPendente = false;
  let timerDireto = 0;
  let cliqueLiberadoEm = 0;
  let vpnPreparadaEm = 0;
  let preparacaoId = 0;
  let videosAtivosAntes = 0;
  let fontesAntes = new Set();
  let adicionandoOutraLive = false;
  let timerMonitor = 0;
  let amostrasVideo = new WeakMap();
  let sessaoVpn = 0;
  let geracaoStreamingAntes = 0;
  let transmissaoPendente = false;
  let repeticaoEmAndamento = false;
  const inicioLog = Date.now();
  let sequenciaLog = 0;

  const semAcento = texto => String(texto || "")
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/\s+/g, " ")
    .trim();

  function rotuloProprio(elemento) {
    // Nunca lê pais: um painel inteiro pode conter "AO VIVO" ou "Assista à
    // transmissão" e transformar qualquer clique no espaço vazio em ação.
    const partes = [
      elemento.getAttribute?.("aria-label"),
      elemento.getAttribute?.("title"),
      elemento.innerText,
      elemento.textContent,
      elemento.getAttribute?.("data-list-item-id"),
    ];
    const proprio = partes.map(semAcento).find(Boolean) || "";
    return proprio.slice(0, 240);
  }

  function nomeDeAssistir(nome) {
    return /\b(assista|assistir|ver|entrar) (a|na|em)? ?(transmissao|transmissoes|live|lives|ao vivo)\b|\b(watch|join) (the )?(stream|streams|live)\b/
      .test(nome);
  }

  function nomeDeAdicionarLive(nome) {
    // O botão com o olho aparece ao passar o mouse sobre outra pessoa que
    // transmite enquanto uma live já está aberta. A tradução/rótulo varia
    // entre versões do Discord, por isso aceitamos apenas intenções explícitas.
    return /\b(adicionar|incluir)( (uma|a|na))?( outra)? (transmissao|live)\b|\b(adicionar|incluir) (a|na) (visualizacao|grade)\b|\b(add|include)( another)? (stream|live)( to (the )?(view|grid))?\b|\bwatch another (stream|live)\b/
      .test(nome);
  }

  function nomeDeCompartilhar(nome) {
    // Somente a confirmação final. "Compartilhar sua tela" abre a escolha
    // de fonte e deve passar sem VPN; "Transmitir" é retido depois da escolha.
    return /^(stream|transmitir)$|\b(iniciar transmissao|comecar transmissao|go live|start streaming|start stream)\b/
      .test(nome);
  }

  function eOlhoAoLadoDeAssistir(botao) {
    // Em algumas compilações o olho não tem aria-label. Na disposição vista
    // no Dorion ele é o botão imediatamente à direita do botão verde. Essa
    // relação de irmãos é estreita e não consulta o texto de painéis/pais.
    const anterior = botao.previousElementSibling;
    return anterior instanceof Element &&
      anterior.matches("button, [role='button']") &&
      nomeDeAssistir(rotuloProprio(anterior));
  }

  function eBotaoAdicionarLive(botao) {
    return nomeDeAdicionarLive(rotuloProprio(botao)) ||
      eOlhoAoLadoDeAssistir(botao);
  }

  function acaoDeLive(alvo) {
    if (!(alvo instanceof Element)) return null;
    // SVGs e spans internos chegam até o botão; áreas genéricas com aria-label
    // ou data-list-item-id não são tratadas como clicáveis por conta própria.
    const botao = alvo.closest("button, [role='button']");
    if (!botao) return null;
    const nome = rotuloProprio(botao);
    const transmite = nomeDeCompartilhar(nome) || /\biniciar transmissao\b/.test(nome);
    const assiste = nomeDeAssistir(nome) || nomeDeAdicionarLive(nome) ||
      eOlhoAoLadoDeAssistir(botao);
    return transmite || assiste ? botao : null;
  }

  function eCompartilhamento(botao) {
    const nome = rotuloProprio(botao);
    return nomeDeCompartilhar(nome) || /\biniciar transmissao\b/.test(nome);
  }

  function descreverControle(botao) {
    const valor = nome => semAcento(botao.getAttribute?.(nome));
    return [
      `aria=${valor("aria-label") || "-"}`,
      `title=${valor("title") || "-"}`,
      `text=${semAcento(botao.textContent).slice(0, 100) || "-"}`,
      `data=${valor("data-list-item-id") || "-"}`,
    ].join("; ");
  }

  function eCancelamento(alvo) {
    if (!(alvo instanceof Element)) return false;
    const botao = alvo.closest("button, [role='button']");
    if (!botao) return false;
    return /^(cancelar|fechar|cancel|close)( compartilhamento| share)?$/
      .test(rotuloProprio(botao));
  }

  const esperar = ms => new Promise(resolve => setTimeout(resolve, ms));

  function assinaturaDaFonte(video) {
    const stream = video.srcObject;
    if (stream?.getTracks) {
      const trilhas = stream.getTracks()
        .map(trilha => `${trilha.kind}:${trilha.id}`)
        .sort()
        .join("|");
      if (trilhas) return `stream:${trilhas}`;
    }
    const endereco = video.currentSrc || video.src;
    return endereco ? `src:${endereco}` : "";
  }

  function estadoDoVideo(video) {
    let quadros = Number(video.webkitDecodedFrameCount) || 0;
    try {
      quadros = Number(video.getVideoPlaybackQuality?.().totalVideoFrames) || quadros;
    } catch (_) {}
    return {
      quadros,
      tempo: Number.isFinite(Number(video.currentTime)) ? Number(video.currentTime) : 0,
    };
  }

  function videoEstaRenderizando(video) {
    return video.readyState >= 2 && !video.paused && !video.ended &&
      video.videoWidth > 0 && video.videoHeight > 0;
  }

  async function fetchNativo() {
    // O Dorion 6.13 não expõe `__TAURI__.http` aos plugins. Ele instala um
    // proxy no `window.fetch`: URLs externas (incluindo 127.0.0.1) seguem
    // pelo tauri-plugin-http, enquanto as URLs do Discord usam o WebView.
    // `nativeFetch` nasce antes de o proxy ficar pronto; esperar as funções
    // divergirem evita a corrida logo após a abertura do Dorion.
    const limite = Date.now() + TIMEOUT_CONTROLE_MS;
    do {
      if (typeof window.fetch === "function" &&
          typeof window.nativeFetch === "function" &&
          window.fetch !== window.nativeFetch) {
        return window.fetch.bind(window);
      }
      await esperar(50);
    } while (Date.now() < limite);
    throw new Error("proxy HTTP local do Dorion indisponível");
  }

  async function chamarServico(caminho) {
    const fetch = await fetchNativo();
    const resposta = await fetch(`${CONTROLE}${caminho}`, {
      method: "POST",
      connectTimeout: TIMEOUT_CONTROLE_MS,
    });
    if (!resposta.ok) {
      throw new Error(`serviço local respondeu HTTP ${resposta.status}`);
    }
    return resposta;
  }

  async function registrar(evento, detalhe = "") {
    const numero = ++sequenciaLog;
    const instante = Date.now() - inicioLog;
    const query = [
      `event=${encodeURIComponent(`${numero}-${evento}`)}`,
      `detail=${encodeURIComponent(String(detalhe).slice(0, 400))}`,
      `t=${instante}`,
    ].join("&");
    try {
      const fetch = await fetchNativo();
      await fetch(`${CONTROLE}/log?${query}`, {
        method: "POST",
        connectTimeout: TIMEOUT_CONTROLE_MS,
      });
    } catch (erro) {
      console.warn("[Dorion GoLive] falha ao registrar", evento, erro);
    }
  }

  async function encerrarSessao(token) {
    if (!token) return;
    await chamarServico(`/stable?session=${encodeURIComponent(token)}`);
  }

  function aviso(texto) {
    let caixa = document.getElementById("dorion-golive-aviso");
    if (!caixa) {
      caixa = document.createElement("div");
      caixa.id = "dorion-golive-aviso";
      Object.assign(caixa.style, {
        position: "fixed", right: "18px", bottom: "18px", zIndex: "2147483647",
        padding: "10px 14px", borderRadius: "8px", color: "white",
        background: "#1e1f22", boxShadow: "0 4px 20px #0008",
        font: "14px/1.35 system-ui, sans-serif", maxWidth: "330px"
      });
      document.body.appendChild(caixa);
    }
    caixa.textContent = texto;
    return caixa;
  }

  async function preparar() {
    if (preparando) return preparando;
    if (livePendente && sessaoVpn > 0) {
      await registrar("vpn_ja_confirmada_reutilizada", `sessao=${sessaoVpn}`);
      return true;
    }
    // Uma tentativa nova invalida o prazo da anterior. Sem isso, dois cliques
    // próximos podiam desligar a VPN um instante antes do segundo clique.
    clearTimeout(timerDireto);
    clearTimeout(timerMonitor);
    timerDireto = 0;
    timerMonitor = 0;
    livePendente = true;
    // Ao adicionar uma segunda live, a primeira volta a emitir `playing`
    // durante a troca de rota. Ela não pode confirmar uma ação que ainda nem
    // foi repetida. Guardamos os vídeos/streams que já existiam para exigir
    // uma mídia realmente nova depois do clique liberado.
    cliqueLiberadoEm = 0;
    const videosAtuais = [...document.querySelectorAll("video")]
      .filter(videoEstaRenderizando);
    videosAtivosAntes = videosAtuais.length;
    fontesAntes = new Set(videosAtuais.map(assinaturaDaFonte).filter(Boolean));
    amostrasVideo = new WeakMap();
    const minhaPreparacao = ++preparacaoId;
    aviso("Ligando e verificando a VPN temporária…");
    registrar("preparacao_iniciada", "ação permanece bloqueada");
    const inicioPreparacao = Date.now();
    preparando = chamarServico("/start")
      .then(async resposta => {
        const token = Number(resposta.headers?.get?.("x-dorion-golive-session"));
        const pronta = resposta.headers?.get?.("x-dorion-golive-ready") === "1";
        const geracaoStreaming = Number(
          resposta.headers?.get?.("x-dorion-golive-streaming-generation")
        );
        if (!pronta || !Number.isSafeInteger(token) || token <= 0) {
          throw new Error("serviço não confirmou a sessão da VPN");
        }
        // Se o usuário cancelou enquanto o serviço conectava, a confirmação
        // tardia serve apenas para desfazer a conexão recém-criada.
        if (minhaPreparacao !== preparacaoId || !livePendente) {
          await encerrarSessao(token).catch(() => {});
          return false;
        }
        sessaoVpn = token;
        geracaoStreamingAntes = Number.isSafeInteger(geracaoStreaming) && geracaoStreaming >= 0
          ? geracaoStreaming
          : 0;
        vpnPreparadaEm = Date.now();
        await registrar(
          "vpn_confirmada",
          `sessao=${token}; espera=${Date.now() - inicioPreparacao}ms`
        );
        return true;
      })
      .catch(erro => {
        livePendente = false;
        aviso("Não foi possível confirmar a conexão da VPN temporária.");
        registrar("vpn_nao_confirmada", erro?.message || erro);
        console.warn("[Dorion GoLive] não foi possível preparar", erro);
        return false;
      })
      .finally(() => { preparando = null; });
    const pronta = await preparando;
    if (pronta) aviso("VPN confirmada; tentando abrir a transmissão…");
    return pronta;
  }

  function voltarAoDireto(videoDetectado = false) {
    if (!livePendente) return;
    livePendente = false;
    preparacaoId++;
    const eraTransmissao = transmissaoPendente;
    compartilhamentoPendente = false;
    transmissaoPendente = false;
    adicionandoOutraLive = false;
    vpnPreparadaEm = 0;
    clearTimeout(timerDireto);
    clearTimeout(timerMonitor);
    timerMonitor = 0;
    const token = sessaoVpn;
    sessaoVpn = 0;
    registrar(
      videoDetectado ? "quadros_confirmados" : "prazo_sem_quadros",
      `sessao=${token || 0}`
    );
    encerrarSessao(token).then(() => {
      const texto = videoDetectado
        ? "Quadros de vídeo confirmados; VPN desligada e conexão direta restaurada."
        : eraTransmissao
          ? "O Dorion não confirmou a transmissão; VPN desligada por segurança."
        : "Nenhum vídeo com quadros em avanço foi confirmado; VPN desligada por segurança.";
      const caixa = aviso(texto);
      setTimeout(() => caixa.remove(), 3500);
    }).catch(erro => {
      aviso("Não foi possível confirmar o retorno à conexão direta.");
      console.warn("[Dorion GoLive] não foi possível estabilizar", erro);
    });
  }

  async function cancelarPreparacao(motivo) {
    if (!livePendente) return;
    livePendente = false;
    preparacaoId++;
    compartilhamentoPendente = false;
    transmissaoPendente = false;
    adicionandoOutraLive = false;
    vpnPreparadaEm = 0;
    clearTimeout(timerDireto);
    clearTimeout(timerMonitor);
    timerDireto = 0;
    timerMonitor = 0;
    try {
      const token = sessaoVpn;
      sessaoVpn = 0;
      registrar("acao_cancelada", `${motivo}; sessao=${token || 0}`);
      await encerrarSessao(token);
      const caixa = aviso(motivo);
      setTimeout(() => caixa.remove(), 3500);
    } catch (erro) {
      aviso("A seleção foi cancelada, mas não consegui desligar a VPN temporária.");
      console.warn("[Dorion GoLive] falha ao cancelar preparação", erro);
    }
  }

  function programarDireto(ms = 35000, videoDetectado = false) {
    clearTimeout(timerDireto);
    timerDireto = setTimeout(() => voltarAoDireto(videoDetectado), ms);
  }

  function iniciarMonitoramentoDeQuadros() {
    clearTimeout(timerMonitor);
    amostrasVideo = new WeakMap();
    const destaPreparacao = preparacaoId;

    const verificar = () => {
      if (!livePendente || cliqueLiberadoEm <= 0 ||
        destaPreparacao !== preparacaoId) return;

      const videos = [...document.querySelectorAll("video")];
      const ativos = videos.filter(videoEstaRenderizando);
      for (const video of ativos) {
        const atual = estadoDoVideo(video);
        const anterior = amostrasVideo.get(video);
        const avancou = anterior && (atual.quadros > anterior.quadros ||
          atual.tempo > anterior.tempo + 0.05);
        const fonte = assinaturaDaFonte(video);
        const eMidiaAdicional = ativos.length > videosAtivosAntes &&
          (!fonte || !fontesAntes.has(fonte));
        const pertenceAAcao = !adicionandoOutraLive || eMidiaAdicional;
        const sequencia = avancou && pertenceAAcao
          ? (anterior.sequencia || 0) + 1
          : 0;
        amostrasVideo.set(video, { ...atual, sequencia });

        // Três amostras consecutivas significam quadros/tempo avançando por
        // mais de um instante. `playing`, sozinho, não confirma isso.
        if (sequencia >= 3) {
          registrar(
            "video_em_avanco",
            `quadros=${atual.quadros}; tempo=${atual.tempo.toFixed(2)}`
          );
          voltarAoDireto(true);
          return;
        }
      }
      timerMonitor = setTimeout(verificar, 500);
    };

    verificar();
  }

  async function finalizarTransmissaoConfirmada() {
    if (!livePendente || !transmissaoPendente) return;
    livePendente = false;
    preparacaoId++;
    compartilhamentoPendente = false;
    transmissaoPendente = false;
    vpnPreparadaEm = 0;
    clearTimeout(timerDireto);
    clearTimeout(timerMonitor);
    timerDireto = 0;
    timerMonitor = 0;
    const token = sessaoVpn;
    sessaoVpn = 0;
    await registrar(
      "transmissao_confirmada_pelo_dorion",
      `geracao>${geracaoStreamingAntes}; sessao=${token || 0}`
    );
    await encerrarSessao(token).catch(erro => {
      console.warn("[Dorion GoLive] falha ao limpar sessão da transmissão", erro);
    });
    const caixa = aviso(
      "Transmissão confirmada; VPN desligada e conexão direta restaurada."
    );
    setTimeout(() => caixa.remove(), 3500);
  }

  function iniciarMonitoramentoDeTransmissao() {
    clearTimeout(timerMonitor);
    const destaPreparacao = preparacaoId;
    const verificar = async () => {
      if (!livePendente || !transmissaoPendente ||
          destaPreparacao !== preparacaoId) return;
      try {
        const resposta = await chamarServico(
          `/stream-state?after=${encodeURIComponent(geracaoStreamingAntes)}`
        );
        if (resposta.headers?.get?.("x-dorion-golive-streaming-ready") === "1") {
          await finalizarTransmissaoConfirmada();
          return;
        }
      } catch (erro) {
        registrar("consulta_streaming_falhou", erro?.message || erro);
      }
      timerMonitor = setTimeout(verificar, 500);
    };
    verificar();
  }

  function reencontrarBotao(original) {
    if (original.isConnected !== false) return original;
    const aria = semAcento(original.getAttribute?.("aria-label"));
    const titulo = semAcento(original.getAttribute?.("title"));
    const eraOlhoContextual = eOlhoAoLadoDeAssistir(original);
    const candidatos = document.querySelectorAll("button, [role='button']");
    for (const candidato of candidatos) {
      if (!acaoDeLive(candidato)) continue;
      const ariaAtual = semAcento(candidato.getAttribute?.("aria-label"));
      const tituloAtual = semAcento(candidato.getAttribute?.("title"));
      if ((aria && ariaAtual === aria) || (titulo && tituloAtual === titulo)) {
        return candidato;
      }
      if (eraOlhoContextual && eOlhoAoLadoDeAssistir(candidato)) {
        return candidato;
      }
    }
    return null;
  }

  function botaoDesabilitado(botao) {
    return botao.matches(":disabled") ||
      botao.getAttribute("aria-disabled") === "true";
  }

  function capturarAcaoReact(botao, preferirPressao = false) {
    const propriedades = Object.keys(botao)
      .find(chave => chave.startsWith("__reactProps$"));
    const props = propriedades ? botao[propriedades] : null;
    if (!props) return null;
    const nomes = preferirPressao
      ? ["onPointerDown", "onMouseDown", "onClick"]
      : ["onClick", "onPointerDown", "onMouseDown"];
    const nome = nomes.find(candidato => typeof props[candidato] === "function");
    return nome ? { nome, executar: props[nome], botao } : null;
  }

  function eventoParaReact(tipo, botao) {
    const nativo = {
      type: tipo.toLowerCase(), target: botao, currentTarget: botao,
      button: 0, buttons: tipo === "onClick" ? 0 : 1,
      isTrusted: true, pointerType: "mouse", isPrimary: true,
      preventDefault() {}, stopPropagation() {}, stopImmediatePropagation() {},
    };
    return {
      ...nativo,
      nativeEvent: nativo,
      persist() {},
      isDefaultPrevented() { return false; },
      isPropagationStopped() { return false; },
    };
  }

  async function aguardarBotaoPronto(original, prazoMs = 12000) {
    const limite = Date.now() + prazoMs;
    do {
      const atual = reencontrarBotao(original);
      if (atual && !botaoDesabilitado(atual)) return atual;
      await esperar(100);
    } while (livePendente && Date.now() < limite);
    return null;
  }

  async function repetirClique(botao) {
    if (agendados.has(botao) || repeticaoEmAndamento) {
      registrar("gesto_duplicado_ignorado", rotuloProprio(botao));
      return;
    }
    agendados.add(botao);
    repeticaoEmAndamento = true;
    // O cartão de transmissão é um popover temporário. A troca de rota pode
    // removê-lo antes de a VPN ficar pronta; a função React guardada ainda é
    // a ação exata do botão escolhido e não depende de coordenadas ou texto.
    const acaoReact = capturarAcaoReact(botao, eBotaoAdicionarLive(botao));
    registrar(
      "acao_do_botao_capturada",
      acaoReact ? acaoReact.nome : "sem manipulador React; será usado o DOM"
    );
    try {
      if (!await preparar()) return;
      let atual = reencontrarBotao(botao);
      let acaoAtual = atual
        ? capturarAcaoReact(atual, eBotaoAdicionarLive(atual))
        : null;
      const acao = acaoAtual || acaoReact;
      if (!acao) {
        atual = await aguardarBotaoPronto(botao);
        if (!atual) {
          await cancelarPreparacao(
            "A interface da live não ficou pronta; VPN temporária desligada."
          );
          return;
        }
        acaoAtual = capturarAcaoReact(atual, eBotaoAdicionarLive(atual));
      }
      cliqueLiberadoEm = Date.now();
      compartilhamentoPendente = eCompartilhamento(atual || botao);
      transmissaoPendente = eCompartilhamento(atual || botao);
      adicionandoOutraLive = adicionandoOutraLive || eBotaoAdicionarLive(atual || botao);
      await registrar(
        "acao_liberada_apos_vpn",
        `${rotuloProprio(atual || botao) || "botão sem rótulo"}; sessao=${sessaoVpn}`
      );
      if (acaoAtual || acaoReact) {
        const escolhida = acaoAtual || acaoReact;
        await Promise.resolve(
          escolhida.executar(eventoParaReact(escolhida.nome, escolhida.botao))
        );
        registrar("acao_react_executada", escolhida.nome);
      } else {
        liberados.add(atual);
      // O olho contextual do Discord reage no pointerdown, enquanto os botões
      // com texto normalmente reagem no click. Reproduzir ambos depois da VPN
      // cobre os dois controles; o WeakSet deixa esta sequência passar uma vez.
        if (adicionandoOutraLive && typeof PointerEvent === "function") {
          atual.dispatchEvent(new PointerEvent("pointerdown", {
            bubbles: true, cancelable: true, composed: true,
            pointerType: "mouse", isPrimary: true, button: 0, buttons: 1,
          }));
          atual.dispatchEvent(new PointerEvent("pointerup", {
            bubbles: true, cancelable: true, composed: true,
            pointerType: "mouse", isPrimary: true, button: 0, buttons: 0,
          }));
        }
        atual.click();
        registrar("clique_dom_repetido_no_dorion", rotuloProprio(atual));
      }
      aviso(transmissaoPendente
        ? "Aguardando o Dorion confirmar a transmissão…"
        : "Aguardando quadros reais da transmissão…");
      if (transmissaoPendente) iniciarMonitoramentoDeTransmissao();
      else iniciarMonitoramentoDeQuadros();
      // Durante o seletor de tela, o clique de cancelar/Esc encerra antes.
      // Este prazo nunca declara sucesso; apenas impede VPN esquecida se não
      // houver qualquer mídia renderizada ou se a interface mudar.
      programarDireto(transmissaoPendente ? 45000 : 35000);
    } catch (erro) {
      registrar("falha_ao_executar_acao", erro?.message || erro);
      await cancelarPreparacao(
        "A ação da transmissão falhou; VPN temporária desligada."
      );
    } finally {
      agendados.delete(botao);
      repeticaoEmAndamento = false;
    }
  }

  document.addEventListener("pointerdown", evento => {
    const botao = acaoDeLive(evento.target);
    if (!botao) {
      const controle = evento.target instanceof Element
        ? evento.target.closest("button, [role='button']")
        : null;
      if (controle) registrar("controle_nao_reconhecido", descreverControle(controle));
      return;
    }
    if (liberados.has(botao)) return;
    compartilhamentoPendente = eCompartilhamento(botao);
    transmissaoPendente = eCompartilhamento(botao);
    adicionandoOutraLive = eBotaoAdicionarLive(botao);
    // Nenhuma parte da ação pode chegar cedo ao Discord: o olho contextual
    // responde já no pointerdown. A sequência completa será reproduzida quando
    // o túnel e a reconexão estiverem prontos.
    evento.preventDefault();
    evento.stopImmediatePropagation?.();
    registrar("pointerdown_bloqueado", rotuloProprio(botao));
    repetirClique(botao);
  }, true);

  // O Discord pode iniciar a ação em qualquer etapa do gesto do mouse. Todas
  // permanecem bloqueadas até `acao_liberada_apos_vpn`; impedir apenas o
  // `click` permite que um handler de `mousedown` comece a negociação cedo.
  for (const tipo of ["mousedown", "pointerup", "mouseup"]) {
    document.addEventListener(tipo, evento => {
      const botao = acaoDeLive(evento.target);
      if (!botao || liberados.has(botao)) return;
      evento.preventDefault();
      evento.stopImmediatePropagation?.();
      registrar(`${tipo}_bloqueado`, rotuloProprio(botao));
      if (!livePendente && !preparando) {
        compartilhamentoPendente = eCompartilhamento(botao);
        transmissaoPendente = eCompartilhamento(botao);
        adicionandoOutraLive = eBotaoAdicionarLive(botao);
        repetirClique(botao);
      }
    }, true);
  }

  document.addEventListener("click", evento => {
    if (compartilhamentoPendente && eCancelamento(evento.target)) {
      // Não bloqueia o clique: o Discord fecha o seletor normalmente.
      setTimeout(() => cancelarPreparacao(
        "Compartilhamento cancelado; VPN temporária desligada."
      ), 0);
      return;
    }
    const botao = acaoDeLive(evento.target);
    if (!botao) return;
    if (liberados.delete(botao)) {
      registrar("click_liberado_para_o_dorion", rotuloProprio(botao));
      return;
    }
    adicionandoOutraLive = eBotaoAdicionarLive(botao);
    evento.preventDefault();
    evento.stopImmediatePropagation();
    registrar("click_original_bloqueado", rotuloProprio(botao));
    repetirClique(botao);
  }, true);

  document.addEventListener("keydown", evento => {
    if (compartilhamentoPendente && evento.key === "Escape") {
      setTimeout(() => cancelarPreparacao(
        "Compartilhamento cancelado; VPN temporária desligada."
      ), 0);
    }
  }, true);

  // No navegador, getDisplayMedia abre o seletor. Primeiro o usuário escolhe
  // a fonte; o MediaStream obtido fica retido aqui até a VPN ser confirmada e
  // só então é devolvido ao Discord para iniciar a transmissão.
  const midia = navigator.mediaDevices;
  if (midia?.getDisplayMedia && !midia.getDisplayMedia.__dorionGoLive) {
    const original = midia.getDisplayMedia.bind(midia);
    const substituta = async opcoes => {
      let stream;
      registrar("seletor_de_tela_aberto", "VPN ainda não solicitada por esta escolha");
      try {
        stream = await original(opcoes);
      } catch (erro) {
        registrar("seletor_cancelado_ou_falhou", erro?.message || erro);
        throw erro;
      }

      compartilhamentoPendente = true;
      transmissaoPendente = true;
      registrar("fonte_de_tela_selecionada", "captura retida; aguardando VPN");
      const pronta = await preparar();
      if (!pronta) {
        for (const trilha of stream?.getTracks?.() || []) trilha.stop?.();
        registrar("captura_nao_liberada", "VPN não confirmada; trilhas encerradas");
        throw new Error("VPN temporária não confirmada para compartilhar a tela");
      }
      compartilhamentoPendente = false;
      await registrar("captura_liberada_apos_vpn", `sessao=${sessaoVpn}`);
      cliqueLiberadoEm = Date.now();
      iniciarMonitoramentoDeTransmissao();
      programarDireto(45000);
      return stream;
    };
    substituta.__dorionGoLive = true;
    try {
      midia.getDisplayMedia = substituta;
      registrar("interceptador_getDisplayMedia_instalado");
    } catch (erro) {
      registrar("falha_ao_instalar_getDisplayMedia", erro?.message || erro);
    }
  }

  function resumoDeRestricoes(opcoes) {
    try {
      return JSON.stringify(opcoes, (_chave, valor) =>
        typeof valor === "function" ? "[função]" : valor
      ).slice(0, 500);
    } catch (_) {
      return String(opcoes).slice(0, 500);
    }
  }

  function eCapturaDesktop(opcoes) {
    const resumo = resumoDeRestricoes(opcoes).toLowerCase();
    return /chromemediasource|displaysurface|logicalsurface|desktop|screen|window/.test(resumo);
  }

  // Clientes Discord que imitam Electron solicitam a fonte escolhida por
  // getUserMedia com `chromeMediaSource: desktop`, não por getDisplayMedia.
  if (midia?.getUserMedia && !midia.getUserMedia.__dorionGoLive) {
    const originalUsuario = midia.getUserMedia.bind(midia);
    const substitutaUsuario = async opcoes => {
      const resumo = resumoDeRestricoes(opcoes);
      const desktop = eCapturaDesktop(opcoes);
      registrar(
        desktop ? "getUserMedia_desktop_interceptado" : "getUserMedia_comum_observado",
        resumo
      );
      if (!desktop) return originalUsuario(opcoes);

      compartilhamentoPendente = true;
      transmissaoPendente = true;
      const pronta = await preparar();
      if (!pronta) {
        registrar("getUserMedia_desktop_bloqueado", "VPN não confirmada");
        throw new Error("VPN temporária não confirmada para compartilhar a tela");
      }
      await registrar("getUserMedia_desktop_liberado_apos_vpn", `sessao=${sessaoVpn}`);
      try {
        const stream = await originalUsuario(opcoes);
        compartilhamentoPendente = false;
        registrar("fonte_desktop_obtida", `sessao=${sessaoVpn}`);
        cliqueLiberadoEm = Date.now();
        iniciarMonitoramentoDeTransmissao();
        programarDireto(45000);
        return stream;
      } catch (erro) {
        registrar("getUserMedia_desktop_falhou", erro?.message || erro);
        await cancelarPreparacao("Compartilhamento cancelado; VPN temporária desligada.");
        throw erro;
      }
    };
    substitutaUsuario.__dorionGoLive = true;
    try {
      midia.getUserMedia = substitutaUsuario;
      registrar("interceptador_getUserMedia_instalado");
    } catch (erro) {
      registrar("falha_ao_instalar_getUserMedia", erro?.message || erro);
    }
  }

  console.log("[Dorion GoLive] detector carregado");
})();
