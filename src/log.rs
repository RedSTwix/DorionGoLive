use std::{
    io::Write,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const TAMANHO_MAXIMO: u64 = 512 * 1024;

fn tranca() -> &'static Mutex<()> {
    static TRAVA: OnceLock<Mutex<()>> = OnceLock::new();
    TRAVA.get_or_init(|| Mutex::new(()))
}

pub fn linha(mensagem: &str) {
    let _guarda = tranca().lock();
    let caminho = crate::caminho_log();
    if std::fs::metadata(&caminho).map(|m| m.len()).unwrap_or(0) > TAMANHO_MAXIMO {
        let _ = std::fs::write(&caminho, b"");
    }
    if let Ok(mut arquivo) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(caminho)
    {
        let agora = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duracao| duracao.as_millis())
            .unwrap_or(0);
        let _ = writeln!(arquivo, "[{agora}] {mensagem}");
    }
    println!("{mensagem}");
}
