import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

test("o plugin retém a ação, prepara e restaura o direto ao tocar vídeo", async () => {
  const eventos = new Map();
  const chamadas = [];
  const timers = new Map();
  const confirmacoesVpn = [];
  let proximoTimer = 1;
  let proximaSessao = 1;
  let geracaoStreaming = 0;
  let rejeitarCaptura = false;

  class Elemento {
    constructor(nome = "") {
      this.nome = nome;
      this.style = {};
      this.parentElement = null;
      this.previousElementSibling = null;
      this.textContent = "";
      this.srcObject = null;
      this.cliques = 0;
      this.eventosDespachados = [];
    }
    closest() { return this; }
    getAttribute(chave) { return chave === "aria-label" ? this.nome : null; }
    matches(seletor) { return seletor === "button, [role='button']"; }
    dispatchEvent(evento) { this.eventosDespachados.push(evento.type); }
    click() { this.cliques++; }
    remove() {}
  }
  class Video extends Elemento {
    constructor() {
      super();
      this.readyState = 4;
      this.paused = false;
      this.ended = false;
      this.videoWidth = 1280;
      this.videoHeight = 720;
      this.currentTime = 0;
      this.quadros = 0;
    }
    getVideoPlaybackQuality() { return { totalVideoFrames: this.quadros }; }
  }
  class EventoSintetico {
    constructor(tipo) { this.type = tipo; }
  }

  const corpo = new Elemento();
  corpo.appendChild = () => {};
  const documento = {
    body: corpo,
    candidatos: [],
    videos: [],
    addEventListener(tipo, fn) { eventos.set(tipo, fn); },
    getElementById() { return null; },
    createElement() { return new Elemento(); },
    querySelectorAll(seletor) {
      return seletor === "video" ? this.videos : this.candidatos;
    },
  };
  const midia = {
    async getDisplayMedia() {
      chamadas.push("seletor");
      if (rejeitarCaptura) throw new Error("seleção cancelada");
      return { id: "captura" };
    },
    async getUserMedia(opcoes) {
      chamadas.push(["getUserMedia", opcoes]);
      return { id: "midia-usuario" };
    },
  };

  const contexto = vm.createContext({
    console,
    Date,
    Element: Elemento,
    HTMLVideoElement: Video,
    PointerEvent: EventoSintetico,
    document: documento,
    navigator: { mediaDevices: midia },
    clearTimeout(id) { timers.delete(id); },
    setTimeout(fn, ms) {
      const id = proximoTimer++;
      timers.set(id, { fn, ms });
      return id;
    },
    window: {
      nativeFetch: async () => ({}),
      async fetch(url) {
            chamadas.push(["http", url]);
            if (url.endsWith("/start")) {
              return new Promise(resolve => confirmacoesVpn.push(() => {
                const sessao = proximaSessao++;
                resolve({
                  ok: true,
                  status: 200,
                  headers: {
                    get(nome) {
                      if (nome === "x-dorion-golive-ready") return "1";
                      if (nome === "x-dorion-golive-session") return String(sessao);
                      if (nome === "x-dorion-golive-streaming-generation") {
                        return String(geracaoStreaming);
                      }
                      return null;
                    },
                  },
                });
              }));
            }
            if (url.includes("/stream-state?after=")) {
              const depoisDe = Number(url.split("after=")[1]);
              return {
                ok: true,
                status: 200,
                headers: {
                  get(nome) {
                    if (nome === "x-dorion-golive-streaming-ready") {
                      return geracaoStreaming > depoisDe ? "1" : "0";
                    }
                    return null;
                  },
                },
              };
            }
            return { ok: true, status: 200, headers: { get() { return null; } } };
      },
    },
  });
  contexto.window.window = contexto.window;
  contexto.window.document = documento;
  contexto.window.navigator = contexto.navigator;

  const codigo = await readFile(new URL("../src/dorion-plugin/DorionGoLive.js", import.meta.url), "utf8");
  vm.runInContext(codigo, contexto);

  async function confirmarVpn() {
    assert.ok(confirmacoesVpn.length, "há uma confirmação de VPN pendente");
    confirmacoesVpn.shift()();
    await new Promise(resolve => setImmediate(resolve));
    await new Promise(resolve => setImmediate(resolve));
  }

  const iniciouVpn = () => chamadas.some(
    x => Array.isArray(x) && x[0] === "http" && x[1].endsWith("/start")
  );
  const encerrouVpn = () => chamadas.some(
    x => Array.isArray(x) && x[0] === "http" && x[1].includes("/stable?session=")
  );

  async function confirmarQuadros(video) {
    const antes = chamadas.filter(
      x => Array.isArray(x) && x[0] === "http" && x[1].includes("/stable?session=")
    ).length;
    for (let i = 0; i < 5; i++) {
      const entrada = [...timers.entries()].find(([, t]) => t.ms === 500);
      assert.ok(entrada, "monitor de quadros continua ativo");
      const [id, timer] = entrada;
      timers.delete(id);
      video.quadros++;
      video.currentTime += 0.5;
      timer.fn();
      await new Promise(resolve => setImmediate(resolve));
      const depois = chamadas.filter(
        x => Array.isArray(x) && x[0] === "http" && x[1].includes("/stable?session=")
      ).length;
      if (depois > antes) return;
    }
    assert.fail("quadros em avanço não confirmaram o vídeo");
  }

  async function confirmarTransmissaoNativa() {
    const antes = chamadas.filter(
      x => Array.isArray(x) && x[0] === "http" && x[1].includes("/stable?session=")
    ).length;
    geracaoStreaming++;
    for (let i = 0; i < 5; i++) {
      await new Promise(resolve => setImmediate(resolve));
      const entrada = [...timers.entries()].find(([, t]) => t.ms === 500);
      if (entrada) {
        const [id, timer] = entrada;
        timers.delete(id);
        timer.fn();
      }
      await new Promise(resolve => setImmediate(resolve));
      const depois = chamadas.filter(
        x => Array.isArray(x) && x[0] === "http" && x[1].includes("/stable?session=")
      ).length;
      if (depois > antes) return;
    }
    assert.fail("estado streaming do Dorion não encerrou a sessão da transmissão");
  }

  // Reproduz o clique circulado na imagem: a área não é botão, embora um pai
  // amplo contenha textos de live. Isso jamais pode iniciar a VPN.
  const vazio = new Elemento();
  vazio.textContent = "alguém AO VIVO Assista à transmissão";
  vazio.closest = () => null;
  eventos.get("pointerdown")({ target: vazio, preventDefault() {} });
  eventos.get("click")({
    target: vazio,
    preventDefault() { throw new Error("clique vazio não pode ser bloqueado"); },
    stopImmediatePropagation() {},
  });
  assert.deepEqual(chamadas, [], "clique fora de botão não aciona a VPN");

  const botao = new Elemento("Stream");
  let evitou = false;
  let parou = false;
  eventos.get("pointerdown")({ target: botao, preventDefault() {} });
  let mousedownEvitado = false;
  let mousedownParado = false;
  eventos.get("mousedown")({
    target: botao,
    preventDefault() { mousedownEvitado = true; },
    stopImmediatePropagation() { mousedownParado = true; },
  });
  eventos.get("click")({
    target: botao,
    preventDefault() { evitou = true; },
    stopImmediatePropagation() { parou = true; },
  });
  await new Promise(resolve => setImmediate(resolve));
  const botaoAtual = new Elemento("Stream");
  botao.isConnected = false;
  documento.candidatos = [botaoAtual];
  assert.equal(botaoAtual.cliques, 0, "não clica antes da confirmação real da VPN");
  assert.equal(mousedownEvitado, true, "também bloqueia o mousedown do Discord");
  assert.equal(mousedownParado, true, "o handler precoce do Discord não recebe mousedown");
  assert.equal(
    [...timers.values()].some(t => t.ms === 10000),
    false,
    "não usa espera fixa para presumir que a VPN conectou"
  );
  await confirmarVpn();

  assert.equal(iniciouVpn(), true);
  assert.equal(botao.cliques, 0, "não clica no nó antigo substituído pelo React");
  assert.equal(botaoAtual.cliques, 1, "reencontra e clica no botão atual");
  assert.equal(
    chamadas.some(x => Array.isArray(x) && x[1].includes("acao_liberada_apos_vpn")),
    true,
    "registra que só liberou a ação depois da confirmação"
  );
  assert.equal(evitou, true);
  assert.equal(parou, true);

  // Quando a confirmação final já preparou a VPN, a captura selecionada
  // reutiliza a mesma sessão e é liberada sem abrir outra conexão.
  chamadas.length = 0;
  await midia.getDisplayMedia();
  assert.equal(chamadas.filter(x => x === "seletor").length, 1);
  assert.equal(
    chamadas.some(x => Array.isArray(x) && x[1].includes("captura_liberada_apos_vpn")),
    true
  );

  await confirmarTransmissaoNativa();
  assert.equal(encerrouVpn(), true);

  const primeiroVideo = new Video();
  primeiroVideo.srcObject = { id: "live-principal" };
  documento.videos = [primeiroVideo];

  // Sem um botão final reconhecido, o seletor abre primeiro. Depois da
  // escolha, o stream fica retido e não volta ao Discord antes da VPN.
  chamadas.length = 0;
  const escolherTela = new Elemento("Compartilhar sua tela");
  let escolhaBloqueada = false;
  eventos.get("pointerdown")({
    target: escolherTela,
    preventDefault() { escolhaBloqueada = true; },
  });
  assert.equal(escolhaBloqueada, false, "o botão que abre a escolha passa direto");
  const capturaDireta = midia.getDisplayMedia();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(
    chamadas.includes("seletor"),
    true,
    "abre a escolha de tela antes de solicitar a VPN"
  );
  assert.equal(iniciouVpn(), true, "a escolha concluída solicita a VPN");
  await confirmarVpn();
  await capturaDireta;
  assert.equal(
    chamadas.some(x => Array.isArray(x) && x[1].includes("captura_liberada_apos_vpn")),
    true,
    "só devolve a captura ao Discord depois da VPN"
  );
  await confirmarTransmissaoNativa();

  // O botão verde mostrado na segunda imagem é uma ação exata de assistir.
  chamadas.length = 0;
  const assistir = new Elemento("Assista à transmissão");
  eventos.get("pointerdown")({ target: assistir, preventDefault() {} });
  eventos.get("click")({ target: assistir, preventDefault() {}, stopImmediatePropagation() {} });
  await new Promise(resolve => setImmediate(resolve));
  await confirmarVpn();
  assert.equal(assistir.cliques, 1);
  assert.deepEqual(
    assistir.eventosDespachados,
    [],
    "o botão verde usa somente click, sem a sequência especial do olho"
  );
  assert.equal(iniciouVpn(), true);
  await confirmarQuadros(primeiroVideo);

  // O fluxo Electron/WebView usa getUserMedia com fonte desktop. A chamada
  // original também fica retida até a VPN; microfone comum não fica.
  chamadas.length = 0;
  const capturaDesktop = midia.getUserMedia({
    video: { mandatory: { chromeMediaSource: "desktop", chromeMediaSourceId: "tela:1" } },
  });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(
    chamadas.some(x => Array.isArray(x) && x[0] === "getUserMedia"),
    false,
    "não pede a fonte desktop antes da confirmação da VPN"
  );
  await confirmarVpn();
  await capturaDesktop;
  assert.equal(
    chamadas.some(x => Array.isArray(x) && x[0] === "getUserMedia"),
    true,
    "libera a fonte desktop depois da VPN"
  );
  await confirmarTransmissaoNativa();

  chamadas.length = 0;
  await midia.getUserMedia({ audio: true });
  assert.equal(iniciouVpn(), false, "microfone comum não liga a VPN");

  // A troca da rota pode desmontar o popover e remover o botão. A ação React
  // capturada antes da VPN ainda deve abrir a live sem um segundo clique.
  chamadas.length = 0;
  let acoesReact = 0;
  const assistirTransitorio = new Elemento("Assista à transmissão");
  assistirTransitorio.__reactProps$teste = {
    onClick() { acoesReact++; },
  };
  eventos.get("pointerdown")({ target: assistirTransitorio, preventDefault() {} });
  assistirTransitorio.isConnected = false;
  documento.candidatos = [];
  await new Promise(resolve => setImmediate(resolve));
  await confirmarVpn();
  assert.equal(acoesReact, 1, "executa a ação guardada mesmo sem o botão no DOM");
  assert.equal(
    chamadas.some(x => Array.isArray(x) && x[1].includes("acao_react_executada")),
    true
  );
  await confirmarQuadros(primeiroVideo);

  // O olho contextual para adicionar outra live pode vir sem rótulo. Só é
  // reconhecido se estiver imediatamente ao lado do botão verde de assistir;
  // o clique real normalmente chega pelo SVG interno.
  chamadas.length = 0;
  const assistirIrmao = new Elemento("Assista à transmissão");
  const olho = new Elemento();
  olho.previousElementSibling = assistirIrmao;
  olho.isConnected = false;
  const assistirIrmaoAtual = new Elemento("Assista à transmissão");
  const olhoAtual = new Elemento();
  olhoAtual.previousElementSibling = assistirIrmaoAtual;
  documento.candidatos = [olhoAtual];
  const iconeDoOlho = new Elemento();
  iconeDoOlho.closest = () => olho;
  eventos.get("pointerdown")({ target: iconeDoOlho, preventDefault() {} });
  eventos.get("click")({
    target: iconeDoOlho,
    preventDefault() {},
    stopImmediatePropagation() {},
  });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(
    encerrouVpn(),
    false,
    "a live já aberta não confirma a segunda antes do clique repetido"
  );
  await confirmarVpn();
  assert.equal(olho.cliques, 0, "não clica no olho antigo removido pelo Dorion");
  assert.equal(olhoAtual.cliques, 1, "reencontra e clica no novo botão do olho");
  assert.deepEqual(
    olhoAtual.eventosDespachados,
    ["pointerdown", "pointerup"],
    "repete também a pressão usada pelo olho contextual"
  );
  assert.equal(iniciouVpn(), true);
  const segundoVideo = new Video();
  segundoVideo.srcObject = { id: "live-adicionada" };
  documento.videos.push(segundoVideo);
  await confirmarQuadros(segundoVideo);

  // Quando há rótulo próprio, não depende da posição no cartão/linha.
  chamadas.length = 0;
  const adicionarOutra = new Elemento("Adicionar outra transmissão");
  eventos.get("pointerdown")({ target: adicionarOutra, preventDefault() {} });
  eventos.get("click")({
    target: adicionarOutra,
    preventDefault() {},
    stopImmediatePropagation() {},
  });
  await new Promise(resolve => setImmediate(resolve));
  await confirmarVpn();
  assert.equal(adicionarOutra.cliques, 1);
  const [prazoAdicionarId, prazoAdicionar] = [...timers.entries()].find(([, t]) => t.ms === 35000);
  timers.delete(prazoAdicionarId);
  prazoAdicionar.fn();
  await new Promise(resolve => setImmediate(resolve));

  // Cancelar a janela interna de compartilhamento desliga imediatamente.
  chamadas.length = 0;
  const compartilhar = new Elemento("Transmitir");
  eventos.get("pointerdown")({ target: compartilhar, preventDefault() {} });
  eventos.get("click")({ target: compartilhar, preventDefault() {}, stopImmediatePropagation() {} });
  await new Promise(resolve => setImmediate(resolve));
  await confirmarVpn();
  const cancelar = new Elemento("Cancelar");
  eventos.get("click")({ target: cancelar, preventDefault() {}, stopImmediatePropagation() {} });
  const [cancelarId, cancelarTimer] = [...timers.entries()].find(([, t]) => t.ms === 0);
  timers.delete(cancelarId);
  cancelarTimer.fn();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(encerrouVpn(), true);

  // O cancelamento do seletor nativo também encerra e preserva o erro original.
  chamadas.length = 0;
  rejeitarCaptura = true;
  const compartilharNovo = new Elemento("Compartilhar sua tela");
  eventos.get("pointerdown")({ target: compartilharNovo, preventDefault() {} });
  const capturaCancelada = assert.rejects(
    midia.getDisplayMedia(),
    /seleção cancelada/
  );
  await capturaCancelada;
  assert.equal(iniciouVpn(), false, "cancelar a escolha não chega a ligar a VPN");
});
